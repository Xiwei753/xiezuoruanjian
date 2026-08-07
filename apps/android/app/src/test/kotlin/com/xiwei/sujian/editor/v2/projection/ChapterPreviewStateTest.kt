package com.xiwei.sujian.editor.v2.projection

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #595 九：预览用纯静态 ChapterPreviewState 行为测试。
 *
 * 结构契约（字段不存在性、方法存在性）已移入
 * [com.xiwei.sujian.arch.ChapterPreviewStateArchitectureTest]；本文件只保留运行时行为：
 * - ChapterPreviewState 是不可变数据载体；
 * - PreviewStyle 默认值合理；
 * - TextRange 不可变。
 */
class ChapterPreviewStateTest {

    @Test
    fun chapterPreviewStateIsImmutable() {
        val state = ChapterPreviewState(
            text = "preview text",
            revision = 5L,
            selection = TextRange(0, 10),
            searchHighlights = listOf(TextRange(0, 5)),
        )
        assertEquals("preview text", state.text)
        assertEquals(5L, state.revision)
        assertEquals(TextRange(0, 10), state.selection)
        assertEquals(1, state.searchHighlights.size)
    }

    @Test
    fun previewStyleDefaultsAreSensible() {
        val style = PreviewStyle()
        assertEquals(16f, style.fontSizeSp, 0.01f)
        assertEquals(1.5f, style.lineSpacingMultiplier, 0.01f)
    }

    @Test
    fun textRangeIsImmutable() {
        val range = TextRange(3, 7)
        assertEquals(3, range.start)
        assertEquals(7, range.end)
    }
}
