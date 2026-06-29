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

        // 有 excludeRange 时的优化路径：
        // 1. 不相交行：收集连续区间，批量 clipRect + layout.draw()，避免逐行 clipRect
        // 2. 相交行：保持 before/after 分段绘制（drawLineSegment）

        // 1. 收集不相交行的连续区间
        val nonOverlapLineRanges = mutableListOf<Pair<Int, Int>>() // (firstLine, lastLine) pairs
        var rangeStart = -1
        for (lineIdx in firstVisLine..lastVisLine) {
            val lineStart = layout.getLineStart(lineIdx)
            val lineEnd = layout.getLineEnd(lineIdx)
            val overlaps = !(lineEnd <= excludeRange.first || lineStart >= excludeRange.last)
            if (!overlaps) {
                if (rangeStart < 0) rangeStart = lineIdx
            } else {
                if (rangeStart >= 0) {
                    nonOverlapLineRanges.add(Pair(rangeStart, lineIdx - 1))
                    rangeStart = -1
                }
            }
        }
        if (rangeStart >= 0) {
            nonOverlapLineRanges.add(Pair(rangeStart, lastVisLine))
        }

        // 2. 批量绘制不相交行（一次 clipRect + layout.draw per 连续区间）
        for ((rangeFirst, rangeLast) in nonOverlapLineRanges) {
            val visTop = layout.getLineTop(rangeFirst).toFloat()
            val visBottom = layout.getLineBottom(rangeLast).toFloat()
            val visLeft = 0f
            val visRight = layout.width.toFloat()
            canvas.save()
            canvas.clipRect(visLeft, visTop, visRight, visBottom)
            layout.draw(canvas)
            canvas.restore()
        }

        // 3. 分段绘制相交行
        for (lineIdx in firstVisLine..lastVisLine) {
            val lineStart = layout.getLineStart(lineIdx)
            val lineEnd = layout.getLineEnd(lineIdx)
            if (lineEnd <= excludeRange.first || lineStart >= excludeRange.last) {
                continue // 已在批量绘制中处理
            }
            // before segment
            val beforeEnd = minOf(lineEnd, excludeRange.first)
            if (beforeEnd > lineStart) {
                drawLineSegment(canvas, layout, text, lineIdx, lineStart, beforeEnd)
            }
            // after segment
            val afterStart = maxOf(lineStart, excludeRange.last)
            if (afterStart < lineEnd) {
                drawLineSegment(canvas, layout, text, lineIdx, afterStart, lineEnd)
            }
        }
    }

    /**
     * 绘制一行中的文本段 [segStart, segEnd)（UTF-16 offset）
     * 
     * 使用 clipRect + Layout.draw() 分段绘制，复用 Layout 的 TextShaper 整形数据，
     * 避免逐 code point 硬画导致复杂 grapheme（emoji、ZWJ、组合字符、RTL）错位。
     * 
     * 长期路线：完全迁移到 grapheme cluster / StaticLayout / TextShaper 分段绘制，
     * 消除手动分段逻辑。当前实现通过 clipRect 复用 Layout 整形，等价于
     * StaticLayout 分段绘制的正确性。
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

        val lineTop = layout.getLineTop(lineIdx).toFloat()
        val lineBottom = layout.getLineBottom(lineIdx).toFloat()
        val xStart = layout.getPrimaryHorizontal(segStart)
        // safeEnd 可能等于行尾或文本末尾，需要安全处理
        val xEnd = if (safeEnd >= text.length) {
            layout.getLineRight(lineIdx)
        } else {
            layout.getPrimaryHorizontal(safeEnd)
        }

        if (xEnd <= xStart) return

        canvas.save()
        canvas.clipRect(xStart, lineTop, xEnd, lineBottom)
        // Layout.draw() 绘制整个 layout，clipRect 限制只显示目标范围
        // Layout 内部使用 TextShaper，正确处理 emoji/ZWJ/组合字符/RTL
        layout.draw(canvas)
        canvas.restore()
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
