package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

/**
 * SujianEditorRenderer 可视行范围计算测试
 *
 * 验证 drawStaticText 的可视行裁剪逻辑：
 * - 无 excludeRange 时只绘制可视行（快速路径）
 * - 有 excludeRange 时只对可视行做拆分
 * - 不相交的可视行批量 clipRect + layout.draw()
 *
 * 注意：由于 Layout 是 Android framework 类，单元测试中无法直接使用。
 * 这里测试的是可视行范围计算的纯逻辑部分，不依赖 Android framework。
 */
class SujianEditorRendererTest {

    // ── 可视行范围计算逻辑 ──
    // 以下方法与 SujianEditorRenderer.drawStaticText 中的逻辑一致

    /**
     * 模拟 Layout.getLineForVertical 的行为
     * 假设每行高度为 lineHeight，第一行 top = 0
     */
    private fun getLineForVertical(vertical: Int, lineHeight: Int, totalLines: Int): Int {
        if (vertical <= 0) return 0
        val line = vertical / lineHeight
        return line.coerceAtMost(totalLines - 1)
    }

    /**
     * 计算可视行范围
     */
    private fun computeVisibleLineRange(
        scrollY: Int,
        viewportHeight: Int,
        lineHeight: Int,
        totalLines: Int
    ): IntRange {
        val firstVisLine = getLineForVertical(scrollY, lineHeight, totalLines)
        val lastVisLine = getLineForVertical(scrollY + viewportHeight, lineHeight, totalLines)
            .coerceAtMost(totalLines - 1)
        return IntRange(firstVisLine, lastVisLine)
    }

    // ── 短文本全部可视 ──

    @Test
    fun testVisibleLineRange_shortText_allVisible() {
        // 5 行文本，每行 20px 高，viewport 200px，scrollY=0
        val range = computeVisibleLineRange(
            scrollY = 0,
            viewportHeight = 200,
            lineHeight = 20,
            totalLines = 5
        )
        // 200/20 = 10 行可视，但总共只有 5 行
        assertEquals(IntRange(0, 4), range)
    }

    @Test
    fun testVisibleLineRange_shortText_scrolledToBottom() {
        // 5 行文本，每行 20px 高，viewport 200px，scrollY=80（第5行顶部=80）
        val range = computeVisibleLineRange(
            scrollY = 80,
            viewportHeight = 200,
            lineHeight = 20,
            totalLines = 5
        )
        // firstVisLine = 80/20 = 4, lastVisLine = 280/20 = 14 → coerceAtMost(4) = 4
        assertEquals(IntRange(4, 4), range)
    }

    // ── 长文本只绘制可视行 ──

    @Test
    fun testVisibleLineRange_longText_onlyVisibleLines() {
        // 100 行文本，每行 20px 高，viewport 200px，scrollY=0
        val range = computeVisibleLineRange(
            scrollY = 0,
            viewportHeight = 200,
            lineHeight = 20,
            totalLines = 100
        )
        // firstVisLine = 0, lastVisLine = 200/20 = 10
        assertEquals(IntRange(0, 10), range)
    }

    @Test
    fun testVisibleLineRange_longText_scrolledMiddle() {
        // 100 行文本，每行 20px 高，viewport 200px，scrollY=1000（中间位置）
        val range = computeVisibleLineRange(
            scrollY = 1000,
            viewportHeight = 200,
            lineHeight = 20,
            totalLines = 100
        )
        // firstVisLine = 1000/20 = 50, lastVisLine = 1200/20 = 60
        assertEquals(IntRange(50, 60), range)
    }

    @Test
    fun testVisibleLineRange_longText_scrolledNearEnd() {
        // 100 行文本，每行 20px 高，viewport 200px，scrollY=1800
        val range = computeVisibleLineRange(
            scrollY = 1800,
            viewportHeight = 200,
            lineHeight = 20,
            totalLines = 100
        )
        // firstVisLine = 1800/20 = 90, lastVisLine = 2000/20 = 100 → coerceAtMost(99) = 99
        assertEquals(IntRange(90, 99), range)
    }

    @Test
    fun testVisibleLineRange_singleLine() {
        // 1 行文本
        val range = computeVisibleLineRange(
            scrollY = 0,
            viewportHeight = 200,
            lineHeight = 20,
            totalLines = 1
        )
        assertEquals(IntRange(0, 0), range)
    }

    @Test
    fun testVisibleLineRange_emptyViewport() {
        // viewportHeight = 0 时，至少显示一行
        val range = computeVisibleLineRange(
            scrollY = 0,
            viewportHeight = 0,
            lineHeight = 20,
            totalLines = 100
        )
        // scrollY + 0 = 0, getLineForVertical(0) = 0
        assertEquals(IntRange(0, 0), range)
    }

    // ── 无 excludeRange 快速路径逻辑 ──
    // 验证：无 excludeRange 时，只遍历可视行，不遍历全文

    @Test
    fun testNoExcludeRange_fastPath_onlyIteratesVisibleLines() {
        val totalLines = 100
        val visibleRange = computeVisibleLineRange(
            scrollY = 500,
            viewportHeight = 200,
            lineHeight = 20,
            totalLines = totalLines
        )

        // 快速路径应该只遍历 [25, 35] 的行
        val linesDrawn = mutableListOf<Int>()
        for (lineIdx in visibleRange.first..visibleRange.last) {
            linesDrawn.add(lineIdx)
        }

        assertEquals(11, linesDrawn.size) // 35 - 25 + 1 = 11
        assertEquals(25, linesDrawn.first())
        assertEquals(35, linesDrawn.last())
    }

    // ── 有 excludeRange 时只对可视行做拆分 ──

    @Test
    fun testExcludeRange_onlyVisibleLinesChecked() {
        val totalLines = 100
        val lineHeight = 20
        val scrollY = 500
        val viewportHeight = 200
        val excludeRange = IntRange(10, 15) // 在可视范围之外

        val visibleRange = computeVisibleLineRange(scrollY, viewportHeight, lineHeight, totalLines)

        // 模拟每行的 start/end offset
        // 假设每行 10 个字符
        val linesNeedSplit = mutableListOf<Int>()
        val linesFullDraw = mutableListOf<Int>()

        for (lineIdx in visibleRange.first..visibleRange.last) {
            val lineStart = lineIdx * 10
            val lineEnd = lineStart + 10

            if (lineEnd <= excludeRange.first || lineStart >= excludeRange.last) {
                linesFullDraw.add(lineIdx)
            } else {
                linesNeedSplit.add(lineIdx)
            }
        }

        // excludeRange [10, 15) 对应行 1-1（字符 10-15），不在可视范围 [25, 35] 内
        // 所以所有可视行都应该走批量绘制（nonOverlapLineRanges）
        assertEquals(0, linesNeedSplit.size)
        // 所有 11 行都不与 excludeRange 相交，形成 1 个连续区间
        assertEquals(11, linesFullDraw.size)
    }

    @Test
    fun testExcludeRange_visibleLinesSplit() {
        val totalLines = 100
        val lineHeight = 20
        val scrollY = 500
        val viewportHeight = 200
        // excludeRange 覆盖可视范围中间的某些行
        // 可视行 25-35，每行 10 字符 → 字符范围 250-360
        val excludeRange = IntRange(270, 280) // 影响行 27

        val visibleRange = computeVisibleLineRange(scrollY, viewportHeight, lineHeight, totalLines)

        val linesNeedSplit = mutableListOf<Int>()
        val linesFullDraw = mutableListOf<Int>()

        for (lineIdx in visibleRange.first..visibleRange.last) {
            val lineStart = lineIdx * 10
            val lineEnd = lineStart + 10

            if (lineEnd <= excludeRange.first || lineStart >= excludeRange.last) {
                linesFullDraw.add(lineIdx)
            } else {
                linesNeedSplit.add(lineIdx)
            }
        }

        // 行 27: 字符 [270, 280) 与 exclude [270, 280) 完全重叠
        assertEquals(1, linesNeedSplit.size)
        assertEquals(27, linesNeedSplit[0])
        assertEquals(10, linesFullDraw.size) // 11 - 1 = 10
    }

    @Test
    fun testExcludeRange_partialOverlap_visibleLinesSplit() {
        val totalLines = 100
        val lineHeight = 20
        val scrollY = 500
        val viewportHeight = 200
        // excludeRange 跨越多行
        val excludeRange = IntRange(265, 295) // 影响行 26, 27, 28, 29

        val visibleRange = computeVisibleLineRange(scrollY, viewportHeight, lineHeight, totalLines)

        val linesNeedSplit = mutableListOf<Int>()
        val linesFullDraw = mutableListOf<Int>()

        for (lineIdx in visibleRange.first..visibleRange.last) {
            val lineStart = lineIdx * 10
            val lineEnd = lineStart + 10

            if (lineEnd <= excludeRange.first || lineStart >= excludeRange.last) {
                linesFullDraw.add(lineIdx)
            } else {
                linesNeedSplit.add(lineIdx)
            }
        }

        // 行 26: [260, 270) 与 [265, 295) 部分重叠
        // 行 27: [270, 280) 与 [265, 295) 完全包含
        // 行 28: [280, 290) 与 [265, 295) 完全包含
        // 行 29: [290, 300) 与 [265, 295) 部分重叠
        assertEquals(4, linesNeedSplit.size)
        assertTrue(linesNeedSplit.contains(26))
        assertTrue(linesNeedSplit.contains(27))
        assertTrue(linesNeedSplit.contains(28))
        assertTrue(linesNeedSplit.contains(29))
        assertEquals(7, linesFullDraw.size) // 11 - 4 = 7
    }

    // ── 不相交行批量绘制：连续区间合并 ──
    // 不相交行收集为连续区间，每个区间一次 clipRect + layout.draw()
    // 这里测试逻辑正确性：确保连续不相交行被正确合并

    @Test
    fun testNonOverlapLines_batchDrawn_continuousRanges() {
        val totalLines = 100
        val lineHeight = 20
        val scrollY = 500
        val viewportHeight = 200
        // excludeRange 覆盖可视范围中间的某些行
        // 可视行 25-35，每行 10 字符 → 字符范围 250-360
        val excludeRange = IntRange(270, 280) // 影响行 27

        val visibleRange = computeVisibleLineRange(scrollY, viewportHeight, lineHeight, totalLines)

        // 模拟 nonOverlapLineRanges 收集逻辑
        val nonOverlapLineRanges = mutableListOf<Pair<Int, Int>>()
        var rangeStart = -1
        for (lineIdx in visibleRange.first..visibleRange.last) {
            val lineStart = lineIdx * 10
            val lineEnd = lineStart + 10
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
            nonOverlapLineRanges.add(Pair(rangeStart, visibleRange.last))
        }

        // 行 27 与 excludeRange 相交，其余行不相交
        // 不相交行分为两个区间：[25, 26] 和 [28, 35]
        assertEquals(2, nonOverlapLineRanges.size)
        assertEquals(Pair(25, 26), nonOverlapLineRanges[0])
        assertEquals(Pair(28, 35), nonOverlapLineRanges[1])
    }

    @Test
    fun testNonOverlapLines_batchDrawn_allNonOverlap() {
        val totalLines = 100
        val lineHeight = 20
        val scrollY = 500
        val viewportHeight = 200
        val excludeRange = IntRange(10, 15) // 在可视范围之外

        val visibleRange = computeVisibleLineRange(scrollY, viewportHeight, lineHeight, totalLines)

        val nonOverlapLineRanges = mutableListOf<Pair<Int, Int>>()
        var rangeStart = -1
        for (lineIdx in visibleRange.first..visibleRange.last) {
            val lineStart = lineIdx * 10
            val lineEnd = lineStart + 10
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
            nonOverlapLineRanges.add(Pair(rangeStart, visibleRange.last))
        }

        // 所有可视行都不与 excludeRange 相交，形成 1 个连续区间 [25, 35]
        assertEquals(1, nonOverlapLineRanges.size)
        assertEquals(Pair(25, 35), nonOverlapLineRanges[0])
    }

    @Test
    fun testNonOverlapLines_batchDrawn_multipleGaps() {
        val totalLines = 100
        val lineHeight = 20
        val scrollY = 500
        val viewportHeight = 200
        // excludeRange 跨越多行，形成多个不相交区间
        val excludeRange = IntRange(265, 295) // 影响行 26, 27, 28, 29

        val visibleRange = computeVisibleLineRange(scrollY, viewportHeight, lineHeight, totalLines)

        val nonOverlapLineRanges = mutableListOf<Pair<Int, Int>>()
        var rangeStart = -1
        for (lineIdx in visibleRange.first..visibleRange.last) {
            val lineStart = lineIdx * 10
            val lineEnd = lineStart + 10
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
            nonOverlapLineRanges.add(Pair(rangeStart, visibleRange.last))
        }

        // 行 26-29 与 excludeRange 相交，不相交行分为两个区间：[25, 25] 和 [30, 35]
        assertEquals(2, nonOverlapLineRanges.size)
        assertEquals(Pair(25, 25), nonOverlapLineRanges[0])
        assertEquals(Pair(30, 35), nonOverlapLineRanges[1])
    }
}
