package com.xiwei.sujian.feature.editor.layout

import android.text.SpannableStringBuilder
import android.text.TextPaint
import android.text.style.LeadingMarginSpan
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论3：首行缩进显示层投影契约测试 — 每个真实段落一个
 * LeadingMarginSpan.Standard（第一行缩进、续行不缩进），span 只作用于显示层，
 * 不改正文字符串；增量重同步只动受影响段落区域。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ParagraphStyleProjectionTest {
    private companion object {
        const val THREE_PARAGRAPHS = "第一段\n第二段\n第三段"
        const val TWO_PARAGRAPHS = "第一段\n第二段"
    }

    private val projection = ParagraphStyleProjection()
    private val paint =
        TextPaint().apply {
            textSize = 32f
        }

    private fun paragraphStarts(text: SpannableStringBuilder): List<Int> =
        text.getSpans(0, text.length, LeadingMarginSpan.Standard::class.java)
            .map { text.getSpanStart(it) }
            .sorted()

    private fun indentOf(
        text: SpannableStringBuilder,
        paragraphStart: Int,
    ): Int =
        text.getSpans(paragraphStart, paragraphStart + 1, LeadingMarginSpan.Standard::class.java)
            .map { it.getLeadingMargin(true) }
            .firstOrNull() ?: 0

    @Test
    fun applyFirstLineIndent_paragraphStartEachParagraph() {
        val text = SpannableStringBuilder(THREE_PARAGRAPHS)

        projection.applyFirstLineIndent(text, true, 2f, paint)

        assertEquals("每个段落起点都必须有 span", listOf(0, 4, 8), paragraphStarts(text))
        assertEquals("正文文本不得被修改", THREE_PARAGRAPHS, text.toString())
    }

    @Test
    fun firstLineMargin_usesFullWidthCharWidthTimesChars() {
        val text = SpannableStringBuilder("段落")
        val fullWidthPx = paint.measureText("中")

        projection.applyFirstLineIndent(text, true, 2f, paint)

        assertEquals("缩进像素 = 全角字符宽 × 2", (fullWidthPx * 2).toInt(), indentOf(text, 0))
        val margin =
            text.getSpans(0, 1, LeadingMarginSpan.Standard::class.java)
                .map { it.getLeadingMargin(true) }
                .first()
        assertTrue("缩进必须为正", margin > 0)
    }

    @Test
    fun continuationLinesNotIndented() {
        val text = SpannableStringBuilder("段落")
        projection.applyFirstLineIndent(text, true, 2f, paint)

        val margin = text.getSpans(0, 1, LeadingMarginSpan.Standard::class.java).first()
        assertEquals("续行 margin 必须为 0", 0, margin.getLeadingMargin(false))
    }

    @Test
    fun disabled_removesAllSpans() {
        val text = SpannableStringBuilder(TWO_PARAGRAPHS)
        projection.applyFirstLineIndent(text, true, 2f, paint)
        assertEquals(2, paragraphStarts(text).size)

        projection.applyFirstLineIndent(text, false, 2f, paint)

        assertEquals("关闭后不得残留 span", emptyList<Int>(), paragraphStarts(text))
    }

    @Test
    fun zeroWidth_removesAllSpans() {
        val text = SpannableStringBuilder(TWO_PARAGRAPHS)
        projection.applyFirstLineIndent(text, true, 2f, paint)

        projection.applyFirstLineIndent(text, true, 0f, paint)

        assertEquals("宽度为 0 等同于关闭", emptyList<Int>(), paragraphStarts(text))
    }

    @Test
    fun resyncRegion_onlyTouchesAffectedParagraphs() {
        val text = SpannableStringBuilder(THREE_PARAGRAPHS)
        projection.applyFirstLineIndent(text, true, 2f, paint)
        // 编辑只影响第一段：删除段内一个字符（区域 [1, 2)）。
        text.replace(1, 2, "")

        projection.resyncParagraphIndent(text, 1, 2, true, paint.measureText("中") * 2f)

        assertParagraphsCovered(text, "第段\n第二段\n第三段")
    }

    @Test
    fun resyncRegion_afterNewlineInsert_createsNewParagraphSpan() {
        val text = SpannableStringBuilder(TWO_PARAGRAPHS)
        projection.applyFirstLineIndent(text, true, 2f, paint)
        val fullWidthPx = paint.measureText("中") * 2f

        // 在第一段中间插入换行：正文变为 第一\n段\n第二段 — 新段落起点 3。
        text.insert(2, "\n")

        projection.resyncParagraphIndent(text, 2, 3, true, fullWidthPx)

        assertParagraphsCovered(text, "第一\n段\n第二段")
        assertEquals("正文不得被 span 逻辑改动", "第一\n段\n第二段", text.toString())
    }

    @Test
    fun resyncRegion_afterNewlineDelete_removesMergedParagraphSpan() {
        val text = SpannableStringBuilder(TWO_PARAGRAPHS)
        projection.applyFirstLineIndent(text, true, 2f, paint)
        val fullWidthPx = paint.measureText("中") * 2f

        // 删除段落间换行：两段合并为一个段落。
        text.delete(3, 4)

        projection.resyncParagraphIndent(text, 3, 3, true, fullWidthPx)

        assertEquals("合并后只剩一个段落 span", 1, text.getSpans(0, text.length, LeadingMarginSpan.Standard::class.java).size)
        assertParagraphsCovered(text, "第一段第二段")
    }

    /** 每个真实段落起点都必须有首行缩进 span 覆盖。 */
    private fun assertParagraphsCovered(
        text: SpannableStringBuilder,
        expected: String,
    ) {
        assertEquals(expected, text.toString())
        val paragraphStarts = mutableListOf(0)
        text.toString().forEachIndexed { index, c ->
            if (c == '\n') paragraphStarts.add(index + 1)
        }
        for (start in paragraphStarts) {
            assertTrue(
                "段落起点 $start 必须有首行缩进 span 覆盖",
                text.getSpans(start, start + 1, LeadingMarginSpan.Standard::class.java).isNotEmpty(),
            )
        }
    }

    @Test
    fun paragraphStartOf_findsContainingParagraph() {
        val text = SpannableStringBuilder("ab\ncd\nef")

        assertEquals(0, projection.paragraphStartOf(text, 0))
        assertEquals(0, projection.paragraphStartOf(text, 1))
        assertEquals(3, projection.paragraphStartOf(text, 4))
        assertEquals(6, projection.paragraphStartOf(text, 6))
        assertEquals(6, projection.paragraphStartOf(text, 7))
    }
}
