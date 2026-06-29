package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

/**
 * 首行缩进按段落应用逻辑测试
 *
 * 验证 rebuildLayout 中 LeadingMarginSpan 按段落应用的遍历逻辑：
 * - 每个段落（以 \n 分隔）独立应用 LeadingMarginSpan.Standard(indentPx, 0)
 * - 空段落也应用缩进
 * - 段落边界正确计算
 *
 * 注意：由于 StaticLayout/SpannableString 是 Android framework 类，
 * 单元测试中无法直接使用。这里测试的是段落分割的纯逻辑部分。
 */
class SujianEditorLayoutIndentTest {

    /**
     * 模拟 rebuildLayout 中的段落遍历逻辑
     * 返回每个段落的 (start, end) 列表
     */
    private fun computeParagraphRanges(text: String): List<Pair<Int, Int>> {
        val ranges = mutableListOf<Pair<Int, Int>>()
        var start = 0
        while (start <= text.length) {
            val end = text.indexOf('\n', start).let { if (it < 0) text.length else it }
            ranges.add(Pair(start, end.coerceAtLeast(start)))
            start = end + 1
            if (end == text.length) break
        }
        return ranges
    }

    // ── 单段落 ──

    @Test
    fun testParagraphRanges_singleParagraph_noNewline() {
        val ranges = computeParagraphRanges("hello")
        assertEquals(1, ranges.size)
        assertEquals(Pair(0, 5), ranges[0])
    }

    @Test
    fun testParagraphRanges_singleChineseParagraph() {
        val ranges = computeParagraphRanges("你好世界")
        assertEquals(1, ranges.size)
        assertEquals(Pair(0, 4), ranges[0])
    }

    // ── 两段落 ──

    @Test
    fun testParagraphRanges_twoParagraphs() {
        val ranges = computeParagraphRanges("hello\nworld")
        assertEquals(2, ranges.size)
        assertEquals(Pair(0, 5), ranges[0])   // "hello"
        assertEquals(Pair(6, 11), ranges[1])   // "world"
    }

    @Test
    fun testParagraphRanges_twoChineseParagraphs() {
        val ranges = computeParagraphRanges("你好\n世界")
        assertEquals(2, ranges.size)
        assertEquals(Pair(0, 2), ranges[0])   // "你好"
        assertEquals(Pair(3, 5), ranges[1])   // "世界"
    }

    // ── 空段落 ──

    @Test
    fun testParagraphRanges_emptyParagraph_betweenTwoParagraphs() {
        val ranges = computeParagraphRanges("hello\n\nworld")
        assertEquals(3, ranges.size)
        assertEquals(Pair(0, 5), ranges[0])   // "hello"
        assertEquals(Pair(6, 6), ranges[1])   // "" (空段落)
        assertEquals(Pair(7, 12), ranges[2])   // "world"
    }

    @Test
    fun testParagraphRanges_emptyParagraph_atEnd() {
        val ranges = computeParagraphRanges("hello\n")
        assertEquals(2, ranges.size)
        assertEquals(Pair(0, 5), ranges[0])   // "hello"
        assertEquals(Pair(6, 6), ranges[1])   // "" (末尾空段落)
    }

    @Test
    fun testParagraphRanges_multipleEmptyParagraphs() {
        val ranges = computeParagraphRanges("a\n\n\nb")
        assertEquals(4, ranges.size)
        assertEquals(Pair(0, 1), ranges[0])   // "a"
        assertEquals(Pair(2, 2), ranges[1])   // "" (空段落)
        assertEquals(Pair(3, 3), ranges[2])   // "" (空段落)
        assertEquals(Pair(4, 5), ranges[3])   // "b"
    }

    // ── 连续空行 ──

    @Test
    fun testParagraphRanges_consecutiveEmptyLines_preserved() {
        val ranges = computeParagraphRanges("第一段\n\n\n\n第二段")
        assertEquals(5, ranges.size)
        assertEquals(Pair(0, 3), ranges[0])   // "第一段"
        assertEquals(Pair(4, 4), ranges[1])   // "" (空行1)
        assertEquals(Pair(5, 5), ranges[2])   // "" (空行2)
        assertEquals(Pair(6, 6), ranges[3])   // "" (空行3)
        assertEquals(Pair(7, 10), ranges[4])  // "第二段"
    }

    // ── 边界情况 ──

    @Test
    fun testParagraphRanges_emptyString() {
        val ranges = computeParagraphRanges("")
        assertEquals(1, ranges.size)
        assertEquals(Pair(0, 0), ranges[0])   // 空文本也是一个段落
    }

    @Test
    fun testParagraphRanges_onlyNewlines() {
        val ranges = computeParagraphRanges("\n\n")
        assertEquals(3, ranges.size)
        assertEquals(Pair(0, 0), ranges[0])   // "" (第一个 \n 前)
        assertEquals(Pair(1, 1), ranges[1])   // "" (两个 \n 之间)
        assertEquals(Pair(2, 2), ranges[2])   // "" (第二个 \n 后)
    }

    @Test
    fun testParagraphRanges_singleNewline() {
        val ranges = computeParagraphRanges("\n")
        assertEquals(2, ranges.size)
        assertEquals(Pair(0, 0), ranges[0])   // "" (空段落)
        assertEquals(Pair(1, 1), ranges[1])   // "" (末尾空段落)
    }

    // ── 每个段落都应独立应用缩进 ──
    // 验证：每个段落的 range 都会被设置一个 LeadingMarginSpan.Standard(indentPx, 0)
    // 这意味着每段的首行都会有 indentPx 的缩进

    @Test
    fun testIndentAppliedToAllParagraphs_includingEmpty() {
        val text = "第一段\n\n第二段"
        val ranges = computeParagraphRanges(text)
        // 每个段落都会有一个 span，包括空段落
        assertEquals(3, ranges.size)
        // 空段落的 start == end，但仍应用缩进
        assertEquals(Pair(4, 4), ranges[1])
    }

    @Test
    fun testIndentAppliedToTrailingEmptyParagraph() {
        val text = "hello\n"
        val ranges = computeParagraphRanges(text)
        assertEquals(2, ranges.size)
        // 末尾空段落也应用缩进
        assertEquals(Pair(6, 6), ranges[1])
    }

    // ── 段落范围不重叠 ──

    @Test
    fun testParagraphRanges_noOverlap() {
        val text = "aaa\nbbb\nccc"
        val ranges = computeParagraphRanges(text)
        for (i in 0 until ranges.size - 1) {
            // 下一个段落的 start > 当前段落的 end（因为 \n 占一个字符）
            assertTrue(ranges[i + 1].first > ranges[i].second)
        }
    }

    @Test
    fun testParagraphRanges_coversEntireText() {
        val text = "hello\nworld\nfoo"
        val ranges = computeParagraphRanges(text)
        // 第一个段落从 0 开始
        assertEquals(0, ranges.first().first)
        // 最后一个段落覆盖到文本末尾
        assertEquals(text.length, ranges.last().second)
    }
}
