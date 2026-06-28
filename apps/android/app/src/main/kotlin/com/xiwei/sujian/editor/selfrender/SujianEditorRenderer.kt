package com.xiwei.sujian.editor.selfrender

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.text.Layout
import android.text.TextPaint

/**
 * 动画中的 overlay 项
 */
data class SujianOverlayAnim(
    val id: ULong,
    val kind: String,          // "insert" | "delete" | "cursor"
    val text: String,
    val startX: Float,
    val startY: Float,
    val endX: Float,
    val endY: Float,
    val durationMs: Long,
    val startTimeMs: Long,
    val glyphRects: List<SujianGlyphRect>
) {
    val isFinished: Boolean
        get() = (System.currentTimeMillis() - startTimeMs) >= durationMs

    val progress: Float
        get() {
            val elapsed = System.currentTimeMillis() - startTimeMs
            return (elapsed.toFloat() / durationMs.toFloat()).coerceIn(0f, 1f)
        }
}

/**
 * SujianEditorRenderer — 自研写作区渲染器
 *
 * 分层绘制：静态正文层 → 选区高亮层 → preedit 层 → 动画层 → 光标层
 *
 * ## 渲染规则
 * - 静态正文使用 StaticLayout 绘制
 * - 动画期间，静态层跳过 animated insert range 避免重影
 * - 删除动画使用删除前 snapshot glyph rect
 * - 光标、选区、preedit、动画独立图层绘制
 * - 滚动中暂停/清理文字动画
 */
class SujianEditorRenderer(
    private val textPaint: TextPaint
) {
    // ── 绘制工具 ──
    private val selectionPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(60, 100, 150, 255) // 半透明蓝色选区
        style = Paint.Style.FILL
    }

    internal val cursorPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(255, 50, 50, 50)
        style = Paint.Style.FILL
        strokeWidth = 2.5f * textPaint.textSize / 16f
    }

    private val composingUnderlinePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(180, 50, 50, 50)
        style = Paint.Style.STROKE
        strokeWidth = 1.5f
    }

    private val animTextPaint = TextPaint(textPaint)

    // ── 动画状态 ──
    private val activeAnimations = mutableListOf<SujianOverlayAnim>()
    private var isScrolling = false

    // ── 动画期间跳过的正文范围 ──
    private var animatedInsertRange: IntRange? = null

    // ── 光标状态 ──
    var cursorVisible: Boolean = true
    var cursorBlinkOn: Boolean = true

    /**
     * 添加动画 overlay
     */
    fun addAnimation(anim: SujianOverlayAnim) {
        if (isScrolling) return
        // 移除同 id 的旧动画
        activeAnimations.removeAll { it.id == anim.id }
        activeAnimations.add(anim)
    }

    /**
     * 设置动画期间跳过的正文范围（UTF-16 offset IntRange）
     * 插入动画期间设为 inserted range，动画结束后清除
     */
    fun setAnimatedInsertRange(range: IntRange?) {
        animatedInsertRange = range
    }

    /**
     * 清理已完成的动画
     */
    fun tickAnimations() {
        activeAnimations.removeAll { it.isFinished }
        // 如果没有活跃的插入动画，清除跳过范围
        val hasActiveInsert = activeAnimations.any { it.kind == "insert" && !it.isFinished }
        if (!hasActiveInsert) {
            animatedInsertRange = null
        }
    }

    /**
     * 设置滚动状态
     */
    fun setScrolling(scrolling: Boolean) {
        isScrolling = scrolling
        if (scrolling) {
            activeAnimations.clear()
            animatedInsertRange = null
        }
    }

    /**
     * 清理所有动画
     */
    fun clearAnimations() {
        activeAnimations.clear()
        animatedInsertRange = null
    }

    /**
     * 是否有活跃动画
     */
    fun hasActiveAnimations(): Boolean = activeAnimations.isNotEmpty()

    /**
     * 主绘制入口
     */
    fun draw(
        canvas: Canvas,
        layout: Layout,
        text: String,
        scrollX: Int,
        scrollY: Int,
        selection: SujianSelection,
        composingStart: Int,
        composingEnd: Int,
        viewportWidth: Int,
        viewportHeight: Int
    ) {
        canvas.save()
        canvas.translate(-scrollX.toFloat(), -scrollY.toFloat())

        // 1. 选区高亮层
        drawSelection(canvas, layout, text, selection)

        // 2. 静态正文层
        drawStaticText(canvas, layout, text, scrollY, viewportHeight)

        // 3. preedit 层（composing 下划线）
        if (composingStart >= 0 && composingEnd >= 0 && composingStart < composingEnd) {
            drawComposingUnderline(canvas, layout, text, composingStart, composingEnd)
        }

        // 4. 动画层
        drawAnimations(canvas)

        // 5. 光标层
        if (cursorVisible && cursorBlinkOn && selection.isCollapsed) {
            drawCursor(canvas, layout, text, selection.head)
        }

        canvas.restore()
    }

    // ── 各层绘制 ──

    private fun drawStaticText(
        canvas: Canvas,
        layout: Layout,
        text: String,
        scrollY: Int,
        viewportHeight: Int
    ) {
        if (text.isEmpty()) return

        val excludeRange = animatedInsertRange

        // 计算可视行范围
        val firstVisLine = layout.getLineForVertical(scrollY.coerceAtLeast(0))
        val lastVisLine = layout.getLineForVertical((scrollY + viewportHeight).coerceAtLeast(0))
            .coerceAtMost(layout.lineCount - 1)

        if (excludeRange == null) {
            // 快速路径：一次 layout.draw() + clipRect 限制到可视区域
            // 相比逐行 clipRect + layout.draw()，只调用一次 layout.draw() 显著减少重复绘制
            val visTop = layout.getLineTop(firstVisLine).toFloat()
            val visBottom = layout.getLineBottom(lastVisLine).toFloat()
            val visLeft = 0f
            val visRight = layout.width.toFloat()
            canvas.save()
            canvas.clipRect(visLeft, visTop, visRight, visBottom)
            layout.draw(canvas)
            canvas.restore()
            return
        }

        // 有 excludeRange：只对可视行做拆分
        // TODO(mid-term): 当前 drawLineFull 仍使用 clipRect + layout.draw() 逐行绘制
        // 长期应改为真正只绘制受影响行的文本段（drawText），避免每行重复 draw 完整 layout
        // 但 Layout.drawLine() 是 @hide API 不可用，当前 clipRect 限制是可行方案
        for (lineIdx in firstVisLine..lastVisLine) {
            val lineStart = layout.getLineStart(lineIdx)
            val lineEnd = layout.getLineEnd(lineIdx)

            // 该行与 excludeRange 不相交 → 正常绘制整行
            if (lineEnd <= excludeRange.first || lineStart >= excludeRange.last) {
                drawLineFull(canvas, layout, lineIdx)
                continue
            }

            // 相交 → 拆分为 before / hidden / after 三段
            // Before: [lineStart, min(lineEnd, excludeRange.first))
            val beforeEnd = minOf(lineEnd, excludeRange.first)
            if (beforeEnd > lineStart) {
                drawLineSegment(canvas, layout, text, lineIdx, lineStart, beforeEnd)
            }
            // Hidden: [excludeRange.first, excludeRange.last) — 跳过
            // After: [max(lineStart, excludeRange.last), lineEnd)
            val afterStart = maxOf(lineStart, excludeRange.last)
            if (afterStart < lineEnd) {
                drawLineSegment(canvas, layout, text, lineIdx, afterStart, lineEnd)
            }
        }
    }

    /**
     * 绘制完整的一行（无裁剪）
     * 使用 clipRect + layout.draw() 限制绘制范围到当前行。
     * 注意：Layout.drawLine() 是 @hide API 不可用，只能用 clip 限制。
     *
     * 性能说明：
     * - 无 excludeRange 时，drawStaticText 已改用单次 layout.draw() + 可视区域 clip，
     *   不会再调用此方法。
     * - 有 excludeRange 时，此方法仍用于不与 excludeRange 相交的行。
     * - 长期应改为 drawText 逐字符/逐段绘制，避免每行重复 draw 完整 layout。
     */
    private fun drawLineFull(canvas: Canvas, layout: Layout, lineIdx: Int) {
        val lineTop = layout.getLineTop(lineIdx)
        val lineBottom = layout.getLineBottom(lineIdx)
        val lineLeft = layout.getLineLeft(lineIdx)
        val lineRight = layout.getLineRight(lineIdx)
        canvas.save()
        canvas.clipRect(lineLeft, lineTop.toFloat(), lineRight, lineBottom.toFloat())
        layout.draw(canvas)
        canvas.restore()
    }

    /**
     * 绘制一行中的文本段 [segStart, segEnd)（UTF-16 offset）
     * 使用 Canvas.drawText 配合 Layout 的水平坐标
     */
    private fun drawLineSegment(
        canvas: Canvas,
        layout: Layout,
        text: String,
        lineIdx: Int,
        segStart: Int,
        segEnd: Int
    ) {
        if (segStart >= segEnd || segStart >= text.length) return
        val safeEnd = minOf(segEnd, text.length)

        // 逐字符绘制，处理 RTL/LTR 混合和 surrogate pair
        var offset = segStart
        while (offset < safeEnd) {
            val codePoint = text.codePointAt(offset)
            val charCount = Character.charCount(codePoint)
            val charStr = text.substring(offset, minOf(offset + charCount, safeEnd))

            val x = layout.getPrimaryHorizontal(offset)
            val baseline = layout.getLineBaseline(lineIdx).toFloat()

            canvas.drawText(charStr, x, baseline, textPaint)
            offset += charCount
        }
    }

    private fun drawSelection(canvas: Canvas, layout: Layout, text: String, selection: SujianSelection) {
        if (selection.isCollapsed) return
        val start = selection.start.coerceIn(0, text.length)
        val end = selection.end.coerceIn(0, text.length)
        if (start >= end) return

        val startLine = layout.getLineForOffset(start)
        val endLine = layout.getLineForOffset(end)

        for (line in startLine..endLine) {
            val lineStart = layout.getLineStart(line)
            val lineEnd = layout.getLineEnd(line)
            val selStart = if (line == startLine) start else lineStart
            val selEnd = if (line == endLine) end else lineEnd

            if (selStart >= selEnd) continue

            val left = layout.getPrimaryHorizontal(selStart)
            val right = layout.getPrimaryHorizontal(selEnd)
            val top = layout.getLineTop(line).toFloat()
            val bottom = layout.getLineBottom(line).toFloat()

            canvas.drawRect(left, top, right, bottom, selectionPaint)
        }
    }

    private fun drawCursor(canvas: Canvas, layout: Layout, text: String, offset: Int) {
        if (text.isEmpty() && offset == 0) {
            // 空文本时在左上角画光标
            canvas.drawRect(0f, 0f, cursorPaint.strokeWidth, textPaint.textSize, cursorPaint)
            return
        }

        val safeOffset = offset.coerceIn(0, text.length)
        val line = layout.getLineForOffset(safeOffset)
        val x = layout.getPrimaryHorizontal(safeOffset)
        val baseline = layout.getLineBaseline(line).toFloat()
        val ascent = layout.getLineAscent(line).toFloat()
        val descent = layout.getLineDescent(line).toFloat()

        canvas.drawRect(
            x - cursorPaint.strokeWidth / 2f,
            baseline + ascent,
            x + cursorPaint.strokeWidth / 2f,
            baseline + descent,
            cursorPaint
        )
    }

    private fun drawComposingUnderline(
        canvas: Canvas,
        layout: Layout,
        text: String,
        composingStart: Int,
        composingEnd: Int
    ) {
        val startLine = layout.getLineForOffset(composingStart)
        val endLine = layout.getLineForOffset(composingEnd)

        for (line in startLine..endLine) {
            val lineStart = layout.getLineStart(line)
            val lineEnd = layout.getLineEnd(line)
            val cStart = if (line == startLine) composingStart else lineStart
            val cEnd = if (line == endLine) composingEnd else lineEnd

            if (cStart >= cEnd) continue

            val left = layout.getPrimaryHorizontal(cStart)
            val right = layout.getPrimaryHorizontal(cEnd)
            val baseline = layout.getLineBaseline(line).toFloat()
            val descent = layout.getLineDescent(line).toFloat()

            canvas.drawLine(left, baseline + descent + 2f, right, baseline + descent + 2f, composingUnderlinePaint)
        }
    }

    private fun drawAnimations(canvas: Canvas) {
        for (anim in activeAnimations) {
            val progress = anim.progress
            if (progress >= 1f) continue

            // 使用 ease-out 插值
            val easedProgress = 1f - (1f - progress) * (1f - progress)

            when (anim.kind) {
                "insert" -> {
                    // 插入动画：从 oldCursorRect 吐到 glyphRect
                    for (glyphRect in anim.glyphRects) {
                        val currentX = anim.startX + (glyphRect.x - anim.startX) * easedProgress
                        val currentY = anim.startY + (glyphRect.y - anim.startY) * easedProgress
                        val currentAlpha = (255 * easedProgress).toInt()

                        animTextPaint.alpha = currentAlpha
                        canvas.drawText(glyphRect.char, currentX, currentY + glyphRect.h, animTextPaint)
                    }
                }
                "delete" -> {
                    // 删除动画：从 glyphRects 吞向 newCursorRect
                    for (glyphRect in anim.glyphRects) {
                        val currentX = glyphRect.x + (anim.endX - glyphRect.x) * easedProgress
                        val currentY = glyphRect.y + (anim.endY - glyphRect.y) * easedProgress
                        val currentAlpha = (255 * (1f - easedProgress)).toInt()
                        val currentScale = 1f - easedProgress * 0.5f

                        animTextPaint.alpha = currentAlpha
                        canvas.save()
                        canvas.scale(currentScale, currentScale, currentX, currentY + glyphRect.h)
                        canvas.drawText(glyphRect.char, currentX, currentY + glyphRect.h, animTextPaint)
                        canvas.restore()
                    }
                }
            }
        }
    }
}
