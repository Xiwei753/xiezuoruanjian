package com.xiwei.sujian.feature.editor.platform

import com.xiwei.sujian.feature.editor.session.ViewportAnchor
import com.xiwei.sujian.feature.editor.visual.VisualProgressWindow

/**
 * #638 评论 5403756824：视口事务级视觉过渡状态。
 *
 * viewport 作为视觉事务的一部分连续过渡，而非“动画期间不 clamp、结束后一次性 clamp”。
 * [fromScrollY] 是事务开始时屏幕真实 scrollY，[toScrollY] 是按新 Layout + 静态 cursor
 * + 新 maxScrollY 算出的最终合法 scrollY。每帧用 [progressWindow].map(globalProgress)
 * 把全局事务 progress 映射到本过渡的 local progress，线性插值 scrollY。
 *
 * - 第一次事务用 [VisualProgressWindow.Full]。
 * - 连续 rebase 时用上一帧保存的 [remainingFraction] 构造
 *   [VisualProgressWindow.fromRemainingFraction]，不能把 viewport 剩余路程重新摊满完整时长。
 */
internal data class ViewportVisualTransition(
    val transactionId: Long,
    val fromScrollY: Float,
    val toScrollY: Float,
    val progressWindow: VisualProgressWindow = VisualProgressWindow.Full,
)

/**
 * #633 评论 5379618506：编辑器视口唯一拥有者。
 *
 * 成为 scrollX / scrollY / maxScrollY / pendingInitialAnchor 的唯一 owner，
 * 替代 SujianEditorView 并行持有四个 viewport 相关可变状态的旧设计。
 *
 * 所有视口状态读写都经方法调用，scroll 范围 / 锚点恢复 / reflow 协调由本类保证一致性。
 * SujianEditorView 通过本类的方法操作视口，不再直接访问字段。
 *
 * 不手写 unsafe impl Send/Sync；不对外部输入用 unwrap/expect 代替错误处理。
 */
internal class EditorViewportController {
    var scrollX: Float = 0f
        private set
    var scrollY: Float = 0f
        private set
    var maxScrollY: Float = 0f
        private set

    private var pendingInitialAnchor: ViewportAnchor? = null
    private var initialRestoreConsumed: Boolean = false

    // #638 评论 5403756824：当前视口视觉过渡 + 最后一次 remainingFraction。
    // viewport 跟整笔视觉事务走，逐帧用 applyVisualFrame 推进 scrollY。
    private var visualTransition: ViewportVisualTransition? = null
    private var lastRemainingFraction: Float = 1f

    /**
     * 首次 attach 时排队等待恢复的逻辑锚点。
     * 只恢复一次：[consumeInitialRestoreIfReady] 成功消费后不再恢复。
     */
    fun queueInitialRestore(anchor: ViewportAnchor?) {
        pendingInitialAnchor = anchor
        initialRestoreConsumed = false
    }

    /**
     * 布局就绪时消费初始锚点并恢复。已消费过或布局未就绪时返回 null。
     */
    fun consumeInitialRestoreIfReady(layoutReady: Boolean): ViewportAnchor? {
        if (initialRestoreConsumed) return null
        if (!layoutReady) return null
        val anchor =
            pendingInitialAnchor ?: run {
                initialRestoreConsumed = true
                return null
            }
        initialRestoreConsumed = true
        pendingInitialAnchor = null
        return anchor
    }

    /**
     * 更新最大纵向滚动范围。若当前 scrollY 超出新范围则夹回。
     */
    fun updateMaxScroll(max: Float) {
        updateMaxScroll(max, clampNow = true)
    }

    /**
     * 更新最大纵向滚动范围。
     *
     * @param max 新的最大纵向滚动范围
     * @param clampNow true 时立即夹取 scrollY 到新范围；false 时只更新 maxScrollY，
     *                 留给调用方在动画完成/最终确定后再夹取（#638 视觉事务时序）。
     */
    fun updateMaxScroll(
        max: Float,
        clampNow: Boolean,
    ) {
        maxScrollY = max.coerceAtLeast(0f)
        if (clampNow && scrollY > maxScrollY) scrollY = maxScrollY
    }

    /**
     * 直接设置滚动位置（窗口重建/重绑定时）。
     */
    fun setScroll(
        x: Float,
        y: Float,
    ) {
        scrollX = x.coerceAtLeast(0f)
        scrollY = y.coerceIn(0f, maxScrollY)
    }

    /**
     * 设置 scrollX（restoreViewportAnchorNow 用）。
     */
    fun setScrollX(value: Float) {
        scrollX = value.coerceAtLeast(0f)
    }

    /**
     * 设置 scrollY（scrollToSelection / restoreViewportAnchorNow 用）。
     */
    fun setScrollY(value: Float) {
        scrollY = value.coerceIn(0f, maxScrollY)
    }

    /**
     * 设置 scrollY 不夹范围（scrollToSelection 中间步骤用，最后会 clamp）。
     */
    fun setScrollYUnclamped(value: Float) {
        scrollY = value
    }

    /**
     * 按增量调整 scrollY（onTouchEvent ACTION_MOVE 用）。
     */
    fun adjustScrollY(delta: Float) {
        scrollY = (scrollY + delta).coerceIn(0f, maxScrollY)
    }

    /**
     * 把 scrollY 夹到有效范围（高度变化后用）。
     */
    fun clamp() {
        scrollY = scrollY.coerceIn(0f, maxScrollY)
    }

    /**
     * #638 评论 5403756824：开始或重基视口视觉过渡。
     *
     * [targetScrollY] 是按新 Layout + 静态 cursor + 新 maxScrollY 算出的最终合法 scrollY。
     * [fromScrollY] 取当前屏幕真实 scrollY，保证过渡起点连续、不跳回旧起点。
     *
     * - 首次过渡（visualTransition == null）用 [VisualProgressWindow.Full]。
     * - 连续 rebase（visualTransition != null）用上一帧保存的 [lastRemainingFraction]
     *   构造 [VisualProgressWindow.fromRemainingFraction]，不把剩余路程重新摊满完整时长。
     */
    fun beginOrRebaseVisualTransition(
        transactionId: Long,
        targetScrollY: Float,
    ) {
        val fromScrollY = scrollY
        val window =
            if (visualTransition == null) {
                VisualProgressWindow.Full
            } else {
                VisualProgressWindow.fromRemainingFraction(lastRemainingFraction)
            }
        visualTransition = ViewportVisualTransition(transactionId, fromScrollY, targetScrollY, window)
    }

    /**
     * #638 评论 5403756824：用当前帧的全局事务 progress 推进视口 scrollY。
     *
     * scrollY = fromScrollY + (toScrollY - fromScrollY) * progressWindow.map(globalProgress)。
     * 同时保存本帧之后的 remainingFraction，供下一次 rebase 用。
     * 无活跃过渡时是 no-op。
     */
    fun applyVisualFrame(globalProgress: Float) {
        val tx = visualTransition ?: return
        val local = tx.progressWindow.map(globalProgress)
        scrollY = tx.fromScrollY + (tx.toScrollY - tx.fromScrollY) * local
        lastRemainingFraction = tx.progressWindow.remainingFractionAt(globalProgress)
    }

    /**
     * #638 评论 5403756824：事务完成后收尾视口过渡。
     *
     * 确保 scrollY == toScrollY（终点帧 applyVisualFrame(1f) 已设，此处兜底），
     * 清掉过渡状态，重置 lastRemainingFraction。下一帧静态 clamp() 应为 no-op。
     */
    fun endVisualTransition() {
        val tx = visualTransition ?: return
        scrollY = tx.toScrollY
        visualTransition = null
        lastRemainingFraction = 1f
    }

    /**
     * #638 评论 5403756824：是否有活跃视口视觉过渡。
     */
    fun hasVisualTransition(): Boolean = visualTransition != null

    /**
     * #638 评论 5403756824：当前视口视觉过渡的 transactionId（无过渡时 null）。
     */
    fun currentVisualTransitionTransactionId(): Long? = visualTransition?.transactionId

    /**
     * 重置全部视口状态（resetForReuse / unbindSession 用）。
     */
    fun reset() {
        scrollX = 0f
        scrollY = 0f
        maxScrollY = 0f
        pendingInitialAnchor = null
        initialRestoreConsumed = false
        visualTransition = null
        lastRemainingFraction = 1f
    }

    /**
     * 清除待恢复锚点（unbindSession 用）。
     */
    fun clearPendingAnchor() {
        pendingInitialAnchor = null
    }

    /**
     * #633 评论 5383643046：开始绑定新 target 的原子入口。
     *
     * 重置旧 target 的 scrollX/scrollY/maxScrollY/pendingInitialAnchor/initialRestoreConsumed，
     * 再接收可选逻辑锚点。target rebind 时 scroll/range/pending 必须作为一组一起重置，
     * 不能只 clearPendingAnchor 而留旧 scroll。
     */
    fun beginTarget(anchor: ViewportAnchor?) {
        scrollX = 0f
        scrollY = 0f
        maxScrollY = 0f
        pendingInitialAnchor = anchor
        initialRestoreConsumed = false
        visualTransition = null
        lastRemainingFraction = 1f
    }
}
