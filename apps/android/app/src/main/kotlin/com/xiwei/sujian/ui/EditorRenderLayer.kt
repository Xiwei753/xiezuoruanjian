package com.xiwei.sujian.ui

import android.graphics.Canvas
import android.text.Spannable
import android.text.style.ForegroundColorSpan

/**
 * EditorRenderLayer v0
 *
 * 统一管理 WriterEditText 上的视觉绘制层，收拢原本散落在 WriterEditText 里的各类 renderer。
 *
 * 职责：
 * - 在系统文本绘制前后注入自定义视觉逻辑
 * - 管理 smooth cursor 绘制
 * - 管理 typing animation overlay 绘制
 * - 管理动画的生命周期
 * - 管理动画期间的字符隐藏（animated skip ranges）
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

    /// Byte ranges that should be hidden during system text drawing
    /// because an insert animation is active. The overlay will draw
    /// ghost glyphs animating from the cursor position instead.
    private val animatedSkipRanges = mutableListOf<Pair<Int, Int>>()
    private val activeSkipSpans = mutableListOf<ForegroundColorSpan>()

    fun setScrolling(scrolling: Boolean) {
        if (isScrolling == scrolling) return
        isScrolling = scrolling
        typingOverlayRenderer.setPausedForScroll(scrolling)
        smoothCursorRenderer.setScrolling(scrolling)
    }

    /**
     * Add a skip range for insert animation.
     * During the animation, characters in [start, end) will be hidden
     * from system text drawing so the overlay ghost can animate them.
     */
    fun addSkipRange(start: Int, end: Int) {
        animatedSkipRanges.add(Pair(start, end))
    }

    /**
     * Clear all skip ranges and remove any active transparent spans.
     */
    fun clearSkipRanges() {
        removeSkipSpans()
        animatedSkipRanges.clear()
    }

    /**
     * 在系统文本绘制之前调用。
     * 负责隐藏原生 cursor（当 smooth cursor 开启时）。
     * 负责对 animated skip ranges 中的字符应用透明 span。
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
        // Apply transparent spans to hide characters being animated
        applySkipSpans()
    }

    /**
     * 在系统文本绘制之后调用。
     * 负责画 smooth cursor 和 typing overlay。
     * 负责移除透明 span（恢复字符可见性供下一帧使用）。
     */
    fun drawAfterText(canvas: Canvas) {
        // Remove skip spans after system draw so the Editable is clean
        // for the next frame's beforeTextDraw to re-apply if needed
        removeSkipSpans()
        smoothCursorRenderer.draw(canvas)
        if (isScrolling) return
        typingOverlayRenderer.onDraw(canvas)
    }

    private fun applySkipSpans() {
        if (animatedSkipRanges.isEmpty()) return
        val editable = editText.text as? Spannable ?: return
        for ((start, end) in animatedSkipRanges) {
            if (start >= end || start >= editable.length) continue
            val safeEnd = end.coerceAtMost(editable.length)
            val span = ForegroundColorSpan(0x00000000) // fully transparent
            editable.setSpan(span, start, safeEnd, Spannable.SPAN_EXCLUSIVE_EXCLUSIVE)
            activeSkipSpans.add(span)
        }
    }

    private fun removeSkipSpans() {
        if (activeSkipSpans.isEmpty()) return
        val editable = editText.text as? Spannable ?: return
        for (span in activeSkipSpans) {
            editable.removeSpan(span)
        }
        activeSkipSpans.clear()
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
        // For insert animations, add skip range so system text draw hides
        // the newly inserted characters during the animation
        if (!anim.isDeletion && anim.insertedStart >= 0) {
            val end = anim.insertedStart + anim.insertedText.length
            addSkipRange(anim.insertedStart, end)
        }
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
        clearSkipRanges()
    }

    fun onDetachedFromWindow() {
        smoothCursorRenderer.onDetachedFromWindow()
        typingOverlayRenderer.clear()
        clearSkipRanges()
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
