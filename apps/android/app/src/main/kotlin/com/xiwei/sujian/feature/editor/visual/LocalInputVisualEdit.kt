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
 * 2. [drainMatchingChain] 等下一份真实 TextLayoutResult 到来时配对消费（由 onAuthoritativeLayout 调用）；
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
     * #694 评论 5691696678 问题1：在 pending 队列里找一条连续本地输入 chain —
     * 首笔 `oldText == [presentedOldText]`、末笔 `newText == [finalNewText]`，
     * 且 chain 中后一笔 `oldText == 前一笔 newText`（连续输入链）。
     *
     * 配对策略（修复快速输入中间 layout 被跳过的丢 patch 问题）：
     * - 连续快速输入 `"" -> "a" -> "ab" -> "abc"` 会留下多笔 pending；
     * - 若 Compose 中间两个 layout 没真正呈现，`lastPresentedLayout` 还是 `""`，
     *   最后只收到 `"abc"` 的 layout；
     * - 旧实现 `drainMatching("abc")` 只返回最后一笔 `"ab" -> "abc"`，
     *   `buildLocalInputPatch` 检查 `oldText("ab") != oldLayout.text("")` 直接丢 patch；
     * - 新实现 `drainMatchingChain("", "abc")` 返回完整 chain
     *   `[""->"a", "a"->"ab", "ab"->"abc"]`，用首笔 oldText("") 作 T0、末笔 newText("abc") 作 Tn。
     *
     * 找到后从队列移除该 chain 及其之前所有更旧的 pending（它们已被这条 chain 覆盖），
     * 返回 chain 列表（按入队顺序）。找不到返回 null（可能是 Core 驱动的修改，无本地输入配对）。
     *
     * chain 不要求是整个 pending 队列，只要找到一条满足首尾+连续条件的子链即可；
     * 多条候选时优先找最长的连续 chain（覆盖最多中间态，T0→Tn 最完整）。
     * 退化情况：chain 长度为 1（单笔 `oldText == presentedOldText && newText == finalNewText`）。
     *
     * @param presentedOldText 上一次真正呈现的 layout 正文（T0）。
     * @param finalNewText 本次权威 layout 的正文（Tn）。
     * @return 连续 chain 列表（按入队顺序）；找不到返回 null。
     *
     * #698 评论 5697612595 chainSize > 1 reflow 收口 —
     * 返回的 chain 只包含 [LocalInputVisualEdit]（text/selection/changes），
     * **不含 layout 对象**。[ComposeEditorVisualState.buildLocalInputPatch] 只用 chain 的
     * text/changes 算 offset map 和 stage 顺序，不创建中间 layout — 中间笔可能从未真正 layout 过
     * （快速输入中间 layout 被跳过），虚构中间 layout 会引入不存在的几何导致 reflow 跳变。
     */
    @Suppress("CognitiveComplexMethod")
    fun drainMatchingChain(
        presentedOldText: String,
        finalNewText: String,
    ): List<LocalInputVisualEdit>? {
        val n = pending.size
        if (n == 0) return null

        var bestStart = -1
        var bestEnd = -1 // exclusive

        // 枚举每个可能的起点（首笔 oldText == presentedOldText），向后扩展连续链。
        for (start in 0 until n) {
            if (pending[start].oldText != presentedOldText) continue
            // 从 start 开始扩展连续链：后一笔 oldText == 前一笔 newText。
            var end = start + 1 // exclusive
            while (end < n && pending[end].oldText == pending[end - 1].newText) {
                end++
            }
            // [start, end) 是一条连续链，首 oldText == presentedOldText。
            // 在 [start, end) 中找最后一个 newText == finalNewText 的位置（最长子链）。
            var lastMatchIdx = -1
            for (i in start until end) {
                if (pending[i].newText == finalNewText) {
                    lastMatchIdx = i
                }
            }
            if (lastMatchIdx != -1) {
                val chainLen = lastMatchIdx + 1 - start
                val bestLen = if (bestStart == -1) 0 else bestEnd - bestStart
                if (chainLen > bestLen) {
                    bestStart = start
                    bestEnd = lastMatchIdx + 1
                }
            }
        }

        if (bestStart == -1) return null

        // 收集 chain: pending[bestStart until bestEnd]。
        val chain = (bestStart until bestEnd).map { pending[it] }
        // 从队列移除该 chain 及其之前所有更旧的 pending（[0, bestEnd)）。
        // 它们已被这条 chain 的最终态覆盖，不再需要配对。
        repeat(bestEnd) {
            pending.removeFirst()
        }
        return chain
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
