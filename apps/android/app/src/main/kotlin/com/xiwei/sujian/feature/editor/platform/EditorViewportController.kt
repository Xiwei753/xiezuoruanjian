package com.xiwei.sujian.feature.editor.platform

import com.xiwei.sujian.feature.editor.session.ViewportAnchor

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
        maxScrollY = max.coerceAtLeast(0f)
        if (scrollY > maxScrollY) scrollY = maxScrollY
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
     * 重置全部视口状态（resetForReuse / unbindSession 用）。
     */
    fun reset() {
        scrollX = 0f
        scrollY = 0f
        maxScrollY = 0f
        pendingInitialAnchor = null
        initialRestoreConsumed = false
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
    }
}
