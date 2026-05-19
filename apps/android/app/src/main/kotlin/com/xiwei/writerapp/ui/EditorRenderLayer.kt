package com.xiwei.writerapp.ui

import android.graphics.Canvas

/**
 * EditorRenderLayer v0
 *
 * 统一管理 WriterEditText 上的视觉绘制层，收拢原本散落在 WriterEditText 里的各类 renderer（如 typing overlay, smooth cursor）。
 *
 * 职责：
 * - 在系统文本绘制前后注入自定义视觉逻辑
 * - 管理动画和 span 的生命周期
 *
 * 未来可扩展：
 * - 段落锚点/导图关联节点标识绘制
 * - AI 批注高亮层
 */
class EditorRenderLayer(private val editText: WriterEditText) {

    val typingOverlayRenderer = TypingOverlayRenderer(editText)
    val smoothCursorRenderer = SmoothCursorRenderer(editText)

    fun beforeTextDraw() {
        if (smoothCursorRenderer.smoothCursorEnabled && editText.selectionStart == editText.selectionEnd) {
            editText.isCursorVisible = false
        }
    }

    fun drawAfterText(canvas: Canvas) {
        smoothCursorRenderer.draw(canvas)
        typingOverlayRenderer.onDraw(canvas)
    }

    fun clear() {
        typingOverlayRenderer.clear()
        // smoothCursorRenderer clear or reset if needed
    }

    fun onDetachedFromWindow() {
        smoothCursorRenderer.onDetachedFromWindow()
        typingOverlayRenderer.clear()
    }

    fun onFocusChanged(focused: Boolean) {
        smoothCursorRenderer.onFocusChanged(focused)
    }

    fun onSelectionChanged(selStart: Int, selEnd: Int) {
        smoothCursorRenderer.onSelectionChanged(selStart, selEnd)
    }
}
