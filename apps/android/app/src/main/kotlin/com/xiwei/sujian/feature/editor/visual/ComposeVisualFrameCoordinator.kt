package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import com.xiwei.sujian.core.interop.diagnostics.EditorDiagnosticsEvents
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.cursorRect
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import uniffi.writer_core.AnimationModeDto

/**
 * #644 评论 #684：帧协调器 — 解决"Core 一笔事务不等于屏幕一帧"的问题。
 *
 * #689 评论 5674631257 步骤3：删除动画运行职责。
 * 现在只回答"旧屏幕帧到新屏幕帧改了什么"，不回答"上一笔动画现在跑到哪了"。
 *
 * #694 评论第 6 步：职责收窄成 Core/external visual coordinator。
 * 本地输入（TYPING/TYPING_COMMIT/IME_COMPOSITION/PASTE/DELETE）不再从这里进入 —
 * 它由 [WritingEditorSurface] 的 InputTransformation → [ComposeEditorVisualState.recordLocalInput]
 * 直接记录，等 [TextLayoutResult] 到达时配对生成 [ComposeVisualPatch]（intent=null）。
 * 现有 [PendingVisualChain]、[lastConsumed]/[latest]、Core revision、transactionId 保留给
 * Undo/Redo/Programmatic/Load/Format 这些真正需要 Core 驱动的修改；
 * 不要再拿它给普通打字和 Backspace 配 TextLayoutResult。
 *
 * 保留：
 * - [PendingVisualChain]、[lastConsumed]/[latest]、Core intent 与真实 [TextLayoutResult]
 *   的双向汇合、offset map chain、[computeRetainedMoves]。
 *
 * 删除：
 * - active: ComposeVisualTransaction?
 * - masterProgress 参数
 * - materializeStartFrame(...)
 * - mappedPrevSuppressedRanges
 * - hiddenRanges 继承
 * - completeTransaction()
 *
 * `onVisualIntent()` 和 `onLayout()` 最终只返回 [ComposeVisualPatch]。
 */
class ComposeVisualFrameCoordinator(
    private val targetId: String,
) {
    companion object {
        private const val TAG = "VisualFrameCoord"
    }

    /** 上一次真正生成 patch 消费的旧侧 layout（基线）；首次 layout 设为基线，之后只在生成时推进。 */
    private var lastConsumed: PresentedLayout? = null

    /** 最新真实 layout — 每一次 onLayout 都更新。 */
    private var latest: PresentedLayout? = null

    /** 中间积累的 Core intent chain — 等匹配的真实 layout 到达后一起合成 patch。 */
    private var pending: PendingVisualChain? = null

    /** 单调递增的 patch ID。 */
    private var nextPatchId: Long = 0L

    /**
     * Core intent 到达 — 只串进 pending chain（连续才拼接，不连续不开硬拼），
     * 然后尝试合流生成 patch。
     *
     * @param motionPolicy 本笔 intent 的 effective 动画策略 — 与 pending chain 同源。
     */
    fun onVisualIntent(
        intent: EditorVisualIntent,
        motionPolicy: EditorMotionPolicy,
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
            val lastExpectedNew = existing.intents.last().expectedNewText
            if (lastExpectedNew == intent.expectedOldText) {
                pending =
                    existing.copy(
                        intents = existing.intents + intent,
                        targetText = intent.expectedNewText,
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

        return tryBuildPatch()
    }

    /**
     * 真实屏幕布局到达 — 更新最新 layout，然后尝试合流生成 patch。
     */
    fun onLayout(snapshot: ComposeLayoutSnapshot): FrameUpdate {
        // Issue #728 评论 5754045689：result 就是 raw 正文布局，直接读 text。
        latest = PresentedLayout(snapshot.result.layoutInput.text.text, snapshot)

        EditorDiagnosticsEvents.editorLayoutPresented(
            targetId = targetId,
            layoutTextLength = snapshot.result.layoutInput.text.text.length,
        )

        if (lastConsumed == null) {
            lastConsumed = latest
        }

        // #684 评论 5665907509 问题2：没有 pending 文本事务的真实重新排版也要推进 lastConsumed。
        if (pending == null && lastConsumed?.text == latest?.text) {
            lastConsumed = latest
        }

        return tryBuildPatch()
    }

    /**
     * #694 评论 5691696678 问题2：屏幕基线推进入口 —
     * 本地输入命中或 IME composition 活跃时，真实 layout 已经呈现但不应走 Core visual path
     * 生成 patch（本地输入有自己的 patch，composition 只推进基线不播放吞吐）。
     *
     * 但 [ComposeVisualFrameCoordinator] 的 [lastConsumed] 基线必须跟随真实 layout 推进，
     * 否则后续 Undo/Redo/Programmatic intent 的 `pending.baseText` 与 `lastConsumed.text`
     * 对不上，[tryBuildPatch] 一直返回 [FrameUpdate.Empty]。
     *
     * 职责：只更新 [latest]/[lastConsumed]，**不**调用 [tryBuildPatch]，**不**生成 Core patch。
     * - 首次（lastConsumed == null）：设为基线。
     * - 没有 Core pending（pending == null）：屏幕基线直接跟随真实 layout。
     * - 有 Core pending：不推进 lastConsumed（等 tryBuildPatch 匹配后再推进），只更新 latest。
     *
     * 诊断事件与 [onLayout] 一致 — overlay/诊断仍能观察到 layout 已呈现。
     */
    fun observePresentedLayout(snapshot: ComposeLayoutSnapshot) {
        // Issue #728 评论 5754045689：result 就是 raw 正文布局，直接读 text。
        val presented = PresentedLayout(snapshot.result.layoutInput.text.text, snapshot)
        latest = presented

        EditorDiagnosticsEvents.editorLayoutPresented(
            targetId = targetId,
            layoutTextLength = snapshot.result.layoutInput.text.text.length,
        )

        // #694 评论 5692161955 问题3：补并发顺序条件。
        // 原实现只要 pending != null 就不推进 lastConsumed，导致
        // "本地输入完成 -> external intent 先到 -> 本地 layout 后到"顺序下卡住。
        // pending.baseText == presented.text 时推进 lastConsumed 是安全的——
        // presented 就是 pending 期望的 baseText，推进后 tryBuildPatch 的 baseText 匹配条件仍成立，
        // 后续 target layout 到达时 patch 能生成。
        when {
            lastConsumed == null -> lastConsumed = presented
            pending == null -> lastConsumed = presented
            pending?.baseText == presented.text -> lastConsumed = presented
            // 其他情况（pending != null 且 pending.baseText != presented.text）：
            // 不推进 lastConsumed（等 tryBuildPatch 匹配后再推进），只更新 latest
        }
    }

    /**
     * 双向合流：当 pending chain 与两份 layout 概念同时满足匹配条件时生成 patch。
     *
     * 匹配条件：
     * - pending != null
     * - lastConsumed != null && latest != null
     * - latest 不是 lastConsumed 本身（确有新 layout）
     * - pending.baseText == lastConsumed.text
     * - pending.targetText == latest.text
     */
    private fun tryBuildPatch(): FrameUpdate {
        val pendingChain = pending
        if (pendingChain == null) {
            return FrameUpdate.Empty
        }
        val consumed = lastConsumed
        val newest = latest
        if (consumed == null || newest == null) return FrameUpdate.Empty
        if (newest === consumed) return FrameUpdate.Empty
        if (pendingChain.baseText != consumed.text) return FrameUpdate.Empty
        if (pendingChain.targetText != newest.text) return FrameUpdate.Empty

        // 合流生成 patch。
        val chain = pendingChain.intents
        val coreTransactionIds = chain.map { it.coreTransactionId }
        val composedOffsetMap = ComposeVisualRebase.composeOffsetMapChain(chain)
        // Issue #728 评论 5754045689：result 就是 raw 正文布局，长度直接读 text。
        val oldLength = consumed.layout.result.layoutInput.text.text.length
        val newLength = newest.layout.result.layoutInput.text.text.length
        val mergedOldRanges: List<androidx.compose.ui.text.TextRange>
        val mergedNewRanges: List<androidx.compose.ui.text.TextRange>
        if (composedOffsetMap != null) {
            val frameChangedRanges =
                ComposeVisualRebase.changedRangesFromComposedMap(
                    composedOffsetMap,
                    oldLength,
                    newLength,
                )
            mergedOldRanges = frameChangedRanges.oldRanges
            mergedNewRanges = frameChangedRanges.newRanges
        } else {
            mergedOldRanges = chain.flatMap { it.oldRanges }
            mergedNewRanges = chain.flatMap { it.newRanges }
        }

        val lastIntent = chain.last()

        val screenSuppressed =
            chain.any { it.animationMode == AnimationModeDto.SYSTEM_SUPPRESSED }

        // 计算 retained moves — 用 offset map chain 合成。
        val retainedMoves =
            ComposeVisualRebase.computeRetainedMoves(
                oldLayout = consumed.layout,
                newLayout = newest.layout,
                chain = chain,
            )

        // 屏幕 patch 的 textKind 按最终净变化决定。
        val transactionTextKind =
            when {
                mergedOldRanges.isEmpty() && mergedNewRanges.isEmpty() -> TextVisualKind.None
                mergedOldRanges.isEmpty() -> TextVisualKind.Insert
                mergedNewRanges.isEmpty() -> TextVisualKind.Delete
                else -> TextVisualKind.Move
            }

        val chainMotionPolicy = pendingChain.motionPolicy
        val customAnimationEnabled = chainMotionPolicy.textEnabled && !screenSuppressed
        val customTextAnimationEnabled =
            customAnimationEnabled && transactionTextKind != TextVisualKind.None

        val textAnimationActive = customTextAnimationEnabled

        // insertedUnits / deletedUnits — Core 给出的 animation units 在 coordinator 构建 patch 时
        // 直接变成 insertedUnits / deletedUnits；真正运行到哪由 timeline 的每个 unit 自己保存时间。
        val composedOldAnimationUnits = ComposeVisualRebase.composeOldAnimationUnitsToBase(chain)
        val newAnimationUnits = ComposeVisualRebase.composeNewAnimationUnitsToFinal(chain)

        val insertedUnits =
            if (textAnimationActive) {
                when (transactionTextKind) {
                    TextVisualKind.Insert,
                    TextVisualKind.Move,
                    -> if (newAnimationUnits.isNotEmpty()) newAnimationUnits else mergedNewRanges
                    TextVisualKind.Delete,
                    TextVisualKind.None,
                    -> emptyList()
                }
            } else {
                emptyList()
            }

        val deletedUnits =
            if (textAnimationActive) {
                when (transactionTextKind) {
                    TextVisualKind.Delete,
                    TextVisualKind.Move,
                    -> if (composedOldAnimationUnits.isNotEmpty()) composedOldAnimationUnits else mergedOldRanges
                    TextVisualKind.Insert,
                    TextVisualKind.None,
                    -> emptyList()
                }
            } else {
                emptyList()
            }

        // Issue #728 评论 5754045689：一次性交出 caret rect + 文字 units —
        // 从 old/new raw layout + old/new selection 直接计算两端 caret rect。
        // old/new selection 从 intent.cursor 或 snapshot.selection 取。
        val oldSelectionEnd = chain.first().cursor?.oldEndUtf16 ?: consumed.layout.selection.end
        val newSelectionEnd = chain.last().cursor?.newEndUtf16 ?: newest.layout.selection.end
        val originCaretRect = consumed.layout.cursorRect(oldSelectionEnd)
        val targetCaretRect = newest.layout.cursorRect(newSelectionEnd)

        // #684 评论 5666730754：无 overlay 工作的事务不按 durationMs 假装 active。
        val effectiveDurationMs =
            if (!textAnimationActive) {
                0L
            } else {
                lastIntent.durationMs
            }

        nextPatchId++
        val patch =
            ComposeVisualPatch(
                id = nextPatchId,
                coreTransactionIds = coreTransactionIds,
                oldLayout = consumed.layout,
                newLayout = newest.layout,
                offsetMap = composedOffsetMap,
                insertedUnits = insertedUnits,
                deletedUnits = deletedUnits,
                retainedMoves = retainedMoves,
                originCaretRect = originCaretRect,
                targetCaretRect = targetCaretRect,
                durationMs = effectiveDurationMs,
                animationMode =
                    if (screenSuppressed) {
                        AnimationModeDto.SYSTEM_SUPPRESSED
                    } else {
                        lastIntent.animationMode
                    },
                motionPolicy = chainMotionPolicy,
                intent = lastIntent,
            )

        // patch 生成后照常推进基线、清 pending，但不要保存一个 active transaction 等待结束。
        lastConsumed = newest
        pending = null

        EditorDiagnosticsEvents.editorVisualTransactionStarted(
            targetId = targetId,
            visualTransactionId = patch.id,
            coreTransactionIds = coreTransactionIds,
            baseRevision = chain.first().baseRevision,
            newRevision = chain.last().newRevision,
            pendingChainSize = chain.size,
            layoutTextLength = newest.layout.result.layoutInput.text.text.length,
        )

        Log.d(
            TAG,
            "patch_built: id=${patch.id} coreTxnIds=$coreTransactionIds " +
                "retained=${retainedMoves.size} " +
                "oldTextLen=${consumed.layout.result.layoutInput.text.text.length} " +
                "newTextLen=${newest.layout.result.layoutInput.text.text.length}",
        )

        return FrameUpdate.NewPatch(patch)
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        lastConsumed = null
        latest = null
        pending = null
        nextPatchId = 0L
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
 * 中间积累的 Core intent chain — 等 onLayout 到达后一起合成 patch。
 */
private data class PendingVisualChain(
    val baseText: String,
    val targetText: String,
    val intents: List<EditorVisualIntent>,
    val motionPolicy: EditorMotionPolicy,
)

/**
 * 帧更新结果 — onLayout / onVisualIntent 返回。
 *
 * #689：从返回 [ComposeVisualTransaction] 改为返回 [ComposeVisualPatch]。
 */
sealed interface FrameUpdate {
    /** 无新 patch（无 pending / 无匹配 layout）。 */
    data object Empty : FrameUpdate

    /** 新 patch 生成 — overlay 读取 patch 并推进 timeline。 */
    data class NewPatch(
        val patch: ComposeVisualPatch,
    ) : FrameUpdate
}
