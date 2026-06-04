package com.xiwei.sujian.ui

import android.graphics.Canvas

/**
 * EditorRenderLayer v0
 *
 * 统一管理 WriterEditText 上的视觉绘制层，收拢原本散落在 WriterEditText 里的各类 renderer。
 *
 * 职责：
 * - 在系统文本绘制前后注入自定义视觉逻辑
 * - 管理 smooth cursor 绘制
 * - 管理 typing animation overlay 绘制
 * - 管理动画和 hidden span 的生命周期
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
     */
    fun drawAfterText(canvas: Canvas) {
        smoothCursorRenderer.draw(canvas)
        if (isScrolling) return
        typingOverlayRenderer.onDraw(canvas)
    }

    /**
     * 添加一个 typing animation。
     * TypingAnimationController 应通过此方法添加动画，而不是直接操作 TypingOverlayRenderer。
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
     * 清理所有 typing animations 和关联的 hidden spans。
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
