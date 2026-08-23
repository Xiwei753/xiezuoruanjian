package com.xiwei.sujian.feature.editor.visual

/**
 * #637 评论 5386066978 项2 / 评论 5386573878：可 rebase 视觉对象的"本次事务内剩余时间"窗口。
 *
 * 问题：rebase 时只保存当前几何/alpha/reBases，新事务的全局 progress 又
 * 从 0 开始跑完整时长。一个吞字已经走到 50%，下一次退格来了以后，新事务让剩下
 * 50% 再花完整 100ms — 单段看是直线，连续输入时速度一直被重置，人眼看到
 * "拖一下、追一下"。
 *
 * 解决：每个可 rebase 的视觉对象（AnimatedSlice / BlockShift / CursorTransition）
 * 携带一个 [VisualProgressWindow]。renderer 先用 [map] 把全局 progress 映射到
 * 本对象的 local progress，位置/alpha/reveal 都吃 localProgress。
 *
 * - 新事务首次播放：[Full]（[start]=0, [end]=1），map(progress)=progress，行为不变。
 * - rebase continuation：旧帧已走 fraction f，新事务设置 [end] = 1 - f，
 *   map(progress) = progress / (1 - f)。原来已走 60% 的字符只用新事务剩余
 *   40ms 完成，不会又慢吞吞跑 100ms。
 *
 * #637 评论 5386573878：连续 rebase 不能反复减速。snapshot 不再保存
 * localProgress（已消费比例），改为保存 [remainingFractionAt] 算出的
 * "当前帧之后还剩多少基准时长"。下一次 rebase 直接 [fromRemainingFraction]
 * 消费这个值，不再从 localProgress 重新推（否则旧 window [0,0.4] 走到 0.2
 * 时 localProgress=0.5，下一次又得到 end=0.5，剩余从 20ms 被放大回 50ms）。
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
     * #637 评论 5386573878：当前这一帧之后还剩多少基准时长（fraction）。
     *
     * - globalProgress <= start → 整个窗口 (end - start) 还没开始走。
     * - globalProgress >= end → 0f，已完成。
     * - 中间 → end - globalProgress。
     *
     * 返回值始终在 [0f, 1f]。snapshot 保存这个值，下一次 rebase 用
     * [fromRemainingFraction] 直接消费，连续 rebase 不会反复减速。
     */
    fun remainingFractionAt(globalProgress: Float): Float {
        if (globalProgress <= start) return (end - start).coerceIn(0f, 1f)
        if (globalProgress >= end) return 0f
        return (end - globalProgress).coerceIn(0f, 1f)
    }

    companion object {
        private const val MIN_SPAN = 0.0001f

        /** 完整窗口 [0, 1] — 新事务首次播放时使用，map(progress) = progress。 */
        val Full = VisualProgressWindow(start = 0f, end = 1f)

        /**
         * #637 评论 5386573878：从上一事务保存的剩余 fraction 构造 continuation 窗口。
         *
         * [remaining] 是旧 window 在当前帧之后还剩多少基准时长（由
         * [remainingFractionAt] 算出）。新事务窗口为 [0, remaining]，
         * 让剩余时长不被重新放大。remaining <= 0f 时返回 [Full]
         * （对象已完成，map 任何 progress 都得 1f，renderer 自然跳过）。
         */
        fun fromRemainingFraction(remaining: Float): VisualProgressWindow {
            val r = remaining.coerceIn(0f, 1f)
            if (r <= 0f) return Full
            return VisualProgressWindow(start = 0f, end = r)
        }
    }
}
