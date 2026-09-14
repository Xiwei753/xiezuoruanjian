package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import com.xiwei.sujian.core.interop.diagnostics.EditorDiagnosticsEvents
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import uniffi.writer_core.AnimationModeDto

/**
 * #644 评论 #684：帧协调器 — 解决"Core 一笔事务不等于屏幕一帧"的问题。
 *
 * Core 在一次按键/删除中可能产生多笔事务（2~5 笔），
 * 但 Compose 一帧的 `onTextLayout` 回调只给出一个最终 [ComposeLayoutSnapshot]。
 * 旧实现试图为每笔 Core 事务单独创建动画事务，导致中间的 `TextLayoutResult`
 * 根本不存在、光标抖动、换行重排错位。
 *
 * 本协调器的策略（双向汇合）：
 * - `onVisualIntent()` 只把 Core intent 串进 pending chain，然后 tryStartTransaction()。
 * - `onLayout()` 保存最新真实 layout，然后 tryStartTransaction()。
 * - 两份概念同时保存：
 *   - [lastConsumed]（lastConsumedPresentedLayout）：上一次真正生成事务消费的旧侧 layout，
 *     只在生成事务时推进；首次 layout 设为基线。
 *   - [latest]（latestUnconsumedPresentedLayout）：每一次 onLayout 都更新的最新真实 layout。
 * - 只有满足 `chain.baseText == lastConsumed.text` 且 `chain.targetText == latest.text`
 *   才生成屏幕事务 —— 无论 intent 先到还是 layout 先到都能汇合。
 *
 * 这样快速输入、快速删除、Enter、软换行都只对真实屏幕帧做动画。
 */
class ComposeVisualFrameCoordinator(
    private val targetId: String,
) {
    companion object {
        private const val TAG = "VisualFrameCoord"
    }

    /** 上一次真正生成事务消费的旧侧 layout（基线）；首次 layout 设为基线，之后只在生成时推进。 */
    private var lastConsumed: PresentedLayout? = null

    /** 最新真实 layout — 每一次 onLayout 都更新。 */
    private var latest: PresentedLayout? = null

    /** 中间积累的 Core intent chain — 等匹配的真实 layout 到达后一起合成事务。 */
    private var pending: PendingVisualChain? = null

    /** 当前正在跑的视觉事务 — overlay 读取。 */
    private var active: ComposeVisualTransaction? = null

    /** 单调递增的事务 ID。 */
    private var nextTransactionId: Long = 0L

    /** 最近一次 onLayout 传入的动画策略 — 供 tryStartTransaction 在 intent 先到时也用到。 */
    private var lastMotionPolicy: EditorMotionPolicy = EditorMotionPolicy()

    /**
     * Core intent 到达 — 只串进 pending chain（连续才拼接，不连续不开硬拼），
     * 然后尝试合流生成事务。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        masterProgress: Float,
    ): FrameUpdate {
        val existing = pending
        if (existing == null) {
            pending =
                PendingVisualChain(
                    baseText = intent.expectedOldText,
                    targetText = intent.expectedNewText,
                    intents = listOf(intent),
                )
        } else {
            // 连续 chain 必须满足上一笔 expectedNewText == 下一笔 expectedOldText；
            // 不连续就不能硬拼，以当前 intent 重开一条链。
            val lastExpectedNew = existing.intents.last().expectedNewText
            if (lastExpectedNew == intent.expectedOldText) {
                pending =
                    existing.copy(
                        intents = existing.intents + intent,
                        targetText = intent.expectedNewText,
                    )
            } else {
                pending =
                    PendingVisualChain(
                        baseText = intent.expectedOldText,
                        targetText = intent.expectedNewText,
                        intents = listOf(intent),
                    )
            }
        }

        EditorDiagnosticsEvents.editorVisualIntentQueued(
            targetId = targetId,
            coreTransactionId = intent.coreTransactionId,
            baseRevision = intent.baseRevision,
            newRevision = intent.newRevision,
            pendingChainSize = pending?.intents?.size ?: 0,
        )

        return tryStartTransaction(masterProgress)
    }

    /**
     * 真实屏幕布局到达 — 更新最新 layout，然后尝试合流生成冻结事务。
     *
     * 从上一份已呈现 layout（[lastConsumed]）→ 当前 layout（[latest]）
     * + 中间积累的 intent chain → 一个 [ComposeVisualTransaction]，创建后不再修改。
     */
    fun onLayout(
        snapshot: ComposeLayoutSnapshot,
        motionPolicy: EditorMotionPolicy,
        masterProgress: Float,
    ): FrameUpdate {
        latest = PresentedLayout(snapshot.result.layoutInput.text.text, snapshot)
        lastMotionPolicy = motionPolicy

        EditorDiagnosticsEvents.editorLayoutPresented(
            targetId = targetId,
            layoutTextLength = snapshot.result.layoutInput.text.length,
        )

        // 首次 layout 设为基线，使第一笔事务的 base 能匹配上。
        if (lastConsumed == null) {
            lastConsumed = latest
        }

        // #684 评论 5665907509 问题2：没有 pending 文本事务的真实重新排版也要推进 lastConsumed。
        // lastConsumed 必须表示"下一笔动画开始前，屏幕最后真实采用的 TextLayoutResult"。
        // 当没有 pending chain 且没有正在跑的 active transaction 且文本未变时，屏幕已经真实呈现，
        // 直接把 lastConsumed 推进到这份 latest，不需要创建文字动画事务。
        // 否则正文没变但真实 TextLayoutResult 已变（宽度变化、字体/字号变化、窗口/方向变化导致
        // 软换行重排）时，屏幕已在 layout B，coordinator 旧侧基线还停在 layout A，
        // 下一次输入生成 layout C 时事务错误地拿 A→C 做 retained move / cursor geometry。
        // 注意：只在文本相同时推进 — 文本变化但无 pending 属于"layout 先到、intent 后到"的
        // 双向汇合场景，lastConsumed 不能提前推进，否则 intent 到达时 baseText 匹配不上。
        if (pending == null && active == null && lastConsumed?.text == latest?.text) {
            lastConsumed = latest
        }

        val update = tryStartTransaction(masterProgress)
        return update
    }

    /**
     * 双向合流：当 pending chain 与两份 layout 概念同时满足匹配条件时生成事务。
     *
     * 匹配条件（无论 intent 先到还是 layout 先到）：
     * - pending != null
     * - lastConsumed != null && latest != null
     * - latest 不是 lastConsumed 本身（确有新 layout）
     * - pending.baseText == lastConsumed.text
     * - pending.targetText == latest.text
     */
    private fun tryStartTransaction(masterProgress: Float): FrameUpdate {
        val pendingChain = pending
        if (pendingChain == null) {
            // 无 pending：基线不动（只在生成事务时推进）。
            return FrameUpdate.Empty
        }
        val consumed = lastConsumed
        val newest = latest
        if (consumed == null || newest == null) return FrameUpdate.Empty
        if (newest === consumed) return FrameUpdate.Empty
        if (pendingChain.baseText != consumed.text) return FrameUpdate.Empty
        if (pendingChain.targetText != newest.text) return FrameUpdate.Empty

        // 合流生成事务。
        val chain = pendingChain.intents
        val coreTransactionIds = chain.map { it.coreTransactionId }
        // #684 评论 5664636035 Bug1：屏幕事务的 old/new changed ranges 应该直接从 T0→Tn composed
        // offset map 的补集算，不再用 chain.flatMap（中间事务坐标不能直接当屏幕坐标）。
        // 当 composedOffsetMap 为 null（chain 中有笔没有 offset map）时，回退到 chain.flatMap。
        val composedOffsetMapForRanges = ComposeVisualRebase.composeOffsetMapChain(chain)
        val oldLength = consumed.layout.result.layoutInput.text.length
        val newLength = newest.layout.result.layoutInput.text.length
        val mergedOldRanges: List<androidx.compose.ui.text.TextRange>
        val mergedNewRanges: List<androidx.compose.ui.text.TextRange>
        if (composedOffsetMapForRanges != null) {
            val frameChangedRanges =
                ComposeVisualRebase.changedRangesFromComposedMap(
                    composedOffsetMapForRanges,
                    oldLength,
                    newLength,
                )
            mergedOldRanges = frameChangedRanges.oldRanges
            mergedNewRanges = frameChangedRanges.newRanges
        } else {
            mergedOldRanges = chain.flatMap { it.oldRanges }
            mergedNewRanges = chain.flatMap { it.newRanges }
        }

        // #684 评论 5664636035 Bug2：无旧动画时光标起点应从 chain 第一笔 old cursor 起跑，
        // 而非最后一笔。firstCursor 用于 cursorStartRect（T0 坐标），lastCursor 用于 cursorEndRect（Tn 坐标）。
        val firstCursor = chain.mapNotNull { it.cursor }.firstOrNull()
        val lastCursor = chain.mapNotNull { it.cursor }.lastOrNull()
        val lastIntent = chain.last()

        // 计算 retained moves — 用 offset map chain 合成。
        val retainedMoves =
            ComposeVisualRebase.computeRetainedMoves(
                oldLayout = consumed.layout,
                newLayout = newest.layout,
                chain = chain,
            )

        // 计算 cursor start/end rect。
        // #684 评论 5663032418 断点1：中断续跑时，文字用 masterProgress 物化当前屏幕帧，
        // 光标也必须从当前屏幕位置继续，而不是从上一笔的逻辑终点重新起跑。
        // 先用当前活跃事务的 cursorStartRect/cursorEndRect + masterProgress 算出
        // 当前屏幕上的光标位置 interruptedCursorRect，作为下一笔 cursorStartRect 的首选。
        val activeTx = active
        val interruptedCursorRect =
            if (activeTx != null && firstCursor?.animate == true) {
                ComposeVisualRebase.interpolateCursorRect(
                    startRect = activeTx.cursorStartRect,
                    endRect = activeTx.cursorEndRect,
                    progress = masterProgress,
                )
            } else {
                null
            }

        // #684 评论 5664636035 Bug2：无旧动画时从 chain 第一笔 old cursor（T0 坐标）起跑，
        // 不再用最后一笔 old cursor（T(n-1) 坐标）查 T0 布局。
        val logicalOldCursorRect =
            if (firstCursor != null) {
                try {
                    val startOffset =
                        firstCursor.oldEndUtf16
                            .coerceIn(0, consumed.layout.result.layoutInput.text.length)
                    consumed.layout.result.getCursorRect(startOffset)
                } catch (_: Throwable) {
                    null
                }
            } else {
                null
            }

        // 优先用当前屏幕插值位置；没有可物化的光标动画时回退到逻辑旧位置。
        val cursorStartRect = interruptedCursorRect ?: logicalOldCursorRect

        // #684 评论 5664636035 Bug2：cursorEndRect 用最后一笔 new cursor（Tn 坐标）查 newest 布局。
        val cursorEndRect =
            if (lastCursor != null) {
                try {
                    val endOffset =
                        lastCursor.newEndUtf16
                            .coerceIn(0, newest.layout.result.layoutInput.text.length)
                    newest.layout.result.getCursorRect(endOffset)
                } catch (_: Throwable) {
                    null
                }
            } else {
                null
            }

        // 计算 hidden ranges — 由 overlay 接管的范围。
        // 是否隐藏正文必须与 overlay 是否真的画正文收口成同一个判断：
        // SYSTEM_SUPPRESSED 时 overlay 不画自定义正文（textEnabled=false），
        // 这里也必须把正文留为可见（hiddenRanges 为空），否则正文会被隐藏到事务结束。
        //
        // #684 评论 5663862982 Bug1：hiddenRanges 必须包含 retained moves 的 newRanges。
        //   overlay 画 retainedMoves（被挤到下一行的保留文字），但这些 newRange 对应的
        //   系统正文若不隐藏，BasicTextField 在最终新位置的那份正文同时可见 → 重影/跳行。
        // #684 评论 5663862982 Bug2：多笔 intent 合成一个屏幕事务时，上一帧的 suppressed
        //   ranges 必须按整条 chain 的 composedOffsetMap（T0->Tn）映射，而不是最后一笔
        //   replaceBounds（T(n-1)->Tn 坐标）。
        // #684 评论 5664636035 Bug1：屏幕事务的 textKind 按最终净变化决定，不再从最后一笔 intent 读。
        val transactionTextKind =
            when {
                mergedOldRanges.isEmpty() && mergedNewRanges.isEmpty() -> TextVisualKind.None
                mergedOldRanges.isEmpty() -> TextVisualKind.Insert
                mergedNewRanges.isEmpty() -> TextVisualKind.Delete
                else -> TextVisualKind.Move
            }
        // #684 评论 5665907509 问题1：屏幕事务级 customAnimationEnabled —
        // SYSTEM_SUPPRESSED 到来时直接落到系统最终正文，不是"本事务不画，但上一事务继续画"。
        // 当 !customAnimationEnabled（SYSTEM_SUPPRESSED 或 motion policy 关闭文字动画）时：
        // - 不映射/继承 active.suppressedCurrentRanges（mappedPrevSuppressedRanges = emptyList）
        // - 不计算 startFrame（不让上一笔 overlay 动画跨过这笔 suppressed 事务继续跑）
        // overlay 据此 transaction.animationMode 判断 systemSuppressed，不再从 _activeIntent 读取。
        val customAnimationEnabled =
            lastMotionPolicy.textEnabled &&
                lastIntent.animationMode != AnimationModeDto.SYSTEM_SUPPRESSED
        val customTextAnimationEnabled =
            customAnimationEnabled && transactionTextKind != TextVisualKind.None

        // (1) 本事务自己 owned 的 new ranges — Insert/Move 的 newRanges。
        val currentOwnedNewRanges =
            if (customTextAnimationEnabled) {
                when (transactionTextKind) {
                    TextVisualKind.Insert,
                    TextVisualKind.Move,
                    -> mergedNewRanges
                    TextVisualKind.Delete,
                    TextVisualKind.None,
                    -> emptyList()
                }
            } else {
                emptyList()
            }

        // (2) retained moves 的 newRanges — overlay 画 retained 文字时系统正文必须透明。
        val retainedNewRanges =
            if (customTextAnimationEnabled) {
                retainedMoves.map { it.newRange }
            } else {
                emptyList()
            }

        // (3) 上一帧仍由 startFrame 接管、且映射到当前 new text 后仍存活的 suppressed ranges。
        //     用整条 chain 的 composedOffsetMap（T0->Tn）映射，不能用最后一笔 replaceBounds。
        // #684 评论 5665907509 问题1：当 !customAnimationEnabled（SYSTEM_SUPPRESSED 或 motion policy
        //     关闭文字动画）时不映射/继承 active.suppressedCurrentRanges —
        //     上一笔动画留下的正文不应跨过这笔 suppressed 事务继续被 OutputTransformation 设透明。
        val composedOffsetMap = composedOffsetMapForRanges
        val prevSuppressedRanges = active?.suppressedCurrentRanges ?: emptyList()
        val mappedPrevSuppressedRanges =
            if (customAnimationEnabled) {
                ComposeVisualRebase.mapSuppressedRangesThroughOffsetMap(prevSuppressedRanges, composedOffsetMap)
            } else {
                emptyList()
            }

        val hiddenRanges =
            (currentOwnedNewRanges + retainedNewRanges + mappedPrevSuppressedRanges)
                .filter { it.start < it.end }

        // 计算 startFrame — 用真实当前 master progress 物化当前屏幕帧。
        // 动画被下一笔输入打断时，startFrame 从半途继续，而不是假定上一笔已经跑到 1f。
        //
        // #684 评论 5663862982 Bug2：多笔 intent 合成一个屏幕事务时，startFrame 的
        //   targetRange 是 T0 坐标，必须用整条 chain 的 composedOffsetMap（T0->Tn）映射，
        //   而不是最后一笔 replaceBounds（T(n-1)->Tn 坐标）。nextReplaceBounds 仅作回退。
        //   currentSuppressedRanges 传上一帧的 suppressedCurrentRanges（表示"上一帧此刻
        //   已经被系统正文隐藏的 ranges"），不是新事务刚算出的 hiddenRanges。
        // #684 评论 5665907509 问题1：当 !customAnimationEnabled 时不计算 startFrame —
        //   不让上一笔 overlay 动画跨过这笔 suppressed 事务继续跑 startFrame rebase。
        //   SYSTEM_SUPPRESSED 到来时 overlay 应直接落到系统最终正文。
        val rebasedFromId = active?.id
        val startFrame =
            if (customAnimationEnabled) {
                active?.let { current ->
                    ComposeVisualRebase.materializeStartFrame(
                        ComposeVisualRebase.MaterializeStartFrameParams(
                            transaction = current,
                            textProgress = masterProgress,
                            cursorProgress = masterProgress,
                            rebaseProgress = masterProgress,
                            nextOffsetMap = composedOffsetMap,
                            nextReplaceBounds = lastIntent.replaceBounds,
                            currentSuppressedRanges = current.suppressedCurrentRanges,
                            cursorSnapshot = null,
                        ),
                    )
                }
            } else {
                null
            }

        // 从 mergedOldRanges 中减去 startFrame 已接管的 old ranges，
        // 避免 startFrame fading slice 和事务 Delete/Move 路径重复绘制同一段文字。
        val effectiveOldRanges =
            ComposeVisualRebase.subtractRanges(
                mergedOldRanges,
                startFrame?.ownedOldRanges.orEmpty(),
            )

        nextTransactionId++
        val transaction =
            ComposeVisualTransaction(
                id = nextTransactionId,
                coreTransactionIds = coreTransactionIds,
                oldLayout = consumed.layout,
                newLayout = newest.layout,
                intents = chain,
                oldRanges = effectiveOldRanges,
                newRanges = mergedNewRanges,
                retainedMoves = retainedMoves,
                cursorStartRect = cursorStartRect,
                cursorEndRect = cursorEndRect,
                startFrame = startFrame,
                durationMs = lastIntent.durationMs,
                motionPolicy = lastMotionPolicy,
                // #684 评论 5663862982：事务生成后冻结的 suppressed ranges —
                // 下一笔 rebase 时按 composedOffsetMap 映射到新坐标系。
                suppressedCurrentRanges = hiddenRanges,
                // #684 评论 5664636035 Bug1：屏幕事务的 textKind 按最终净变化决定。
                textKind = transactionTextKind,
                // #684 评论 5665907509 问题1：把最终的 animationMode 直接冻结进事务，
                // overlay 据此判断 systemSuppressed，不再从 _activeIntent（最后一笔 Core intent）判断。
                animationMode = lastIntent.animationMode,
            )

        active = transaction
        lastConsumed = newest
        pending = null

        EditorDiagnosticsEvents.editorVisualTransactionStarted(
            targetId = targetId,
            visualTransactionId = transaction.id,
            coreTransactionIds = coreTransactionIds,
            baseRevision = chain.first().baseRevision,
            newRevision = chain.last().newRevision,
            pendingChainSize = chain.size,
            layoutTextLength = newest.layout.result.layoutInput.text.length,
        )
        if (rebasedFromId != null) {
            EditorDiagnosticsEvents.editorVisualTransactionRebased(
                targetId = targetId,
                visualTransactionId = transaction.id,
                rebasedFromVisualTransactionId = rebasedFromId,
                pendingChainSize = chain.size,
            )
        }

        Log.d(
            TAG,
            "transaction_started: id=${transaction.id} coreTxnIds=$coreTransactionIds " +
                "rebasedFrom=$rebasedFromId retained=${retainedMoves.size} " +
                "oldTextLen=${consumed.layout?.result?.layoutInput?.text?.length ?: 0} " +
                "newTextLen=${newest.layout.result.layoutInput.text.length}",
        )

        return FrameUpdate.NewTransaction(transaction, hiddenRanges)
    }

    /**
     * 当前活跃事务 — overlay 读取。
     */
    fun currentTransaction(): ComposeVisualTransaction? = active

    /**
     * 动画完成时由 overlay 通知 — 清除内部 active，避免下一笔拿已结束的旧事务当当前事务。
     */
    fun completeTransaction(transactionId: Long) {
        val current = active ?: return
        if (current.id != transactionId) return
        EditorDiagnosticsEvents.editorVisualTransactionCompleted(
            targetId = targetId,
            visualTransactionId = current.id,
            coreTransactionIds = current.coreTransactionIds,
        )
        // #684 评论 5665907509 问题2：动画结束时，如果最新真实 layout 与事务 newLayout 同文本，
        // 把 lastConsumed 更新到最新真实 layout，避免动画结束后基线仍是旧几何。
        // 动画期间可能发生宽度变化、字体/字号变化、窗口/方向变化导致软换行重排，
        // 屏幕已在 layout B（同文本新几何），但 lastConsumed 仍停在事务开始时的 layout A，
        // 下一次输入生成 layout C 时事务错误地拿 A→C 做 retained move / cursor geometry。
        val newest = latest
        val currentNewText = current.newLayout?.result?.layoutInput?.text?.text
        if (newest != null && currentNewText != null && newest.text == currentNewText) {
            lastConsumed = newest
        }
        active = null
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        lastConsumed = null
        latest = null
        pending = null
        active = null
        nextTransactionId = 0L
    }
}

/**
 * 呈现过的布局快照 — 记录最后一次真正被 overlay 消费的 text 和 layout。
 */
private data class PresentedLayout(
    val text: String,
    val layout: ComposeLayoutSnapshot,
)

/**
 * 中间积累的 Core intent chain — 等 onLayout 到达后一起合成事务。
 *
 * [baseText]/[targetText] 始终携带用于匹配的文本身份（来自每笔 intent 的
 * expectedOldText/expectedNewText），不再永远写成空串。
 */
private data class PendingVisualChain(
    val baseText: String,
    val targetText: String,
    val intents: List<EditorVisualIntent>,
)

/**
 * 帧更新结果 — onLayout 返回。
 */
sealed interface FrameUpdate {
    /** 无新事务（无 pending / 无匹配 layout）。 */
    data object Empty : FrameUpdate

    /** 新事务生成 — overlay 读取 transaction 并更新 hiddenRanges。 */
    data class NewTransaction(
        val transaction: ComposeVisualTransaction,
        val hiddenRanges: List<androidx.compose.ui.text.TextRange>,
    ) : FrameUpdate
}
