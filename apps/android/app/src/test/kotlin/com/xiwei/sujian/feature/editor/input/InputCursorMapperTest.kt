package com.xiwei.sujian.feature.editor.input

import com.xiwei.sujian.feature.editor.projection.TextOffsetIndex
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #624 评论7：[InputCursorMapper] 单元测试。
 *
 * 从旧正文偏移索引迁移 5 个 computeResultingSelectionUtf8 用例，改用
 * [InputCursorMapper] + [TextOffsetIndex]。断言与旧实现一致 — 验证偏移映射语义
 * 在迁移到增量索引后保持不变。
 */
class InputCursorMapperTest {
    @Test
    fun computeResultingSelectionUtf8_insertChinese_position1() {
        val committedText = "abc"
        val index = TextOffsetIndex().apply { rebuildFromText(committedText) }
        val (anchor, head) =
            InputCursorMapper.computeResultingSelectionUtf8(
                index,
                1,
                1,
                "你",
                1,
            )
        assertEquals(Pair(4, 4), Pair(anchor, head))
    }

    @Test
    fun computeResultingSelectionUtf8_insertChinese_position0() {
        val committedText = "abc"
        val index = TextOffsetIndex().apply { rebuildFromText(committedText) }
        val (anchor, head) =
            InputCursorMapper.computeResultingSelectionUtf8(
                index,
                1,
                1,
                "你",
                0,
            )
        assertEquals(Pair(1, 1), Pair(anchor, head))
    }

    @Test
    fun computeResultingSelectionUtf8_replaceWithEmoji_position1() {
        val committedText = "abc"
        val index = TextOffsetIndex().apply { rebuildFromText(committedText) }
        val (anchor, head) =
            InputCursorMapper.computeResultingSelectionUtf8(
                index,
                0,
                1,
                "😀",
                1,
            )
        assertEquals(Pair(4, 4), Pair(anchor, head))
    }

    @Test
    fun computeResultingSelectionUtf8_negativePosition() {
        val committedText = "abc"
        val index = TextOffsetIndex().apply { rebuildFromText(committedText) }
        val (anchor, head) =
            InputCursorMapper.computeResultingSelectionUtf8(
                index,
                1,
                1,
                "你",
                -1,
            )
        assertEquals(Pair(0, 0), Pair(anchor, head))
    }

    @Test
    fun computeResultingSelectionUtf8_replaceRangeWithChinese() {
        val committedText = "a你好b"
        val index = TextOffsetIndex().apply { rebuildFromText(committedText) }
        val (anchor, head) =
            InputCursorMapper.computeResultingSelectionUtf8(
                index,
                1,
                7,
                "世界",
                1,
            )
        assertEquals(Pair(7, 7), Pair(anchor, head))
    }
}
