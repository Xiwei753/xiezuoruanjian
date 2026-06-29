package com.xiwei.sujian.ui

import android.graphics.Canvas

/**
 * EditorRenderLayer v1（旧版路线）
 *
 * **已废弃**：ghost overlay 路线已废弃。
 * 正文完整绘制后叠 ghost 必然重影，这是架构缺陷。
 * 真吞吐只在 SujianEditorView 上实现（静态层跳过 range + overlay 层绘制）。
 * 此类只作为旧版编辑器无动画兜底使用，不再新增功能。
 *
 * 统一管理 WriterEditText 上的视觉绘制层，收拢原本散落在 WriterEditText 里的各类 renderer。
 *
 * 职责：
 * - 在系统文本绘制前后注入自定义视觉逻辑
 * - 管理 smooth cursor 绘制
 * - 管理 typing animation ghost overlay 绘制（路线 B：非真吐字/吞字，正文始终完整绘制）
 * - 管理动画的生命周期
 *
 * 设计原则（一个结果一个来源）：
 * - 静态正文永远由系统 EditText 完整绘制，不做任何修改
 * - 动画只做 overlay（附加绘制），不修改 Editable
 * - 禁止向正文 Editable 注入透明 ForegroundColorSpan 隐藏文字
 *
 * 未来可扩展（预留接口，本次不实现）：
 * - 段落锚点 / 导图关联节点标识绘制
 * - AI 批注高亮层
 * - 搜索高亮层
 */
class EditorRenderLayer(private val editText: WriterEditText) {

    val typingOverlayRenderer = TypingOverlayRenderer(editText)
    val smoothCursorRenderer = SmoothCursorRenderer(editText)
    private var isScrolling = false

    fun setScrolling(scrolling: Boolean) {
        if (isScrolling == scrolling) return
        isScrolling = scrolling
        typingOverlayRenderer.setPausedForScroll(scrolling)
        smoothCursorRenderer.setScrolling(scrolling)
    }

    /**
     * 在系统文本绘制之前调用。
     * 负责隐藏原生 cursor（当 smooth cursor 开启时）。
     * 不再修改 Editable（不注入透明 span）。
     */
    fun beforeTextDraw() {
        if (isScrolling) {
            if (smoothCursorRenderer.smoothCursorEnabled) {
                editText.isCursorVisible = false
            }
            return
        }
        if (smoothCursorRenderer.smoothCursorEnabled && editText.selectionStart == editText.selectionEnd) {
            editText.isCursorVisible = false
        }
    }

    /**
     * 在系统文本绘制之后调用。
     * 负责画 smooth cursor 和 typing overlay。
     * 不再需要移除透明 span（因为不再注入）。
     */
    fun drawAfterText(canvas: Canvas) {
        smoothCursorRenderer.draw(canvas)
        if (isScrolling) return
        typingOverlayRenderer.onDraw(canvas)
    }

    /**
     * 添加一个 typing animation。
     * TypingAnimationController 应通过此方法添加动画，而不是直接操作 TypingOverlayRenderer。
     * 静态正文永远完整绘制，动画只做附加 overlay。
     */
    fun addTypingAnim(anim: OverlayAnim) {
        if (isScrolling) {
            typingOverlayRenderer.removeAnim(anim)
            return
        }
        typingOverlayRenderer.addAnim(anim)
    }

    /**
     * 移除一个 typing animation。
     */
    fun removeTypingAnim(anim: OverlayAnim) {
        typingOverlayRenderer.removeAnim(anim)
    }

    /**
     * 清理所有 typing animations。
     * 必须在 runWithoutTextAnimations()、onDetachedFromWindow()、setText() 等场景调用。
     */
    fun clear() {
        typingOverlayRenderer.clear()
    }

    fun onDetachedFromWindow() {
        smoothCursorRenderer.onDetachedFromWindow()
        typingOverlayRenderer.clear()
    }

    fun onEditorResume() {
        smoothCursorRenderer.onEditorResume()
        typingOverlayRenderer.onEditorResume()
    }

    fun onFocusChanged(focused: Boolean) {
        smoothCursorRenderer.onFocusChanged(focused)
    }

    fun onSelectionChanged(selStart: Int, selEnd: Int) {
        smoothCursorRenderer.onSelectionChanged(selStart, selEnd)
    }
}
