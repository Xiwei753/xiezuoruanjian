package com.xiwei.sujian.editor.selfrender

import android.graphics.Canvas

import android.graphics.Paint
import android.graphics.Path
import android.text.Layout
import android.text.TextPaint
import com.xiwei.sujian.model.SujianReflowGlyphRectData

/**
 * 动画中的 overlay 项
 */
data class SujianOverlayAnim(
    val id: ULong,
    val kind: String,          // "insert" | "delete" | "cursor" | "reflow"
    val text: String,
    val startX: Float,
    val startY: Float,
    val startBaselineY: Float,  // 新增：起始基线 Y
    val endX: Float,
    val endY: Float,
    val endBaselineY: Float,    // 新增：目标基线 Y
    val durationMs: Long,
    val startTimeMs: Long,
    val glyphRects: List<SujianGlyphRect>,
    val insertRange: IntRange? = null,  // insert 动画的跳过范围，动画完成时精确移除
    val reflowRects: List<SujianReflowGlyphRectData> = emptyList(),  // reflow 动画的新位置数据
    val reflowInsertRanges: List<IntRange> = emptyList()  // reflow 动画的 UTF-16 跳过范围，用于 tickAnimations 精确移除
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
 * SujianEditorRenderer — 自研写作区渲染器（唯一主路径）
 *
 * 分层绘制：静态正文层 → 选区高亮层 → preedit 层 → 动画层 → 光标层
 *
 * ## 动画路线
 * - 真吞吐：静态层跳过 inserted range + overlay 层绘制
 * - 禁止：ghost overlay（正文完整绘制后叠 ghost 必然重影）
 * - 禁止：透明 span 污染 Editable
 *
 * ## 渲染规则
 * - 静态正文使用 StaticLayout 绘制
 * - 动画期间，静态层跳过 animated insert range 避免重影
 * - 删除动画使用删除前 snapshot glyph rect
 * - 光标、选区、preedit、动画独立图层绘制
 * - 滚动中暂停/清理文字动画
 */
class SujianEditorRenderer(
    private val textPaint: TextPaint,
    private val density: Float  // resources.displayMetrics.density
) {
    // ── 绘制工具 ──
    internal val selectionPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0 // 默认值，由 setThemeColors() 覆盖
        style = Paint.Style.FILL
    }

    internal val cursorPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0 // 默认值，由 setThemeColors() 覆盖
        style = Paint.Style.FILL
        strokeWidth = 1.5f * density  // 固定 1.5dp
    }

    internal val composingUnderlinePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0 // 默认值，由 setThemeColors() 覆盖
        style = Paint.Style.STROKE
        strokeWidth = 1.5f
    }

    private val animTextPaint = TextPaint(textPaint)

    /**
     * 从主题注入颜色，覆盖所有硬编码默认值。
     * 由 SujianEditorView.applyThemeColors() 调用。
     */
    fun setThemeColors(textColor: Int, cursorColor: Int, composingColor: Int, selectionColor: Int) {
        cursorPaint.color = cursorColor
        composingUnderlinePaint.color = composingColor
        selectionPaint.color = selectionColor
        animTextPaint.color = textColor
    }

    // ── 动画状态 ──
    private val activeAnimations = mutableListOf<SujianOverlayAnim>()
    private var isScrolling = false

    // ── 动画期间跳过的正文范围（支持多个并发 insert 动画） ──
    private val activeInsertRanges = mutableListOf<IntRange>()

    // ── 光标状态 ──
    var cursorVisible: Boolean = true
    var cursorBlinkOn: Boolean = true

    // ── 光标视觉状态（由 CursorController 驱动）──
    var cursorVisualX: Float = 0f
    var cursorVisualTop: Float = 0f
    var cursorVisualBottom: Float = 0f
    var cursorTargetX: Float = 0f
    var cursorTargetTop: Float = 0f
    var cursorTargetBottom: Float = 0f
    var smoothCursorEnabled: Boolean = false
    var isCursorAnimating: Boolean = false  // 由 CursorController 设置

    /**
     * 添加动画 overlay
     */
    fun addAnimation(anim: SujianOverlayAnim): Boolean {
        if (isScrolling) return false
        // 移除同 id 的旧动画
        activeAnimations.removeAll { it.id == anim.id }
        activeAnimations.add(anim)
        return true
    }

    /**
     * 添加一个动画期间跳过的正文范围（UTF-16 offset IntRange）
     * 每个 insert 动画独立添加，动画结束后只移除自己的 range
     */
    fun addActiveInsertRange(range: IntRange) {
        activeInsertRanges.add(range)
    }

    /**
     * 移除指定跳过范围（insert 动画完成时调用）
     */
    fun removeActiveInsertRange(range: IntRange) {
        activeInsertRanges.remove(range)
    }

    /**
     * 清空所有跳过范围
     */
    fun clearActiveInsertRanges() {
        activeInsertRanges.clear()
    }

    /**
     * 映射已有 activeInsertRanges 以响应文本插入（UTF-16 offset）。
     *
     * 遍历所有已有 range：
     * - pos <= range.first：range 后移 len → IntRange(range.first + len, range.last + len)
     * - pos >= range.last：不变
     * - pos 在 range 内部（range.first < pos < range.last）：从列表中移除（相交取消策略）
     */
    fun mapActiveInsertRangesForInsert(pos: Int, len: Int) {
        val newRanges = mutableListOf<IntRange>()
        for (range in activeInsertRanges) {
            when {
                pos <= range.first -> newRanges.add(IntRange(range.first + len, range.last + len))
                pos >= range.last -> newRanges.add(range)
                else -> { /* range.first < pos < range.last：相交，取消 */ }
            }
        }
        activeInsertRanges.clear()
        activeInsertRanges.addAll(newRanges)
    }

    /**
     * 映射已有 activeInsertRanges 以响应文本删除（UTF-16 offset）。
     *
     * 遍历所有已有 range：
     * - 删除范围完全在 range 前（pos + len <= range.first）：range 前移 → IntRange(range.first - len, range.last - len)
     * - 删除范围完全在 range 后（pos >= range.last）：不变
     * - 删除范围和 range 相交：从列表中移除（相交取消策略）
     */
    fun mapActiveInsertRangesForDelete(pos: Int, len: Int) {
        val newRanges = mutableListOf<IntRange>()
        for (range in activeInsertRanges) {
            when {
                pos + len <= range.first -> newRanges.add(IntRange(range.first - len, range.last - len))
                pos >= range.last -> newRanges.add(range)
                else -> { /* 相交，取消 */ }
            }
        }
        activeInsertRanges.clear()
        activeInsertRanges.addAll(newRanges)
    }

    /**
     * @deprecated 使用 addActiveInsertRange / clearActiveInsertRanges 代替
     * 保留向后兼容，内部转为列表操作
     */
    fun setAnimatedInsertRange(range: IntRange?) {
        activeInsertRanges.clear()
        if (range != null) {
            activeInsertRanges.add(range)
        }
    }

    /**
     * 清理已完成的动画
     */
    fun tickAnimations() {
        // 先收集已完成的 insert 动画的 range，精确移除对应的跳过范围
        val finishedInsertAnims = activeAnimations.filter { it.kind == "insert" && it.isFinished }
        for (anim in finishedInsertAnims) {
            val range = anim.insertRange
            if (range != null) {
                activeInsertRanges.remove(range)
            }
        }
        // 收集已完成的 reflow 动画的 range，精确移除对应的跳过范围
        val finishedReflowAnims = activeAnimations.filter { it.kind == "reflow" && it.isFinished }
        for (anim in finishedReflowAnims) {
            for (range in anim.reflowInsertRanges) {
                activeInsertRanges.remove(range)
            }
        }
        activeAnimations.removeAll { it.isFinished }
    }

    /**
     * 查询当前是否正在滚动
     */
    fun isScrolling(): Boolean = isScrolling

    /**
     * 设置滚动状态
     */
    fun setScrolling(scrolling: Boolean) {
        isScrolling = scrolling
        if (scrolling) {
            activeAnimations.clear()
            activeInsertRanges.clear()
        }
    }

    /**
     * 清理所有动画
     */
    fun clearAnimations() {
        activeAnimations.clear()
        activeInsertRanges.clear()
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

        val excludeRanges = activeInsertRanges

        // 计算可视行范围
        val firstVisLine = layout.getLineForVertical(scrollY.coerceAtLeast(0))
        val lastVisLine = layout.getLineForVertical((scrollY + viewportHeight).coerceAtLeast(0))
            .coerceAtMost(layout.lineCount - 1)

        if (excludeRanges.isEmpty()) {
            // 快速路径：一次 layout.draw() + clipRect 限制到可视区域
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

        // 有 excludeRanges 时的路径：
        // 对每条 line，收集所有与之相交的 exclude ranges，分段绘制排除这些 ranges

        // 1. 收集不相交行的连续区间（与所有 exclude ranges 都不相交的行）
        val nonOverlapLineRanges = mutableListOf<Pair<Int, Int>>()
        var rangeStart = -1
        for (lineIdx in firstVisLine..lastVisLine) {
            val lineStart = layout.getLineStart(lineIdx)
            val lineEnd = layout.getLineEnd(lineIdx)
            val overlaps = excludeRanges.any { er ->
                !(lineEnd <= er.first || lineStart >= er.last)
            }
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

        // 2. 批量绘制不相交行
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

        // 3. 分段绘制相交行（排除所有 intersecting ranges）
        for (lineIdx in firstVisLine..lastVisLine) {
            val lineStart = layout.getLineStart(lineIdx)
            val lineEnd = layout.getLineEnd(lineIdx)

            // 收集该行相交的 exclude ranges
            val intersectingRanges = excludeRanges.filter { er ->
                !(lineEnd <= er.first || lineStart >= er.last)
            }
            if (intersectingRanges.isEmpty()) continue // 已在批量绘制中处理

            // 计算该行需要排除的 offset 段，合并重叠区间
            val excludeSegments = mutableListOf<Pair<Int, Int>>()
            for (er in intersectingRanges) {
                val segStart = maxOf(lineStart, er.first)
                val segEnd = minOf(lineEnd, er.last)
                if (segStart < segEnd) {
                    excludeSegments.add(Pair(segStart, segEnd))
                }
            }
            // 按 start 排序并合并重叠
            excludeSegments.sortBy { it.first }
            val merged = mutableListOf<Pair<Int, Int>>()
            for (seg in excludeSegments) {
                if (merged.isNotEmpty() && merged.last().second >= seg.first) {
                    // 重叠或相邻，合并
                    val last = merged.removeLast()
                    merged.add(Pair(last.first, maxOf(last.second, seg.second)))
                } else {
                    merged.add(seg)
                }
            }

            // 计算可见段（排除 merged 后的区间）
            val visibleSegments = mutableListOf<Pair<Int, Int>>()
            var pos = lineStart
            for ((exStart, exEnd) in merged) {
                if (pos < exStart) {
                    visibleSegments.add(Pair(pos, exStart))
                }
                pos = maxOf(pos, exEnd)
            }
            if (pos < lineEnd) {
                visibleSegments.add(Pair(pos, lineEnd))
            }

            // 绘制每个可见段
            for ((segStart, segEnd) in visibleSegments) {
                drawLineSegment(canvas, layout, text, lineIdx, segStart, segEnd)
            }
        }
    }

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

        // 使用 getSelectionPath 精确获取 visual run 的实际绘制范围，
        // 避免 getPrimaryHorizontal 两点裁剪在 RTL/Bidi 场景下将不连续 run 段错误合并。
        val path = Path()
        layout.getSelectionPath(segStart, safeEnd, path)
        if (path.isEmpty) return

        canvas.save()
        canvas.clipPath(path)
        layout.draw(canvas)
        canvas.restore()
    }

    private fun drawSelection(canvas: Canvas, layout: Layout, text: String, selection: SujianSelection) {
        if (selection.isCollapsed) return
        val start = selection.start.coerceIn(0, text.length)
        val end = selection.end.coerceIn(0, text.length)
        if (start >= end) return

        // 使用 getSelectionPath 精确绘制选区高亮，正确处理 RTL/Bidi 场景
        val path = Path()
        layout.getSelectionPath(start, end, path)
        if (path.isEmpty) return

        canvas.drawPath(path, selectionPaint)
    }

    private fun drawCursor(canvas: Canvas, layout: Layout, text: String, offset: Int) {
        if (text.isEmpty() && offset == 0) {
            // 空文本时在左上角画光标
            canvas.drawRect(0f, 0f, cursorPaint.strokeWidth, textPaint.textSize, cursorPaint)
            return
        }
        
        // 使用 CursorController 驱动的视觉位置
        val drawX: Float
        val drawTop: Float
        val drawBottom: Float
        
        if (smoothCursorEnabled && isCursorAnimating) {
            drawX = cursorVisualX
            drawTop = cursorVisualTop
            drawBottom = cursorVisualBottom
        } else {
            // 非动画时直接用 layout 计算
            val safeOffset = offset.coerceIn(0, text.length)
            val line = layout.getLineForOffset(safeOffset)
            drawX = layout.getPrimaryHorizontal(safeOffset)
            val baseline = layout.getLineBaseline(line).toFloat()
            val ascent = layout.getLineAscent(line).toFloat()
            val descent = layout.getLineDescent(line).toFloat()
            drawTop = baseline + ascent
            drawBottom = baseline + descent
        }
        
        canvas.drawRect(
            drawX - cursorPaint.strokeWidth / 2f,
            drawTop,
            drawX + cursorPaint.strokeWidth / 2f,
            drawBottom,
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
        // 使用 getSelectionPath + clipPath 绘制下划线，
        // 确保 Bidi 不连续 visual run 的空隙不会被画上线。
        val startLine = layout.getLineForOffset(composingStart)
        val endLine = layout.getLineForOffset(composingEnd)

        for (line in startLine..endLine) {
            val lineStart = layout.getLineStart(line)
            val lineEnd = layout.getLineEnd(line)
            val cStart = if (line == startLine) composingStart else lineStart
            val cEnd = if (line == endLine) composingEnd else lineEnd

            if (cStart >= cEnd) continue

            val linePath = Path()
            layout.getSelectionPath(cStart, cEnd, linePath)
            if (linePath.isEmpty) continue

            val baseline = layout.getLineBaseline(line).toFloat()
            val descent = layout.getLineDescent(line).toFloat()
            val underlineY = baseline + descent + 2f

            canvas.save()
            canvas.clipPath(linePath)
            canvas.drawLine(0f, underlineY, layout.width.toFloat(), underlineY, composingUnderlinePaint)
            canvas.restore()
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
                        // 使用 baselineY 做插值，而非 y
                        val currentBaselineY = anim.startBaselineY + (glyphRect.baselineY - anim.startBaselineY) * easedProgress
                        val currentAlpha = (255 * easedProgress).toInt()

                        animTextPaint.alpha = currentAlpha
                        // Canvas.drawText 永远用 baselineY
                        canvas.drawText(glyphRect.char, currentX, currentBaselineY, animTextPaint)
                    }
                }
                "delete" -> {
                    // 删除动画：从 glyphRects 吞向 newCursorRect
                    for (glyphRect in anim.glyphRects) {
                        val currentX = glyphRect.x + (anim.endX - glyphRect.x) * easedProgress
                        // 使用 baselineY 做插值
                        val currentBaselineY = glyphRect.baselineY + (anim.endBaselineY - glyphRect.baselineY) * easedProgress
                        val currentAlpha = (255 * (1f - easedProgress)).toInt()
                        val currentScale = 1f - easedProgress * 0.5f

                        animTextPaint.alpha = currentAlpha
                        canvas.save()
                        canvas.scale(currentScale, currentScale, currentX, currentBaselineY)
                        canvas.drawText(glyphRect.char, currentX, currentBaselineY, animTextPaint)
                        canvas.restore()
                    }
                }
                "reflow" -> {
                    // Reflow 动画：插入点右侧 glyph 的位移动画
                    // 方案 A：静态层跳过 reflow ranges，overlay 从旧位置到新位置
                    // opacity: 1.0（始终可见，因为静态层跳过了这些 glyph）
                    // scale: 1.0（无缩放变化）
                    // position: 旧位置 → 新位置
                    if (anim.reflowRects.isNotEmpty() && anim.glyphRects.size == anim.reflowRects.size) {
                        animTextPaint.alpha = 255
                        for (i in anim.glyphRects.indices) {
                            val oldGlyph = anim.glyphRects[i]
                            val newRect = anim.reflowRects[i]
                            // 从旧位置插值到新位置
                            val currentX = oldGlyph.x + (newRect.newX.toFloat() - oldGlyph.x) * easedProgress
                            val currentBaselineY = oldGlyph.baselineY + (newRect.newBaselineY.toFloat() - oldGlyph.baselineY) * easedProgress
                            canvas.drawText(oldGlyph.char, currentX, currentBaselineY, animTextPaint)
                        }
                    }
                }
            }
        }
    }
}
