package com.xiwei.sujian.editor.selfrender

import android.graphics.Paint
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import android.os.Build

/**
 * Glyph 矩形信息，用于动画坐标
 */
data class SujianGlyphRect(
    val x: Float,
    val y: Float,        // top 坐标（baseline + ascent）
    val w: Float,
    val h: Float,
    val char: String,
    val baselineY: Float  // 文字基线 Y 坐标
)

/**
 * 光标矩形信息，包含 baselineY
 */
data class SujianCursorRect(
    val x: Float,
    val top: Float,
    val bottom: Float,
    val baselineY: Float
)

/**
 * 行信息缓存
 */
data class SujianLineInfo(
    val baseline: Int,
    val ascent: Int,
    val descent: Int,
    val top: Int,
    val bottom: Int,
    val startOffset: Int,
    val endOffset: Int
)

/**
 * SujianEditorLayout — 自研写作区布局引擎
 *
 * 使用 TextPaint + StaticLayout 计算文本行布局。
 * 文本未变时复用 layout cache，避免每帧全量布局。
 *
 * ## 架构原则
 * - 只负责布局计算和坐标查询
 * - 不负责绘制（由 SujianEditorRenderer 负责）
 * - 不负责文本变更（由 SujianEditorBuffer 负责）
 */
class SujianEditorLayout(
    private val textPaint: TextPaint
) {
    // ── 布局参数 ──
    var availableWidth: Int = 0
        private set
    var spacingMultiplier: Float = 1.0f
        private set
    var spacingExtra: Float = 0f
        private set
    var firstLineIndentPx: Float = 0f
        private set

    // ── 缓存的布局 ──
    private var cachedLayout: StaticLayout? = null
    private var cachedText: String? = null
    private var cachedWidth: Int = -1

    // ── 行信息缓存 ──
    private var lineInfos: List<SujianLineInfo> = emptyList()

    /**
     * 更新布局参数。参数变化时标记需要重新布局。
     */
    fun updateParams(
        width: Int,
        spacingMultiplier: Float,
        spacingExtra: Float,
        firstLineIndentPx: Float
    ) {
        var needsRelayout = false
        if (this.availableWidth != width) {
            this.availableWidth = width
            needsRelayout = true
        }
        if (this.spacingMultiplier != spacingMultiplier) {
            this.spacingMultiplier = spacingMultiplier
            needsRelayout = true
        }
        if (this.spacingExtra != spacingExtra) {
            this.spacingExtra = spacingExtra
            needsRelayout = true
        }
        if (this.firstLineIndentPx != firstLineIndentPx) {
            this.firstLineIndentPx = firstLineIndentPx
            needsRelayout = true
        }
        if (needsRelayout) {
            invalidate()
        }
    }

    /**
     * 获取或创建 StaticLayout。
     * 如果文本和宽度未变，复用缓存。
     */
    fun getLayout(text: String): StaticLayout {
        val cached = cachedLayout
        if (cached != null && cachedText == text && cachedWidth == availableWidth) {
            return cached
        }
        return rebuildLayout(text)
    }

    /**
     * 强制重建布局
     */
    fun invalidate() {
        cachedLayout = null
        cachedText = null
        cachedWidth = -1
        lineInfos = emptyList()
    }

    /**
     * 获取布局高度（像素）
     */
    fun getHeight(text: String): Int {
        return getLayout(text).height
    }

    /**
     * 获取行数
     */
    fun getLineCount(text: String): Int {
        return getLayout(text).lineCount
    }

    /**
     * 根据 offset 获取行号（安全处理越界）
     */
    fun getLineForOffset(text: String, offset: Int): Int {
        val layout = getLayout(text)
        val safeOffset = offset.coerceIn(0, text.length)
        if (layout.lineCount == 0) return 0
        return layout.getLineForOffset(safeOffset).coerceIn(0, layout.lineCount - 1)
    }

    /**
     * 根据行号和 x 坐标获取 offset（安全处理越界）
     */
    fun getOffsetForHorizontal(text: String, line: Int, x: Float): Int {
        val layout = getLayout(text)
        if (layout.lineCount == 0) return 0
        val safeLine = line.coerceIn(0, layout.lineCount - 1)
        return layout.getOffsetForHorizontal(safeLine, x).coerceIn(0, text.length)
    }

    /**
     * 获取指定行的信息
     */
    fun getLineInfo(text: String, line: Int): SujianLineInfo? {
        ensureLineInfos(text)
        if (line < 0 || line >= lineInfos.size) return null
        return lineInfos[line]
    }

    /**
     * 获取所有行信息
     */
    fun getAllLineInfos(text: String): List<SujianLineInfo> {
        ensureLineInfos(text)
        return lineInfos
    }

    /**
     * 根据 y 坐标获取行号
     */
    fun getLineForY(text: String, y: Float): Int {
        val layout = getLayout(text)
        return layout.getLineForVertical(y.toInt()).coerceIn(0, layout.lineCount - 1)
    }

    /**
     * 根据 x, y 坐标获取光标偏移（UTF-16）
     */
    fun getOffsetForPosition(text: String, x: Float, y: Float): Int {
        val layout = getLayout(text)
        val line = layout.getLineForVertical(y.toInt()).coerceIn(0, layout.lineCount - 1)
        return layout.getOffsetForHorizontal(line, x).coerceIn(0, text.length)
    }

    /**
     * 获取指定偏移量处的光标 x 坐标
     */
    fun getCursorX(text: String, offset: Int): Float {
        val layout = getLayout(text)
        val safeOffset = offset.coerceIn(0, text.length)
        if (layout.lineCount == 0) return 0f
        val line = layout.getLineForOffset(safeOffset)
        return layout.getPrimaryHorizontal(safeOffset)
    }

    /**
     * 获取指定偏移量处的光标 y 坐标（基线）
     */
    fun getCursorY(text: String, offset: Int): Float {
        val layout = getLayout(text)
        val safeOffset = offset.coerceIn(0, text.length)
        if (layout.lineCount == 0) return 0f
        val line = layout.getLineForOffset(safeOffset)
        return layout.getLineBaseline(line).toFloat()
    }

    /**
     * 获取指定偏移量处的光标矩形
     */
    fun getCursorRect(text: String, offset: Int): SujianCursorRect {
        val layout = getLayout(text)
        val safeOffset = offset.coerceIn(0, text.length)
        if (layout.lineCount == 0) {
            // 空文本：baselineY = textSize（第一行基线位置）
            return SujianCursorRect(0f, 0f, textPaint.textSize, textPaint.textSize)
        }
        val line = layout.getLineForOffset(safeOffset)
        val x = layout.getPrimaryHorizontal(safeOffset)
        val baseline = layout.getLineBaseline(line).toFloat()
        val ascent = layout.getLineAscent(line).toFloat()
        val descent = layout.getLineDescent(line).toFloat()
        return SujianCursorRect(x, baseline + ascent, baseline + descent, baseline)
    }

    /**
     * 获取指定文本范围的 glyph 矩形列表
     * 用于动画坐标计算
     */
    fun getGlyphRects(text: String, startOffset: Int, endOffset: Int): List<SujianGlyphRect> {
        val layout = getLayout(text)
        val result = mutableListOf<SujianGlyphRect>()
        if (text.isEmpty() || startOffset >= endOffset || startOffset >= text.length) return result

        val safeStart = startOffset.coerceIn(0, text.length)
        val safeEnd = endOffset.coerceIn(0, text.length)

        var currentOffset = safeStart
        while (currentOffset < safeEnd) {
            val codePoint = text.codePointAt(currentOffset)
            val charCount = Character.charCount(codePoint)
            val charStr = text.substring(currentOffset, (currentOffset + charCount).coerceAtMost(safeEnd))

            val line = layout.getLineForOffset(currentOffset)
            val x = layout.getPrimaryHorizontal(currentOffset)
            val baseline = layout.getLineBaseline(line).toFloat()
            val ascent = layout.getLineAscent(line).toFloat()
            val descent = layout.getLineDescent(line).toFloat()

            // 计算 glyph 宽度
            val nextX = if (currentOffset + charCount < text.length) {
                layout.getPrimaryHorizontal(currentOffset + charCount)
            } else {
                x + textPaint.measureText(charStr)
            }
            val width = nextX - x

            result.add(SujianGlyphRect(
                x = x,
                y = baseline + ascent,
                w = width.coerceAtLeast(0f),
                h = descent - ascent,
                char = charStr,
                baselineY = baseline  // 新增
            ))

            currentOffset += charCount
        }

        return result
    }

    // ── 内部方法 ──

    private fun rebuildLayout(text: String): StaticLayout {
        val builder = StaticLayout.Builder.obtain(text, 0, text.length, textPaint, availableWidth.coerceAtLeast(1))
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setLineSpacing(spacingExtra, spacingMultiplier)
            .setIncludePad(false)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            builder.setJustificationMode(Layout.JUSTIFICATION_MODE_NONE)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            builder.setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE)
            builder.setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE)
        }

        // 首行缩进：对每个段落独立应用 LeadingMarginSpan.Standard(indentPx, 0)
        // 空段落也应用缩进，确保换行后的空行首行也有缩进
        if (firstLineIndentPx > 0f && text.isNotEmpty()) {
            val indentPx = firstLineIndentPx.toInt()
            val spannedString = android.text.SpannableString(text)
            var start = 0
            while (start <= text.length) {
                val end = text.indexOf('\n', start).let { if (it < 0) text.length else it }
                val span = android.text.style.LeadingMarginSpan.Standard(indentPx, 0)
                spannedString.setSpan(span, start, end.coerceAtLeast(start), android.text.Spannable.SPAN_INCLUSIVE_INCLUSIVE)
                start = end + 1
                if (end == text.length) break
            }
            builder.setText(spannedString)
        }

        val layout = builder.build()
        cachedLayout = layout
        cachedText = text
        cachedWidth = availableWidth
        lineInfos = emptyList() // 清空行信息缓存

        return layout
    }

    private fun ensureLineInfos(text: String) {
        if (lineInfos.isNotEmpty()) return
        val layout = getLayout(text)
        lineInfos = (0 until layout.lineCount).map { line ->
            SujianLineInfo(
                baseline = layout.getLineBaseline(line),
                ascent = layout.getLineAscent(line),
                descent = layout.getLineDescent(line),
                top = layout.getLineTop(line),
                bottom = layout.getLineBottom(line),
                startOffset = layout.getLineStart(line),
                endOffset = layout.getLineEnd(line)
            )
        }
    }
}
