package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import com.xiwei.sujian.core.interop.diagnostics.EditorDiagnosticsEvents
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.cursorRect

/**
 * #644 评论 #684：帧协调器 — 解决"Core 一笔事务不等于屏幕一帧"的问题。
 *
 * #689 评论 5674631257 步骤3：删除动画运行职责。
 * 现在只回答"旧屏幕帧到新屏幕帧改了什么"，不回答"上一笔动画现在跑到哪了"。
 *
 * #694 评论第 6 步：职责收窄成 Core/external visual coordinator。
 * Issue #735 评论 5773604666 问题1：删除双视觉入口后，所有正文编辑统一走 [onEditFact]。
 * 不再有本地输入旁路 — [ComposeEditorVisualState.onEditFact] 是唯一视觉 patch 来源。
 *
 * Issue #735 评论 5771063665：删除 Core VisualIntent 专用分支。
 * 本协调器只负责把 [EditorEditFact] + old/new [ComposeLayoutSnapshot]
 * 变成 Android 平台自己的 visual patch。
 *
 * Issue #737：本协调器只整理 motion 输入 — 生成 [ComposeVisualPatch] 后交给
 * [ComposeEditorVisualState.drainPendingPatchesAtFrame] 直接构造 [CoordinatedEditMotion]。
 * 不再"把文字 unit 提前交给 timeline"（timeline 已删除），
 * 也不直接决定文字动画启动（由 VisualState 在 drain 时根据 motionPolicy 决定）。
 *
 * `onEditFact()` 和 `onLayout()` 最终只返回 [ComposeVisualPatch]。
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

    /** 中间积累的编辑事实 chain — 等匹配的真实 layout 到达后一起合成 patch。 */
    private var pending: PendingEditChain? = null

    /** 单调递增的 patch ID。 */
    private var nextPatchId: Long = 0L

    /**
     * 编辑事实到达 — 只串进 pending chain（连续才拼接，不连续不开硬拼），
     * 然后尝试合流生成 patch。
     *
     * Issue #735 评论 5771063665：参数从 [EditorVisualIntent] 改为 [EditorEditFact]。
     */
    fun onEditFact(fact: EditorEditFact): FrameUpdate {
        val existing = pending
        if (existing == null) {
            pending =
                PendingEditChain(
                    baseText = fact.expectedOldText,
                    targetText = fact.expectedNewText,
                    facts = listOf(fact),
                )
        } else {
            val lastExpectedNew = existing.facts.last().expectedNewText
            if (lastExpectedNew == fact.expectedOldText) {
                pending =
                    existing.copy(
                        facts = existing.facts + fact,
                        targetText = fact.expectedNewText,
                    )
            } else {
                pending =
                    PendingEditChain(
                        baseText = fact.expectedOldText,
                        targetText = fact.expectedNewText,
                        facts = listOf(fact),
                    )
            }
        }

        EditorDiagnosticsEvents.editorEditFactQueued(
            targetId = targetId,
            coreTransactionId = fact.coreTransactionId,
            baseRevision = fact.baseRevision,
            newRevision = fact.newRevision,
            pendingChainSize = pending?.facts?.size ?: 0,
        )

        return tryBuildPatch()
    }

    /**
     * 候选几何到达 — 平台已经算出某个真实 layout（例如 IME preedit 期间的最终文字 B），
     * 但这份 layout 不允许推进 committed baseline。
     *
     * 只缓存 [latest]，不初始化/推进 [lastConsumed]，不生成 patch，直接返回 [FrameUpdate.Empty]。
     *
     * Issue #735 评论 5775326365：IME preedit 已经是最终文字 B 时，composition 活跃分支把 preedit
     * layout 交给本方法缓存。commit 时无论后面还有没有新的 onTextLayout(B)：
     * - fact 先到：[onEditFact]`(A->B)` 可以直接拿 coordinator 已缓存的 latest=B，生成 patch；
     * - final layout 先到：普通 [onLayout]`(B)` 不会把 A baseline 提前推进，fact 后到照样配；
     * - 根本没有 final layout 回调：preedit 阶段缓存的 B layout 已经够用了。
     *
     * 与已删除的 `observePresentedLayout` 的关键区别：本方法**绝不**碰 [lastConsumed]，
     * 不会偷偷推进 committed baseline。
     */
    fun onProvisionalLayout(snapshot: ComposeLayoutSnapshot): FrameUpdate {
        latest = PresentedLayout(snapshot.result.layoutInput.text.text, snapshot)

        EditorDiagnosticsEvents.editorLayoutPresented(
            targetId = targetId,
            layoutTextLength = snapshot.result.layoutInput.text.text.length,
        )

        // 只缓存 latest，不初始化/推进 lastConsumed，不生成 patch。
        return FrameUpdate.Empty
    }

    /**
     * 真实屏幕布局到达 — 更新最新 layout，然后尝试合流生成 patch。
     *
     * 本方法处理可推进 committed baseline 的普通 layout（composition 结束后的最终 layout、
     * Core visual path 的 Undo/Redo/Programmatic/Load/Format 等）。
     * IME composition 活跃期间的 preedit layout 应走 [onProvisionalLayout]。
     */
    fun onLayout(snapshot: ComposeLayoutSnapshot): FrameUpdate {
        latest = PresentedLayout(snapshot.result.layoutInput.text.text, snapshot)

        EditorDiagnosticsEvents.editorLayoutPresented(
            targetId = targetId,
            layoutTextLength = snapshot.result.layoutInput.text.text.length,
        )

        if (lastConsumed == null) {
            lastConsumed = latest
        }

        if (pending == null && lastConsumed?.text == latest?.text) {
            lastConsumed = latest
        }

        return tryBuildPatch()
    }

    /**
     * 双向合流：当 pending chain 与两份 layout 概念同时满足匹配条件时生成 patch。
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

        val chain = pendingChain.facts
        val coreTransactionIds = chain.map { it.coreTransactionId }
        val composedOffsetMap = ComposeVisualRebase.composeOffsetMapChain(chain)
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

        val lastFact = chain.last()

        val screenSuppressed =
            chain.any { it.animationMode == AnimationMode.SYSTEM_SUPPRESSED }

        val retainedMoves =
            ComposeVisualRebase.computeRetainedMoves(
                oldLayout = consumed.layout,
                newLayout = newest.layout,
                chain = chain,
            )

        val transactionTextKind =
            when {
                mergedOldRanges.isEmpty() && mergedNewRanges.isEmpty() -> TextVisualKind.None
                mergedOldRanges.isEmpty() -> TextVisualKind.Insert
                mergedNewRanges.isEmpty() -> TextVisualKind.Delete
                else -> TextVisualKind.Move
            }

        val composedOldAnimationUnits = ComposeVisualRebase.composeOldAnimationUnitsToBase(chain)
        val newAnimationUnits = ComposeVisualRebase.composeNewAnimationUnitsToFinal(chain)

        val insertedUnits =
            when (transactionTextKind) {
                TextVisualKind.Insert,
                TextVisualKind.Move,
                -> if (newAnimationUnits.isNotEmpty()) newAnimationUnits else mergedNewRanges
                TextVisualKind.Delete,
                TextVisualKind.None,
                -> emptyList()
            }

        val deletedUnits =
            when (transactionTextKind) {
                TextVisualKind.Delete,
                TextVisualKind.Move,
                -> if (composedOldAnimationUnits.isNotEmpty()) composedOldAnimationUnits else mergedOldRanges
                TextVisualKind.Insert,
                TextVisualKind.None,
                -> emptyList()
            }

        val oldSelectionEnd =
            chain.first().oldSelectionEndUtf16.let { if (it >= 0) it else consumed.layout.selection.end }
        val newSelectionEnd = chain.last().newSelectionEndUtf16.let { if (it >= 0) it else newest.layout.selection.end }
        val originCaretRect = consumed.layout.cursorRect(oldSelectionEnd)
        val targetCaretRect = newest.layout.cursorRect(newSelectionEnd)

        val effectiveDurationMs = lastFact.durationMs

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
                        AnimationMode.SYSTEM_SUPPRESSED
                    } else {
                        lastFact.animationMode
                    },
            )

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
 * Issue #735 评论 5771063665：中间积累的编辑事实 chain — 等 onLayout 到达后一起合成 patch。
 */
private data class PendingEditChain(
    val baseText: String,
    val targetText: String,
    val facts: List<EditorEditFact>,
)

/**
 * 帧更新结果 — onLayout / onEditFact 返回。
 */
sealed interface FrameUpdate {
    /** 无新 patch（无 pending / 无匹配 layout）。 */
    data object Empty : FrameUpdate

    /** 新 patch 生成 — overlay 读取 patch 并推进 timeline。 */
    data class NewPatch(
        val patch: ComposeVisualPatch,
    ) : FrameUpdate
}
