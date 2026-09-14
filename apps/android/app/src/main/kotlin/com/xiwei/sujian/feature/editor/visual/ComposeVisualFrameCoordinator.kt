package com.xiwei.sujian.feature.editor.visual

import android.util.Log
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy

/**
 * #644 评论 #684：帧协调器 — 解决"Core 一笔事务不等于屏幕一帧"的问题。
 *
 * Core 在一次按键/删除中可能产生多笔事务（2~5 笔），
 * 但 Compose 一帧的 `onTextLayout` 回调只给出一个最终 [ComposeLayoutSnapshot]。
 * 旧实现试图为每笔 Core 事务单独创建动画事务，导致中间的 `TextLayoutResult`
 * 根本不存在、光标抖动、换行重排错位。
 *
 * 本协调器的策略：
 * - `onVisualIntent()` 只把 Core intent 串进 pending chain，不创建事务。
 * - `onLayout()` 在真正收到 Compose 屏幕布局时，
 *   把上一份已呈现的 layout → 当前 layout + 中间积累的 intent chain
 *   合成一个冻结的 [ComposeVisualTransaction]。
 *
 * 这样快速输入、快速删除、Enter、软换行都只对真实屏幕帧做动画。
 */
class ComposeVisualFrameCoordinator {
    companion object {
        private const val TAG = "VisualFrameCoord"
    }

    /** 已呈现到屏幕的布局快照 — 上一次真正由 overlay 消费的 layout。 */
    private var presented: PresentedLayout? = null

    /** 中间积累的 Core intent chain — 等下一个 onLayout 到达后一起合成事务。 */
    private var pending: PendingVisualChain? = null

    /** 当前正在跑的视觉事务 — overlay 读取。 */
    private var active: ComposeVisualTransaction? = null

    /** 单调递增的事务 ID。 */
    private var nextTransactionId: Long = 0L

    /**
     * Core intent 到达 — 只串进 pending chain，不启动动画、不创建事务。
     */
    fun onVisualIntent(intent: EditorVisualIntent) {
        val existing = pending
        if (existing == null) {
            pending =
                PendingVisualChain(
                    baseText = intent.offsetMap?.entries?.firstOrNull()?.let {
                        // 将来可从 intent 推导 baseText，当前先用空串。
                        ""
                    } ?: "",
                    targetText = "",
                    intents = listOf(intent),
                )
        } else {
            // 验证连续性：上一笔 expectedNewText == 下一笔 expectedOldText 不再严格校验，
            // 因为 Core 可能合并或拆分事务。直接追加即可。
            pending = existing.copy(intents = existing.intents + intent)
        }

        Log.d(TAG, "intent_queued: coreTxn=${intent.coreTransactionId} pending=${pending?.intents?.size}")
    }

    /**
     * 真实屏幕布局到达 — 生成冻结事务。
     *
     * 从上一份已呈现 layout → 当前 layout + 中间积累的 intent chain
     * → 一个 [ComposeVisualTransaction]，创建后不再修改。
     */
    fun onLayout(
        snapshot: ComposeLayoutSnapshot,
        motionPolicy: EditorMotionPolicy,
    ): FrameUpdate {
        val previousPresented = presented
        val chain = pending
        pending = null

        if (previousPresented == null || chain == null || chain.intents.isEmpty()) {
            // 首帧或无 pending intent — 只更新 presented，不创建事务。
            presented = PresentedLayout(snapshot.result.layoutInput.text.text, snapshot)
            return FrameUpdate.Empty
        }

        // 合并 chain 中所有 intent 的 ranges 和 Core 事务 ID。
        val allIntents = chain.intents
        val coreTransactionIds = allIntents.map { it.coreTransactionId }
        val mergedOldRanges = allIntents.flatMap { it.oldRanges }
        val mergedNewRanges = allIntents.flatMap { it.newRanges }

        // 取最后一个 intent 的 cursor 信息。
        val lastIntent = allIntents.last()
        val cursorInfo = lastIntent.cursor

        // 计算 retained moves — 用 offset map chain 合成。
        val retainedMoves =
            ComposeVisualRebase.computeRetainedMoves(
                oldLayout = previousPresented.layout,
                newLayout = snapshot,
                chain = allIntents,
            )

        // 计算 cursor start/end rect。
        val cursorStartRect =
            if (cursorInfo != null && previousPresented.layout != null) {
                try {
                    val startOffset =
                        cursorInfo.oldEndUtf16
                            .coerceIn(0, previousPresented.layout.result.layoutInput.text.length)
                    previousPresented.layout.result.getCursorRect(startOffset)
                } catch (_: Throwable) {
                    null
                }
            } else {
                null
            }

        val cursorEndRect =
            if (cursorInfo != null) {
                try {
                    val endOffset =
                        cursorInfo.newEndUtf16
                            .coerceIn(0, snapshot.result.layoutInput.text.length)
                    snapshot.result.getCursorRect(endOffset)
                } catch (_: Throwable) {
                    null
                }
            } else {
                null
            }

        // 计算 hidden ranges — 由 overlay 接管的范围。
        val hasTextAnimation =
            motionPolicy.textEnabled && lastIntent.textKind != TextVisualKind.None
        val hiddenRanges =
            if (hasTextAnimation) {
                mergedNewRanges.filter { it.start < it.end }
            } else {
                emptyList()
            }

        // 计算 startFrame — 物化当前帧作为新事务的起点。
        val startFrame =
            active?.let { current ->
                ComposeVisualRebase.materializeStartFrame(
                    ComposeVisualRebase.MaterializeStartFrameParams(
                        transaction = current,
                        textProgress = 1f,
                        cursorProgress = 1f,
                        rebaseProgress = 1f,
                        nextReplaceBounds = lastIntent.replaceBounds,
                        hiddenRanges = hiddenRanges,
                        cursorSnapshot = null,
                    ),
                )
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
                oldLayout = previousPresented.layout,
                newLayout = snapshot,
                intents = allIntents,
                oldRanges = effectiveOldRanges,
                newRanges = mergedNewRanges,
                retainedMoves = retainedMoves,
                cursorStartRect = cursorStartRect,
                cursorEndRect = cursorEndRect,
                startFrame = startFrame,
                durationMs = lastIntent.durationMs,
                motionPolicy = motionPolicy,
            )

        active = transaction
        presented = PresentedLayout(snapshot.result.layoutInput.text.text, snapshot)

        Log.d(
            TAG,
            "transaction_started: id=${transaction.id} coreTxnIds=$coreTransactionIds " +
                "retained=${retainedMoves.size} oldTextLen=${previousPresented.layout?.result?.layoutInput?.text?.length ?: 0} " +
                "newTextLen=${snapshot.result.layoutInput.text.length}",
        )

        return FrameUpdate.NewTransaction(transaction, hiddenRanges)
    }

    /**
     * 当前活跃事务 — overlay 读取。
     */
    fun currentTransaction(): ComposeVisualTransaction? = active

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        presented = null
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
    /** 无新事务（首帧或无 pending intent）。 */
    data object Empty : FrameUpdate

    /** 新事务生成 — overlay 读取 transaction 并更新 hiddenRanges。 */
    data class NewTransaction(
        val transaction: ComposeVisualTransaction,
        val hiddenRanges: List<androidx.compose.ui.text.TextRange>,
    ) : FrameUpdate
}
