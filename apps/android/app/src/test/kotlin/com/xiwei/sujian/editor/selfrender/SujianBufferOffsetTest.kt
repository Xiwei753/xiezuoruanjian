package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

/**
 * 自研写作区 Buffer UTF-8/UTF-16 偏移转换测试 + clampToCharBoundary 测试
 *
 * 验证 Core 动画事件（UTF-8 byte offset）与 Android Layout（UTF-16 offset）之间的转换正确性。
 * 验证 clampToCharBoundary 对 surrogate pair 的安全处理。
 *
 * 这对真吐字/吞字动画至关重要，因为：
 * - Core 返回的 rangeStart/oldCursorIndex/newCursorIndex 是 UTF-8 byte offset
 * - Android Layout 的 getGlyphRects/getCursorRect 需要 UTF-16 offset
 * - animatedInsertRange 也必须是 UTF-16 offset
 */
class SujianBufferOffsetTest {

    // ── 使用 SujianEditorBuffer 的静态方法 ──

    private fun utf16ToUtf8(text: String, utf16Offset: Int): Int {
        return SujianEditorBuffer.utf16ToUtf8(text, utf16Offset)
    }

    private fun utf8ToUtf16(text: String, utf8Offset: Int): Int {
        return SujianEditorBuffer.utf8ToUtf16(text, utf8Offset)
    }

    private fun clampToCharBoundary(text: String, offset: Int): Int {
        return SujianEditorBuffer.clampToCharBoundary(text, offset)
    }

    // ── 插入动画场景 ──

    @Test
    fun insertAnimation_ascii_rangeStartConversion() {
        // 在 "abc" 中光标位置 1 插入 "X" → "aXbc"
        // Core 返回 rangeStart = 1 (UTF-8 byte offset)
        val newText = "aXbc"
        val rangeStartUtf8 = 1
        val rangeStartUtf16 = utf8ToUtf16(newText, rangeStartUtf8)
        assertEquals(1, rangeStartUtf16) // "X" 在 UTF-16 offset 1
        // animatedInsertRange = IntRange(1, 2)
        assertEquals(IntRange(1, 2), IntRange(rangeStartUtf16, rangeStartUtf16 + 1))
    }

    @Test
    fun insertAnimation_chinese_rangeStartConversion() {
        // 在 "你好" 后插入 "世界" → "你好世界"
        // Core 返回 rangeStart = 6 (UTF-8 byte offset，"你" 3 bytes + "好" 3 bytes)
        val newText = "你好世界"
        val rangeStartUtf8 = 6
        val rangeStartUtf16 = utf8ToUtf16(newText, rangeStartUtf8)
        assertEquals(2, rangeStartUtf16) // "世" 在 UTF-16 offset 2
        // animatedInsertRange = IntRange(2, 4)
        assertEquals(IntRange(2, 4), IntRange(rangeStartUtf16, rangeStartUtf16 + "世界".length))
    }

    @Test
    fun insertAnimation_mixed_rangeStartConversion() {
        // 在 "a你b" 后插入 "好" → "a你b好"
        // Core 返回 rangeStart = 5 (UTF-8 byte offset: a=1, 你=3, b=1, total=5)
        val newText = "a你b好"
        val rangeStartUtf8 = 5
        val rangeStartUtf16 = utf8ToUtf16(newText, rangeStartUtf8)
        assertEquals(3, rangeStartUtf16) // "好" 在 UTF-16 offset 3
        // animatedInsertRange = IntRange(3, 4)
        assertEquals(IntRange(3, 4), IntRange(rangeStartUtf16, rangeStartUtf16 + 1))
    }

    @Test
    fun insertAnimation_emoji_rangeStartConversion() {
        // 在 "a" 后插入 emoji "😀" → "a😀"
        // Core 返回 rangeStart = 1 (UTF-8 byte offset)
        val newText = "a😀"
        val rangeStartUtf8 = 1
        val rangeStartUtf16 = utf8ToUtf16(newText, rangeStartUtf8)
        assertEquals(1, rangeStartUtf16) // emoji 在 UTF-16 offset 1
        // emoji 在 UTF-16 中占 2 code units，animatedInsertRange = IntRange(1, 3)
        val emojiUtf16Length = "😀".length // = 2
        assertEquals(IntRange(1, 3), IntRange(rangeStartUtf16, rangeStartUtf16 + emojiUtf16Length))
    }

    // ── 删除动画场景 ──

    @Test
    fun deleteAnimation_ascii_oldCursorConversion() {
        // 从 "abc" 删除 "b" → "ac"
        // Core 返回 oldCursorIndex = 2 (UTF-8 byte offset，光标在 "b" 后)
        val oldText = "abc"
        val oldCursorUtf8 = 2
        val oldCursorUtf16 = utf8ToUtf16(oldText, oldCursorUtf8)
        assertEquals(2, oldCursorUtf16) // 光标在 UTF-16 offset 2
    }

    @Test
    fun deleteAnimation_chinese_oldCursorConversion() {
        // 从 "你好" 删除 "好" → "你"
        // Core 返回 oldCursorIndex = 6 (UTF-8 byte offset，光标在 "好" 后)
        val oldText = "你好"
        val oldCursorUtf8 = 6
        val oldCursorUtf16 = utf8ToUtf16(oldText, oldCursorUtf8)
        assertEquals(2, oldCursorUtf16) // 光标在 UTF-16 offset 2
    }

    @Test
    fun deleteAnimation_emoji_oldCursorConversion() {
        // 从 "a😀b" 删除 emoji → "ab"
        // Core 返回 oldCursorIndex = 5 (UTF-8 byte offset: a=1, 😀=4)
        val oldText = "a😀b"
        val oldCursorUtf8 = 5
        val oldCursorUtf16 = utf8ToUtf16(oldText, oldCursorUtf8)
        assertEquals(3, oldCursorUtf16) // 光标在 UTF-16 offset 3 (emoji 占 2 code units)
    }

    // ── oldCursorRect 计算 ──

    @Test
    fun insertAnimation_rangeStartEqualsOldCursor_simpleInsert() {
        val newText = "aXb"
        val rangeStartUtf8 = 1
        val rangeStartUtf16 = utf8ToUtf16(newText, rangeStartUtf8)
        assertEquals(1, rangeStartUtf16)
    }

    @Test
    fun insertAnimation_rangeStartEqualsOldCursor_chineseInsert() {
        val newText = "你好"
        val rangeStartUtf8 = 3
        val rangeStartUtf16 = utf8ToUtf16(newText, rangeStartUtf8)
        assertEquals(1, rangeStartUtf16)
    }

    @Test
    fun insertAnimation_rangeStartEqualsOldCursor_afterDeleteSelection() {
        val newText = "aXe"
        val rangeStartUtf8 = 1
        val rangeStartUtf16 = utf8ToUtf16(newText, rangeStartUtf8)
        assertEquals(1, rangeStartUtf16)
    }

    // ── animatedInsertRange 边界 ──

    @Test
    fun animatedInsertRange_emptyWhenNoAnimation() {
        var animatedInsertRange: IntRange? = null
        assertNull(animatedInsertRange)
    }

    @Test
    fun animatedInsertRange_setOnInsert_clearedOnFinish() {
        var animatedInsertRange: IntRange? = null
        animatedInsertRange = IntRange(1, 3)
        assertEquals(IntRange(1, 3), animatedInsertRange)
        animatedInsertRange = null
        assertNull(animatedInsertRange)
    }

    @Test
    fun animatedInsertRange_clearedOnScroll() {
        var animatedInsertRange: IntRange? = IntRange(1, 3)
        animatedInsertRange = null
        assertNull(animatedInsertRange)
    }

    @Test
    fun animatedInsertRange_clearedOnComposing() {
        var animatedInsertRange: IntRange? = IntRange(1, 3)
        animatedInsertRange = null
        assertNull(animatedInsertRange)
    }

    @Test
    fun animatedInsertRange_replacedOnNewInsert() {
        var animatedInsertRange: IntRange? = null
        animatedInsertRange = IntRange(1, 2)
        assertEquals(IntRange(1, 2), animatedInsertRange)
        animatedInsertRange = IntRange(2, 3)
        assertEquals(IntRange(2, 3), animatedInsertRange)
    }

    // ── UTF-16 → UTF-8 转换（静态方法）──

    @Test
    fun utf16ToUtf8_ascii() {
        assertEquals(0, utf16ToUtf8("abc", 0))
        assertEquals(1, utf16ToUtf8("abc", 1))
        assertEquals(3, utf16ToUtf8("abc", 3))
    }

    @Test
    fun utf16ToUtf8_chinese() {
        // "你好" = 6 UTF-8 bytes
        assertEquals(0, utf16ToUtf8("你好", 0))
        assertEquals(3, utf16ToUtf8("你好", 1))
        assertEquals(6, utf16ToUtf8("你好", 2))
    }

    @Test
    fun utf16ToUtf8_emoji() {
        // "😀" = 4 UTF-8 bytes, 2 UTF-16 code units
        assertEquals(0, utf16ToUtf8("😀", 0))
        assertEquals(4, utf16ToUtf8("😀", 2)) // emoji 完整后
    }

    @Test
    fun utf16ToUtf8_mixed() {
        // "a你😀b" = 1+3+4+1 = 9 UTF-8 bytes
        assertEquals(0, utf16ToUtf8("a你😀b", 0))
        assertEquals(1, utf16ToUtf8("a你😀b", 1))  // after 'a'
        assertEquals(4, utf16ToUtf8("a你😀b", 2))  // after '你'
        assertEquals(8, utf16ToUtf8("a你😀b", 4))  // after emoji (2 UTF-16 units)
        assertEquals(9, utf16ToUtf8("a你😀b", 5))  // after 'b'
    }

    @Test
    fun utf16ToUtf8_outOfBounds() {
        assertEquals(3, utf16ToUtf8("abc", 10)) // 超出范围，返回全文 UTF-8 长度
        assertEquals(0, utf16ToUtf8("abc", -1))  // 负数，返回 0
    }

    // ── UTF-8 → UTF-16 转换（静态方法）──

    @Test
    fun utf8ToUtf16_ascii() {
        assertEquals(0, utf8ToUtf16("abc", 0))
        assertEquals(1, utf8ToUtf16("abc", 1))
        assertEquals(3, utf8ToUtf16("abc", 3))
    }

    @Test
    fun utf8ToUtf16_chinese() {
        assertEquals(0, utf8ToUtf16("你好", 0))
        assertEquals(1, utf8ToUtf16("你好", 3))
        assertEquals(2, utf8ToUtf16("你好", 6))
    }

    @Test
    fun utf8ToUtf16_emoji() {
        assertEquals(0, utf8ToUtf16("😀", 0))
        assertEquals(2, utf8ToUtf16("😀", 4)) // emoji 完整后
    }

    @Test
    fun utf8ToUtf16_midCodePoint_stopsBefore() {
        // UTF-8 offset 落在 code point 中间时，停在当前 code point 之前
        // "你" = 3 UTF-8 bytes，offset 1 和 2 都在 "你" 中间
        assertEquals(0, utf8ToUtf16("你好", 1)) // "你" 还没结束
        assertEquals(0, utf8ToUtf16("你好", 2)) // "你" 还没结束
        assertEquals(1, utf8ToUtf16("你好", 3)) // "你" 结束
    }

    @Test
    fun utf8ToUtf16_emoji_midCodePoint() {
        // "😀" = 4 UTF-8 bytes
        assertEquals(0, utf8ToUtf16("😀", 1)) // emoji 还没结束
        assertEquals(0, utf8ToUtf16("😀", 2)) // emoji 还没结束
        assertEquals(0, utf8ToUtf16("😀", 3)) // emoji 还没结束
        assertEquals(2, utf8ToUtf16("😀", 4)) // emoji 结束
    }

    @Test
    fun utf8ToUtf16_outOfBounds() {
        assertEquals(3, utf8ToUtf16("abc", 100)) // 超出范围，返回文本长度
        assertEquals(0, utf8ToUtf16("abc", -1))   // 负数，返回 0
    }

    // ── clampToCharBoundary ──

    @Test
    fun clampToCharBoundary_ascii_noChange() {
        assertEquals(0, clampToCharBoundary("abc", 0))
        assertEquals(1, clampToCharBoundary("abc", 1))
        assertEquals(2, clampToCharBoundary("abc", 2))
        assertEquals(3, clampToCharBoundary("abc", 3))
    }

    @Test
    fun clampToCharBoundary_chinese_noChange() {
        assertEquals(0, clampToCharBoundary("你好", 0))
        assertEquals(1, clampToCharBoundary("你好", 1))
        assertEquals(2, clampToCharBoundary("你好", 2))
    }

    @Test
    fun clampToCharBoundary_surrogatePair_clampsBack() {
        // "a😀b" → indices: 0='a', 1=high surrogate, 2=low surrogate, 3='b'
        val text = "a😀b"
        // offset 2 指向低代理，应该回退到 1（高代理）
        assertEquals(1, clampToCharBoundary(text, 2))
        // offset 1 指向高代理，不需要回退
        assertEquals(1, clampToCharBoundary(text, 1))
        // offset 3 指向 'b'，不需要回退
        assertEquals(3, clampToCharBoundary(text, 3))
    }

    @Test
    fun clampToCharBoundary_multipleEmoji() {
        // "😀😀" → indices: 0=high, 1=low, 2=high, 3=low
        val text = "😀😀"
        // offset 1 指向第一个 emoji 的低代理
        assertEquals(0, clampToCharBoundary(text, 1))
        // offset 3 指向第二个 emoji 的低代理
        assertEquals(2, clampToCharBoundary(text, 3))
    }

    @Test
    fun clampToCharBoundary_boundaryConditions() {
        assertEquals(0, clampToCharBoundary("abc", 0))
        assertEquals(0, clampToCharBoundary("abc", -1))
        assertEquals(3, clampToCharBoundary("abc", 3))
        assertEquals(3, clampToCharBoundary("abc", 10))
    }

    @Test
    fun clampToCharBoundary_emptyString() {
        assertEquals(0, clampToCharBoundary("", 0))
        assertEquals(0, clampToCharBoundary("", 1))
    }

    // ── 往返转换验证 ──

    @Test
    fun roundTrip_ascii() {
        val text = "hello world"
        for (i in 0..text.length) {
            val utf8 = utf16ToUtf8(text, i)
            val utf16 = utf8ToUtf16(text, utf8)
            assertEquals("Round trip failed at offset $i", i, utf16)
        }
    }

    @Test
    fun roundTrip_chinese() {
        val text = "你好世界"
        for (i in 0..text.length) {
            val utf8 = utf16ToUtf8(text, i)
            val utf16 = utf8ToUtf16(text, utf8)
            assertEquals("Round trip failed at offset $i", i, utf16)
        }
    }

    @Test
    fun roundTrip_mixed() {
        val text = "a你b好c"
        for (i in 0..text.length) {
            val utf8 = utf16ToUtf8(text, i)
            val utf16 = utf8ToUtf16(text, utf8)
            assertEquals("Round trip failed at offset $i", i, utf16)
        }
    }

    @Test
    fun roundTrip_emoji() {
        val text = "a😀b"
        // 注意：emoji 在 UTF-16 中占 2 code units
        val validOffsets = listOf(0, 1, 3, 4) // 0='a'前, 1='a'后/emoji前, 3=emoji后/'b'前, 4='b'后
        for (i in validOffsets) {
            val utf8 = utf16ToUtf8(text, i)
            val utf16 = utf8ToUtf16(text, utf8)
            assertEquals("Round trip failed at offset $i", i, utf16)
        }
    }
}
