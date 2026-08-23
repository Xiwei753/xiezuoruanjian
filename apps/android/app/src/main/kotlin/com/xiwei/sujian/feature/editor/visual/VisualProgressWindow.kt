package com.xiwei.sujian.feature.editor.visual

/**
 * #637 评论 5386066978 项2：可 rebase 视觉对象的"本次事务内剩余时间"窗口。
 *
 * 问题：rebase 时只保存当前几何/alpha/reBases，新事务的全局 progress 又
 * 从 0 开始跑完整时长。一个吞字已经走到 50%，下一次退格来了以后，新事务让剩下
 * 50% 再花完整 100ms — 单段看是直线，连续输入时速度一直被重置，人眼看到
 * "拖一下、追一下"。
 *
 * 解决：每个可 rebase 的视觉对象（AnimatedSlice / BlockShift / CursorTransition）
 * 携带一个 [VisualProgressWindow]。renderer 先用 [map] 把= 把全局 progress 映射到
 * 本对象的 local progress，位置/alpha/reveal 都吃 localProgress。
 *
 * - 新事务首次播放：[Full]（[start]=0, [end]=1），map(progress)=progress，行为不变。
 * - rebase continuation：旧帧已走 fraction f，新事务设置 [end] = 1 - f，
 *   map(progress) = progress / (1 - f)。原来已走 60% 的字符只用新事务剩余
 *   40ms 完成，不会又慢吞吞跑 100ms。
 *
 * 边界稳定：
 * - globalProgress <= start → 0f
 * - globalProgress >= end → 1f
 * - span 退化（end <= start）时按最小跨度处理，避免除零。
 *
 * 不加 easing — [AnimationTimeline.progress] 和 renderer 本就是线性插值，
 * 问题不是曲线不够 linear，而是 rebase 反复重置剩余路程。
 */
data class VisualProgressWindow(
    val start: Float = 0f,
    val end: Float = 1f,
) {
    /**
     * 把全局事务 progress 映射到本对象的 local progress。
     *
     * 返回值始终在 [0f, 1f]：globalProgress <= start → 0f，
     * globalProgress >= end → 1f，中间线性插值。
     */
    fun map(globalProgress: Float): Float {
        if (globalProgress <= start) return 0f
        if (globalProgress >= end) return 1f
        val span = (end - start).coerceAtLeast(MIN_SPAN)
        return ((globalProgress - start) / span).coerceIn(0f, 1f)
    }

    /**
     * #637 评论 5386066978 项2：从旧帧已消费的 local fraction 构造 continuation 窗口。
     *
     * 旧帧已走 [consumedFraction]（0 = 刚开始，1 = 已完成），新事务只需播放剩余
     * 1 - consumedFraction 部分。返回 [VisualProgressWindow](start=0, end=1-consumedFraction)。
     *
     * consumedFraction >= 1f 时返回 [Full]（对象已完成，map 任何 progress 都得 1f，
     * renderer 自然跳过）；consumedFraction <= 0f 时返回 [Full]（对象未开始，
     * 新事务从头播放）。
     */
    fun continued(consumedFraction: Float): VisualProgressWindow {
        val f = consumedFraction.coerceIn(0f, 1f)
        if (f <= 0f || f >= 1f) return Full
        return VisualProgressWindow(start = 0f, end = 1f - f)
    }

    companion object {
        private const val MIN_SPAN = 0.0001f

        /** 完整窗口 [0, 1] — 新事务首次播放时使用，map(progress) = progress。 */
        val Full = VisualProgressWindow(start = 0f, end = 1f)
    }
}
