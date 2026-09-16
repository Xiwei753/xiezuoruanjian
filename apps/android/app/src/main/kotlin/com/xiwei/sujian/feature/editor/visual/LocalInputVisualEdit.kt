package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import kotlin.collections.ArrayDeque

/**
 * #694 评论第 1 步：Android 本地输入的一次 change —
 * 不带 Core revision/transactionId，只描述 IME/键盘本次输入的 new/old UTF-16 range。
 *
 * [newRange] 当前正文中的新 range；[oldRange] 旧正文中对应的 range。
 * 由 `InputTransformation.changes.forEachChange { range, originalRange -> }` 直接提供，
 * 不再对前后整章做 commonPrefix/commonSuffix 猜动画 change。
 */
data class LocalInputChange(
    val newRange: TextRange,
    val oldRange: TextRange,
)

/**
 * #694 评论第 1 步：Android 本地输入的视觉事实 —
 * 记录本次用户输入，等下一份真实 [androidx.compose.ui.text.TextLayoutResult] 到来时配对。
 *
 * 不带 Core revision/transactionId — 本地输入不等 Core 回声。
 * [sequence] 单调递增序号，用于配对时取最新一笔。
 * [oldText]/[newText] 本次输入前后的完整正文（UTF-16）。
 * [oldSelection]/[newSelection] 输入前后选区。
 * [changes] 本次输入的 change 列表（来自 TextFieldBuffer.forEachChange）。
 */
data class LocalInputVisualEdit(
    val sequence: Long,
    val oldText: String,
    val newText: String,
    val oldSelection: TextRange,
    val newSelection: TextRange,
    val changes: List<LocalInputChange>,
)

/**
 * #694 评论第 1 步：本地输入视觉事实 tracker —
 * 普通 [ArrayDeque]，**不是 Compose State**，不在 InputTransformation 里改 StateFlow/mutableStateOf。
 *
 * 职责只有：
 * 1. [record] 本次用户输入（由 InputTransformation 调用）；
 * 2. [drainMatching] 等下一份真实 TextLayoutResult 到来时配对消费（由 onAuthoritativeLayout 调用）；
 * 3. [clear] 章节切换/detach 时清空。
 *
 * 不启动协程，不等 Core，不直接开始动画。
 */
class LocalInputVisualEditTracker {
    private val pending: ArrayDeque<LocalInputVisualEdit> = ArrayDeque()
    private var nextSequence: Long = 1L

    /**
     * 记录本次本地输入 — 只入队，不触发任何动画。
     * @return 分配的 [LocalInputVisualEdit.sequence]。
     */
    fun record(
        oldText: String,
        newText: String,
        oldSelection: TextRange,
        newSelection: TextRange,
        changes: List<LocalInputChange>,
    ): Long {
        if (changes.isEmpty()) return 0L
        val seq = nextSequence++
        pending.addLast(
            LocalInputVisualEdit(
                sequence = seq,
                oldText = oldText,
                newText = newText,
                oldSelection = oldSelection,
                newSelection = newSelection,
                changes = changes,
            ),
        )
        return seq
    }

    /**
     * #694 评论第 3 步：找 newText == [newText] 的 pending local edit / 连续 edit chain。
     *
     * 配对策略：优先找 `edit.newText == newText` 的最新一笔（IME 已上屏的最终正文）。
     * 找到则从队列移除并返回；否则返回 null（可能是 Core 驱动的修改，无本地输入配对）。
     *
     * 连续快速输入（空串 -> a -> ab -> abc）会留下多笔 pending；
     * onAuthoritativeLayout 拿到最终 "abc" 的 layout 时，配对 newText == "abc" 的那一笔，
     * 用其 oldText（空串）作为 T0，跳过中间 "a"/"ab" 的中间态。
     */
    fun drainMatching(newText: String): LocalInputVisualEdit? {
        // 从最新往最旧找 newText 匹配的一笔（最终态配对）。
        for (i in pending.indices.reversed()) {
            val edit = pending[i]
            if (edit.newText == newText) {
                // 移除该笔及之前所有更旧的 pending（它们已被这一笔的最终态覆盖）。
                while (pending.size > i) {
                    pending.removeFirst()
                }
                return edit
            }
        }
        return null
    }

    /** 是否有 pending 本地输入待配对。 */
    fun hasPending(): Boolean = pending.isNotEmpty()

    /** pending 数量 — 供诊断/测试。 */
    fun pendingSize(): Int = pending.size

    /** 清空所有 pending — 章节切换/detach 时调用。 */
    fun clear() {
        pending.clear()
        nextSequence = 1L
    }
}
