package com.xiwei.sujian.editor.selfrender

import android.graphics.Canvas

import android.graphics.Paint
import android.graphics.Path
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import com.xiwei.sujian.model.AnimationModeData
import com.xiwei.sujian.model.SujianReflowGlyphRectData

/**
 * 动画中的 overlay 项
 */
data class SujianOverlayAnim(
    val id: ULong,
    val kind: String,
    val text: String,
    val startX: Float,
    val startY: Float,
    val startBaselineY: Float,
    val endX: Float,
    val endY: Float,
    val endBaselineY: Float,
    val durationMs: Long,
    val startTimeMs: Long,
    val glyphRects: List<SujianGlyphRect>,
    val insertRangeId: ULong? = null,
    val reflowRects: List<SujianReflowGlyphRectData> = emptyList(),
    val reflowRangeIds: List<ULong> = emptyList(),
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
 * - 真吞吐：静态层跳过 animated range + overlay 层绘制
 * - 禁止：ghost overlay（正文完整绘制后叠 ghost 必然重影）
 * - 禁止：透明 span 污染 Editable
 *
 * ## 渲染规则
 * - 静态正文使用 StaticLayout 绘制
 * - 动画期间，静态层跳过 animated range 避免重影
 * - 删除动画使用删除前 glyph rect 快照
 * - 光标、选区、preedit、动画独立图层绘制
 * - 滚动中暂停/清理文字动画
 * - SnapshotAnimation 尚未实现，统一降级为跳过，不创建 overlay，不创建 hidden range
 * - hidden range 生命周期只通过稳定 ID 管理，byte range 只用于静态层绘制排除
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

    // ── 动画期间跳过的正文范围（支持多个并发 insert 动画，用 ID 追踪防止映射后残留） ──
    private data class ActiveInsertRangeEntry(val id: ULong, val range: HalfOpenRange)
    private val activeInsertRanges = mutableListOf<ActiveInsertRangeEntry>()
    private var nextInsertRangeId: ULong = 1u

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

    // ── Composing 光标视觉状态（由 drawComposingTextAndUnderline 计算）──
    internal var hasComposingCursor: Boolean = false
    internal var composingCursorX: Float = 0f
    internal var composingCursorTop: Float = 0f
    internal var composingCursorBottom: Float = 0f

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
     * 添加一个动画期间跳过的正文范围（UTF-16 offset HalfOpenRange）
     * 每个 insert 动画独立添加，动画结束后只移除自己的 range
     * @return 该 range 的唯一 ID，用于后续精确移除
     */
    fun addActiveInsertRange(range: HalfOpenRange): ULong {
        val id = nextInsertRangeId++
        activeInsertRanges.add(ActiveInsertRangeEntry(id, range))
        return id
    }

    /**
     * 移除指定跳过范围（insert 动画完成时按 ID 调用）
     */
    fun removeActiveInsertRangeById(id: ULong) {
        activeInsertRanges.removeAll { it.id == id }
    }

    /**
     * 清空所有跳过范围，同时取消拥有这些 range 的动画（防止重影）
     */
    fun clearActiveInsertRanges() {
        val clearedIds = activeInsertRanges.map { it.id }.toSet()
        activeInsertRanges.clear()
        // 取消拥有被清除 range 的动画，否则静态层已显示文字但动画层还在画 ghost
        if (clearedIds.isNotEmpty()) {
            activeAnimations.removeAll { anim ->
                when (anim.kind) {
                    "insert", "cluster", "run" -> anim.insertRangeId != null && anim.insertRangeId in clearedIds
                    "reflow" -> anim.reflowRangeIds.any { it in clearedIds }
                    else -> false
                }
            }
        }
    }

    /**
     * 映射已有 activeInsertRanges 以响应文本插入（UTF-16 offset）。
     *
     * 遍历所有已有 range：
     * - pos <= range.start：range 后移 len → HalfOpenRange(range.start + len, range.end + len)
     * - pos >= range.last：不变
     * - pos 在 range 内部（range.first < pos < range.last）：从列表中移除（相交取消策略）
     *
     * 相交取消时同时取消对应动画，防止静态层已显示文字但动画层还在画 ghost。
     */
    fun mapActiveInsertRangesForInsert(pos: Int, len: Int) {
        val canceledIds = mutableListOf<ULong>()
        val newRanges = mutableListOf<ActiveInsertRangeEntry>()
        for (entry in activeInsertRanges) {
            val range = entry.range
            when {
                pos <= range.start -> newRanges.add(entry.copy(range = HalfOpenRange(range.start + len, range.end + len)))
                pos >= range.end -> newRanges.add(entry)
                else -> { canceledIds.add(entry.id) /* 相交，取消 */ }
            }
        }
        activeInsertRanges.clear()
        activeInsertRanges.addAll(newRanges)
        // 相交取消的 range 对应的动画也必须取消
        cancelAnimationsByRangeIds(canceledIds)
    }

    /**
     * 映射已有 activeInsertRanges 以响应文本删除（UTF-16 offset）。
     *
     * 遍历所有已有 range：
     * - 删除范围完全在 range 前（pos + len <= range.start）：range 前移 → HalfOpenRange(range.start - len, range.end - len)
     * - 删除范围完全在 range 后（pos >= range.last）：不变
     * - 删除范围和 range 相交：从列表中移除（相交取消策略）
     *
     * 相交取消时同时取消对应动画，防止静态层已显示文字但动画层还在画 ghost。
     */
    fun mapActiveInsertRangesForDelete(pos: Int, len: Int) {
        val canceledIds = mutableListOf<ULong>()
        val newRanges = mutableListOf<ActiveInsertRangeEntry>()
        for (entry in activeInsertRanges) {
            val range = entry.range
            when {
                pos + len <= range.start -> newRanges.add(entry.copy(range = HalfOpenRange(range.start - len, range.end - len)))
                pos >= range.end -> newRanges.add(entry)
                else -> { canceledIds.add(entry.id) /* 相交，取消 */ }
            }
        }
        activeInsertRanges.clear()
        activeInsertRanges.addAll(newRanges)
        // 相交取消的 range 对应的动画也必须取消
        cancelAnimationsByRangeIds(canceledIds)
    }

    /**
     * 清理已完成的动画
     */
    fun tickAnimations() {
        val finishedInsertAnims = activeAnimations.filter { 
            (it.kind == "insert" || it.kind == "cluster" || it.kind == "run") && it.isFinished 
        }
        for (anim in finishedInsertAnims) {
            val rangeId = anim.insertRangeId
            if (rangeId != null) {
                activeInsertRanges.removeAll { it.id == rangeId }
            }
        }
        // 收集已完成的 reflow 动画的 range ID，精确移除对应的跳过范围
        val finishedReflowAnims = activeAnimations.filter { it.kind == "reflow" && it.isFinished }
        for (anim in finishedReflowAnims) {
            for (rangeId in anim.reflowRangeIds) {
                activeInsertRanges.removeAll { it.id == rangeId }
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
     * 取消拥有指定 range ID 的动画，同时移除这些动画拥有的所有 range。
     *
     * 当 range 因相交被取消时调用，确保静态层和动画层状态一致，防止重影。
     * 如果 reflow 动画的某个 range 被取消，整个 reflow 动画及其所有 range 都会被移除。
     */
    private fun cancelAnimationsByRangeIds(rangeIds: List<ULong>) {
        if (rangeIds.isEmpty()) return
        val idSet = rangeIds.toSet()

        // 找到所有拥有被取消 range 的动画
        val animationsToCancel = activeAnimations.filter { anim ->
            when (anim.kind) {
                "insert", "cluster", "run" -> anim.insertRangeId != null && anim.insertRangeId in idSet
                "reflow" -> anim.reflowRangeIds.any { it in idSet }
                else -> false
            }
        }

        // 收集这些动画拥有的所有 range ID（包括未被直接取消的），防止孤立 range
        val allRangeIdsToRemove = mutableSetOf<ULong>()
        for (anim in animationsToCancel) {
            if (anim.insertRangeId != null) allRangeIdsToRemove.add(anim.insertRangeId)
            allRangeIdsToRemove.addAll(anim.reflowRangeIds)
        }
        activeInsertRanges.removeAll { it.id in allRangeIdsToRemove }

        // 移除被取消的动画
        activeAnimations.removeAll { it in animationsToCancel }
    }

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
        composingText: String,
        composingCursor: Int,
        viewportWidth: Int,
        viewportHeight: Int
    ) {
        // 重置 composing 光标状态
        hasComposingCursor = false

        canvas.save()
        canvas.translate(-scrollX.toFloat(), -scrollY.toFloat())

        // 1. 选区高亮层
        drawSelection(canvas, layout, text, selection)

        // 2. 静态正文层
        drawStaticText(canvas, layout, text, scrollY, viewportHeight)

        // 3. preedit 层（composing 文字 + 下划线）
        if (composingText.isNotEmpty() && composingStart >= 0) {
            // setComposingText 场景：composing 文字不在 buffer.text 中，需要临时绘制
            drawComposingTextAndUnderline(canvas, layout, text, composingStart, composingText, composingCursor)
        } else if (composingStart >= 0 && composingEnd >= 0 && composingStart < composingEnd && composingStart < text.length) {
            // setComposingRegion 场景：composing 文字已在正文中，只画下划线
            drawComposingUnderline(canvas, layout, text, composingStart, composingEnd)
        }

        // 4. 动画层
        drawAnimations(canvas)

        // 5. 光标层
        if (cursorVisible && cursorBlinkOn && selection.isCollapsed) {
            if (hasComposingCursor) {
                // composing 期间：光标跟随 composingCursor 在 composing 文字中的位置
                canvas.drawRect(
                    composingCursorX - cursorPaint.strokeWidth / 2f,
                    composingCursorTop,
                    composingCursorX + cursorPaint.strokeWidth / 2f,
                    composingCursorBottom,
                    cursorPaint
                )
            } else {
                drawCursor(canvas, layout, text, selection.head)
            }
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

        val excludeRanges = activeInsertRanges.map { it.range }

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
                !(lineEnd <= er.start || lineStart >= er.end)
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
                !(lineEnd <= er.start || lineStart >= er.end)
            }
            if (intersectingRanges.isEmpty()) continue // 已在批量绘制中处理

            // 计算该行需要排除的 offset 段，合并重叠区间
            val excludeSegments = mutableListOf<Pair<Int, Int>>()
            for (er in intersectingRanges) {
                val segStart = maxOf(lineStart, er.start)
                val segEnd = minOf(lineEnd, er.end)
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

    /**
     * 绘制 composing 文字和下划线（setComposingText 场景）
     *
     * composing 文字不在 buffer.text 中，需要在光标位置临时绘制。
     * 使用 StaticLayout 正确处理换行，首行从光标位置开始，后续行从左边距开始。
     * 同时计算 composing 光标位置，供光标层使用。
     */
    private fun drawComposingTextAndUnderline(
        canvas: Canvas,
        layout: Layout,
        text: String,
        composingStart: Int,
        composingText: String,
        composingCursor: Int
    ) {
        if (composingText.isEmpty()) return

        // 获取 composing 文字起始位置（光标在主布局中的位置）
        val safeOffset = composingStart.coerceIn(0, text.length)
        val startX: Float
        val startBaselineY: Float

        if (text.isEmpty()) {
            // 空文本时 composing 从左上角开始
            startX = 0f
            startBaselineY = textPaint.textSize
        } else {
            val startLine = layout.getLineForOffset(safeOffset)
            startX = layout.getPrimaryHorizontal(safeOffset)
            startBaselineY = layout.getLineBaseline(startLine).toFloat()
        }

        // 为 composing 文字创建临时 StaticLayout
        // 使用 LeadingMarginSpan 让首行从 startX 位置开始，后续行从左边距开始
        val layoutWidth = layout.width.coerceAtLeast(1)
        val composingLayout: StaticLayout

        if (startX > 0f) {
            // 首行有缩进偏移，使用 LeadingMarginSpan 处理
            val indentPx = Math.round(startX)
            val spannedString = android.text.SpannableString(composingText)
            val marginSpan = android.text.style.LeadingMarginSpan.Standard(indentPx, 0)
            spannedString.setSpan(marginSpan, 0, composingText.length, android.text.Spannable.SPAN_INCLUSIVE_INCLUSIVE)

            composingLayout = StaticLayout.Builder.obtain(
                spannedString, 0, spannedString.length, textPaint, layoutWidth
            ).setAlignment(Layout.Alignment.ALIGN_NORMAL)
             .setLineSpacing(layout.spacingAdd, layout.spacingMultiplier)
             .setIncludePad(false)
             .build()
        } else {
            composingLayout = StaticLayout.Builder.obtain(
                composingText, 0, composingText.length, textPaint, layoutWidth
            ).setAlignment(Layout.Alignment.ALIGN_NORMAL)
             .setLineSpacing(layout.spacingAdd, layout.spacingMultiplier)
             .setIncludePad(false)
             .build()
        }

        // 绘制 composing 文字和下划线
        val firstLineBaseline = composingLayout.getLineBaseline(0).toFloat()

        for (i in 0 until composingLayout.lineCount) {
            val lineStart = composingLayout.getLineStart(i)
            val lineEnd = composingLayout.getLineEnd(i)
            if (lineStart >= lineEnd) continue

            val lineText = composingText.substring(lineStart, lineEnd)
            val drawBaselineY = startBaselineY + (composingLayout.getLineBaseline(i).toFloat() - firstLineBaseline)

            // LeadingMarginSpan 影响布局计算，getLineLeft 返回含 margin 偏移的绘制起点
            val lineDrawX = composingLayout.getLineLeft(i)

            // 绘制文字
            canvas.drawText(lineText, lineDrawX, drawBaselineY, textPaint)

            // 绘制下划线
            val descent = composingLayout.getLineDescent(i).toFloat()
            val underlineY = drawBaselineY + descent + 2f
            val textWidth = textPaint.measureText(lineText)
            if (textWidth > 0f) {
                canvas.drawLine(lineDrawX, underlineY, lineDrawX + textWidth, underlineY, composingUnderlinePaint)
            }
        }

        // 计算 composing 光标位置
        val cursorOffset = composingCursor.coerceIn(0, composingText.length)
        val cursorLine = composingLayout.getLineForOffset(cursorOffset)
        val cursorXInComposing = composingLayout.getPrimaryHorizontal(cursorOffset)
        val cursorBaselineY = startBaselineY + (composingLayout.getLineBaseline(cursorLine).toFloat() - firstLineBaseline)
        val cursorAscent = composingLayout.getLineAscent(cursorLine).toFloat()
        val cursorDescent = composingLayout.getLineDescent(cursorLine).toFloat()

        hasComposingCursor = true
        composingCursorX = cursorXInComposing
        composingCursorTop = cursorBaselineY + cursorAscent
        composingCursorBottom = cursorBaselineY + cursorDescent
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
                "cluster" -> {
                    // Cluster 动画：复杂 grapheme（emoji 等）整组作为一个 ghost
                    // 与 insert 类似，但整组从起点到目标位置
                    for (glyphRect in anim.glyphRects) {
                        val currentX = anim.startX + (glyphRect.x - anim.startX) * easedProgress
                        val currentBaselineY = anim.startBaselineY + (glyphRect.baselineY - anim.startBaselineY) * easedProgress
                        val currentAlpha = (255 * easedProgress).toInt()
                        val currentScale = 0.8f + 0.2f * easedProgress

                        animTextPaint.alpha = currentAlpha
                        canvas.save()
                        canvas.scale(currentScale, currentScale, currentX, currentBaselineY)
                        canvas.drawText(glyphRect.char, currentX, currentBaselineY, animTextPaint)
                        canvas.restore()
                    }
                }
                "run" -> {
                    // Run 动画：分组动画（9-40 clusters）
                    // 整组从起点淡入到目标位置
                    for (glyphRect in anim.glyphRects) {
                        val currentX = anim.startX + (glyphRect.x - anim.startX) * easedProgress
                        val currentBaselineY = anim.startBaselineY + (glyphRect.baselineY - anim.startBaselineY) * easedProgress
                        val currentAlpha = (255 * easedProgress).toInt()

                        animTextPaint.alpha = currentAlpha
                        canvas.drawText(glyphRect.char, currentX, currentBaselineY, animTextPaint)
                    }
                }

            }
        }
    }
}
