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

    /**
     * Core intent 到达 — 只串进 pending chain（连续才拼接，不连续不开硬拼），
     * 然后尝试合流生成事务。
     *
     * #684 评论 5667483662 问题1：motionPolicy 一起存进 pending chain，
     * 让 tryStartTransaction 永远使用和这条 pending chain 同源的 policy，
     * 不再依赖"最近一次 onLayout 顺手记住的 policy"。
     * 传入前先在 visualState 层 effective()，这里直接存。
     *
     * @param motionPolicy 本笔 intent 的 effective 动画策略 — 与 pending chain 同源。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
        masterProgress: Float,
    ): FrameUpdate {
        val existing = pending
        if (existing == null) {
            pending =
                PendingVisualChain(
                    baseText = intent.expectedOldText,
                    targetText = intent.expectedNewText,
                    intents = listOf(intent),
                    motionPolicy = motionPolicy,
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
                        // 同一条 chain 内 policy 以最新一笔为准（连续输入同一设置）。
                        motionPolicy = motionPolicy,
                    )
            } else {
                pending =
                    PendingVisualChain(
                        baseText = intent.expectedOldText,
                        targetText = intent.expectedNewText,
                        intents = listOf(intent),
                        motionPolicy = motionPolicy,
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
     *
     * #684 评论 5667483662 问题1：onLayout 不再接收 motionPolicy —
     * 事务的动画策略由 pending chain 自己携带（与 intent 同源），
     * onLayout 只负责 layout 汇合，不决定这笔事务该用什么动画设置。
     */
    fun onLayout(
        snapshot: ComposeLayoutSnapshot,
        masterProgress: Float,
    ): FrameUpdate {
        latest = PresentedLayout(snapshot.result.layoutInput.text.text, snapshot)

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
        // 而非最后一笔。firstCursor 用于 cursorMotionPath（T0 坐标），lastCursor 用于 cursorMotionPath（Tn 坐标）。
        val firstCursor = chain.mapNotNull { it.cursor }.firstOrNull()
        val lastCursor = chain.mapNotNull { it.cursor }.lastOrNull()
        val lastIntent = chain.last()

        // #684 评论 5666730754：screenSuppressed — 整条 chain 里只要有一笔 SYSTEM_SUPPRESSED
        // 就算。overlay 实际只需要区分 suppressed / 非 suppressed，用 screenSuppressed 收口
        // animationMode 和 textAnimationActive/cursorAnimationActive。
        val screenSuppressed =
            chain.any { it.animationMode == AnimationModeDto.SYSTEM_SUPPRESSED }

        // 计算 retained moves — 用 offset map chain 合成。
        val retainedMoves =
            ComposeVisualRebase.computeRetainedMoves(
                oldLayout = consumed.layout,
                newLayout = newest.layout,
                chain = chain,
            )

        // 计算 cursor motion path。
        // #684 评论 5672654866：coordinator 不再负责重建"此刻屏幕光标在哪"。
        // 当前 cursor rect 始终留在 overlay 的长生命周期 Animatable 里。
        // coordinator 只根据本事务 old/new layout、intent chain、animation units
        // 生成冻结的 cursorMotionPath。下一笔到来时不再把上一笔 progress 换算成 rect。
        // #684 评论 5664636035 Bug2：无旧动画时光标起点应从 chain 第一笔 old cursor 起跑（T0 坐标）。

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
        // #684 评论 5666730754：用 screenSuppressed（整条 chain 任一笔 SYSTEM_SUPPRESSED）收口，
        // 不再只看最后一笔 intent 的 animationMode。
        // #684 评论 5667483662 问题1：用 pendingChain.motionPolicy（与 intent 同源），
        // 不再用 lastMotionPolicy（最近一次 onLayout 顺手记住的 policy）。
        val chainMotionPolicy = pendingChain.motionPolicy
        val customAnimationEnabled = chainMotionPolicy.textEnabled && !screenSuppressed
        val customTextAnimationEnabled =
            customAnimationEnabled && transactionTextKind != TextVisualKind.None

        // #684 评论 5666730754：冻结正文/光标视觉所有权 — 屏幕事务创建时一次算死，
        // 表示这笔事务的正文/光标是否真的被 overlay 接管动画过（不是"事务还挂着"）。
        // materializeStartFrame / interruptedCursorRect / overlay 绘制 / 无 overlay 事务 settle
        // 四处统一读这两个冻结字段。
        val textAnimationActive = customTextAnimationEnabled
        val cursorAnimationActive =
            !screenSuppressed &&
                chainMotionPolicy.cursorEnabled &&
                firstCursor != null &&
                lastCursor != null &&
                chain.any { it.cursor?.animate == true } &&
                firstCursor.oldEndUtf16 != lastCursor.newEndUtf16

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
                            rebaseProgress = masterProgress,
                            nextOffsetMap = composedOffsetMap,
                            nextReplaceBounds = lastIntent.replaceBounds,
                            currentSuppressedRanges = current.suppressedCurrentRanges,
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
        // #684 评论 5666730754：无 overlay 工作的事务不按 durationMs 假装 active。
        // 如果一笔事务 textAnimationActive==false && cursorAnimationActive==false && startFrame==null，
        // 它本身没有任何 overlay 工作，不要让它按 durationMs 假装 active 100ms。
        // 把 durationMs 设为 0，overlay 的 LaunchedEffect 会走 snapTo(1f) 分支立即完成并
        // completeTransaction，active 被清掉，lastConsumed 也会在 completeTransaction 里推进。
        // overlay 的 hasAnimation 此时为 false（textAnimationActive/cursorAnimationActive 都 false
        // 且 startFrame==null），不会画出任何东西。
        val effectiveDurationMs =
            if (!textAnimationActive && !cursorAnimationActive && startFrame == null) {
                0L
            } else {
                lastIntent.durationMs
            }
        // #684 评论 5669048233 Bug2 修复：多 Core intent 合成一帧时，animation units
        // 映射到最终 T0→Tn 坐标。不再退化成整块 mergedRanges — 用 compose 函数把每笔
        // intent 的 units 沿 offsetMap chain 映射到统一坐标系，保留 Core 原来的 unit 边界。
        val composedOldAnimationUnits = ComposeVisualRebase.composeOldAnimationUnitsToBase(chain)
        val newAnimationUnits = ComposeVisualRebase.composeNewAnimationUnitsToFinal(chain)

        // #684 评论 5670941608：oldAnimationUnits 也要与 effectiveOldRanges 使用同一个
        // startFrame.ownedOldRanges ownership subtraction。否则"上一笔动画尚未结束，下一笔
        // 马上删除/替换刚才那段文字"时，同一段旧文字同时由 startFrame fading slice 和
        // unit-delete 路径绘制，出现重影。blocker 只覆盖 unit 一部分时保留 subtraction 后
        // 剩下的片段，保持 Core 的动画粒度，不把相邻 unit 再 merge。
        val startFrameOwnedOldRanges = startFrame?.ownedOldRanges.orEmpty()
        val effectiveOldAnimationUnits =
            composedOldAnimationUnits.flatMap { unit ->
                ComposeVisualRebase.subtractRanges(
                    candidates = listOf(unit),
                    blockers = startFrameOwnedOldRanges,
                )
            }
        // #684 评论 5672654866：构建光标运动路径 —
        // 把"光标路径"从单纯 start/end 两点升级成和文字 unit 对应的路径。
        // 一次提交多个插入 unit 时按 newAnimationUnits 顺序取每个 unit 出现后的 caret rect，
        // endFraction 与 unitLocalProgress 分段时序一致。
        // cursorAnimationActive==false 时不构建路径（overlay 不会动画光标）。
        val cursorMotionPath =
            if (cursorAnimationActive) {
                buildCursorMotionPath(
                    oldLayout = consumed.layout,
                    newLayout = newest.layout,
                    intents = chain,
                    oldAnimationUnits = effectiveOldAnimationUnits,
                    newAnimationUnits = newAnimationUnits,
                )
            } else {
                null
            }
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
                cursorMotionPath = cursorMotionPath,
                startFrame = startFrame,
                durationMs = effectiveDurationMs,
                motionPolicy = chainMotionPolicy,
                // #684 评论 5663862982：事务生成后冻结的 suppressed ranges —
                // 下一笔 rebase 时按 composedOffsetMap 映射到新坐标系。
                suppressedCurrentRanges = hiddenRanges,
                // #684 评论 5664636035 Bug1：屏幕事务的 textKind 按最终净变化决定。
                textKind = transactionTextKind,
                // #684 评论 5665907509 问题1 + 评论 5666730754：用 screenSuppressed 收口 —
                // 整条 chain 里只要有一笔 SYSTEM_SUPPRESSED，当前屏幕事务的 animationMode 就是
                // SYSTEM_SUPPRESSED。overlay 据此判断 systemSuppressed，不再从 _activeIntent 读取。
                animationMode =
                    if (screenSuppressed) {
                        AnimationModeDto.SYSTEM_SUPPRESSED
                    } else {
                        lastIntent.animationMode
                    },
                // #684 评论 5666730754：冻结正文/光标视觉所有权。
                textAnimationActive = textAnimationActive,
                cursorAnimationActive = cursorAnimationActive,
                // #684 评论 5668108597 问题2：冻结 animation units 供 overlay 按单元做吐字/吞字。
                // #684 评论 5670941608：oldAnimationUnits 已扣除 startFrame.ownedOldRanges，
                // 避免同一段旧文字被 startFrame 和 unit-delete 双重绘制。
                oldAnimationUnits = effectiveOldAnimationUnits,
                newAnimationUnits = newAnimationUnits,
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
 *
 * #684 评论 5667483662 问题1：[motionPolicy] 与 intent 同源 —
 * tryStartTransaction 永远使用这条 pending chain 自己携带的 policy，
 * 不再依赖"最近一次 onLayout 顺手记住的 policy"。
 * 传入前已 effective()，这里存的是 effective 后的策略。
 */
private data class PendingVisualChain(
    val baseText: String,
    val targetText: String,
    val intents: List<EditorVisualIntent>,
    val motionPolicy: EditorMotionPolicy,
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
