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

    /**
     * #624 评论7 复审：覆盖旧后缀真实位移路径（suffixUtf16 > 0）。
     *
     * 现有 5 个迁移用例的 targetUtf16 都落在 replacement end（suffixUtf16 == 0）或
     * 旧前缀，从未进入"光标在替换区后的真实后缀"分支。该分支有独立的位移换算
     * `safeStart + replacementUtf8Len + (oldUtf8 - safeEnd)`，无覆盖即缺口。
     *
     * 场景：committed="abc"，替换 "b"（utf8 [1,2)）为 "你"（3 字节），newCursorPosition=2。
     * - replaceStartUtf16=1, replaceEndUtf16=2, replacementUtf16Len=1, replacementUtf8Len=3
     * - newReplaceEndUtf16=2, totalUtf16=3
     * - targetUtf16 = 2 + 2 - 1 = 3（> newReplaceEndUtf16，进入旧后缀，suffixUtf16=1）
     * - oldUtf16 = 2 + 1 = 3, oldUtf8 = index.utf16ToUtf8(3) = 3
     * - targetUtf8 = 1 + 3 + (3 - 2) = 5（"a" 1 + "你" 3 + "c" 1）
     */
    @Test
    fun computeResultingSelectionUtf8_cursorInOldSuffixAfterReplace() {
        val committedText = "abc"
        val index = TextOffsetIndex().apply { rebuildFromText(committedText) }
        val (anchor, head) =
            InputCursorMapper.computeResultingSelectionUtf8(
                index,
                1,
                2,
                "你",
                2,
            )
        assertEquals(Pair(5, 5), Pair(anchor, head))
    }

    /**
     * #624 评论7 复审：旧后缀位移 + 多字节后缀 — 替换区后是中文，验证 UTF-8 长度差
     * 换算在多字节后缀下正确。
     *
     * 场景：committed="a你b"（utf8: a=1, 你=3, b=1，共 5），替换 "a"（[0,1)）为 "世"（3 字节），
     * newCursorPosition=2 → target 落在 "b" 上。
     * - replaceStartUtf16=0, replaceEndUtf16=1, replacementUtf16Len=1, replacementUtf8Len=3
     * - newReplaceEndUtf16=1, totalUtf16=3
     * - targetUtf16 = 1 + 2 - 1 = 2（"你" 后，"b" 前）
     * - 2 > newReplaceEndUtf16=1，旧后缀：suffixUtf16=1, oldUtf16=1+1=2, oldUtf8=index.utf16ToUtf8(2)=4
     * - targetUtf8 = 0 + 3 + (4 - 1) = 6（"世" 3 + "你" 3 = 6，"b" 前）
     */
    @Test
    fun computeResultingSelectionUtf8_cursorInMultibyteSuffixAfterReplace() {
        val committedText = "a你b"
        val index = TextOffsetIndex().apply { rebuildFromText(committedText) }
        val (anchor, head) =
            InputCursorMapper.computeResultingSelectionUtf8(
                index,
                0,
                1,
                "世",
                2,
            )
        assertEquals(Pair(6, 6), Pair(anchor, head))
    }
}
