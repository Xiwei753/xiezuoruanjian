package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

/**
 * SujianInputConnection.deleteSurroundingTextInCodePoints 的 code point 转换逻辑测试
 *
 * 验证 code point 数量到 UTF-16 unit 数量的正确换算。
 * 这对 surrogate pair（如 emoji）至关重要：1 code point = 2 UTF-16 units。
 *
 * 注意：这里测试的是纯逻辑部分（code point → UTF-16 转换），
 * 不依赖 Android framework 类（View, BaseInputConnection 等）。
 */
class SujianInputConnectionCodePointTest {

    // ── Code point → UTF-16 转换逻辑 ──
    // 以下方法与 SujianInputConnection.deleteSurroundingTextInCodePoints 中的逻辑一致

    /**
     * 将光标前的 code point 数量转换为 UTF-16 unit 数量
     * 向左扫描，正确处理 surrogate pair
     */
    private fun codePointsToUtf16Before(text: String, cursorPos: Int, beforeLength: Int): Int {
        var offset = cursorPos
        var cpCount = 0
        while (cpCount < beforeLength && offset > 0) {
            offset -= 1
            if (offset > 0 && Character.isSurrogatePair(text[offset - 1], text[offset])) {
                offset -= 1
            }
            cpCount++
        }
        return cursorPos - offset
    }

    /**
     * 将光标后的 code point 数量转换为 UTF-16 unit 数量
     * 向右扫描，正确处理 surrogate pair
     */
    private fun codePointsToUtf16After(text: String, cursorPos: Int, afterLength: Int): Int {
        var offset = cursorPos
        var cpCount = 0
        while (cpCount < afterLength && offset < text.length) {
            val ch = text[offset]
            if (Character.isHighSurrogate(ch) && offset + 1 < text.length && Character.isLowSurrogate(text[offset + 1])) {
                offset += 2
            } else {
                offset += 1
            }
            cpCount++
        }
        return offset - cursorPos
    }

    // ── ASCII 字符删除 ──

    @Test
    fun testDeleteSurroundingTextInCodePoints_simpleAscii_before() {
        // "abcde"，光标在 3（'d' 前），删除 2 个 code point
        val text = "abcde"
        val cursorPos = 3
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 2)
        // ASCII: 1 code point = 1 UTF-16 unit
        assertEquals(2, utf16Before)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_simpleAscii_after() {
        // "abcde"，光标在 2（'c' 前），删除后方 2 个 code point
        val text = "abcde"
        val cursorPos = 2
        val utf16After = codePointsToUtf16After(text, cursorPos, 2)
        assertEquals(2, utf16After)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_simpleAscii_beforeAndAfter() {
        // "abcde"，光标在 2，删除前 1 后 1
        val text = "abcde"
        val cursorPos = 2
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 1)
        val utf16After = codePointsToUtf16After(text, cursorPos, 1)
        assertEquals(1, utf16Before)
        assertEquals(1, utf16After)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_simpleAscii_deleteAllBefore() {
        // "abc"，光标在 3，删除前 3 个 code point
        val text = "abc"
        val cursorPos = 3
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 3)
        assertEquals(3, utf16Before)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_simpleAscii_deleteMoreThanAvailable() {
        // "ab"，光标在 2，删除前 5 个 code point（只有 2 个可用）
        val text = "ab"
        val cursorPos = 2
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 5)
        assertEquals(2, utf16Before) // 只能删 2 个
    }

    // ── Surrogate pair（emoji）删除 ──

    @Test
    fun testDeleteSurroundingTextInCodePoints_surrogatePair_before() {
        // "a😀b"，光标在 3（'b' 前，emoji 占 2 UTF-16 units），删除前 1 个 code point
        val text = "a😀b"
        // text.length = 4: a(1) + 😀(2) + b(1)
        val cursorPos = 3 // 'b' 前
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 1)
        // 1 code point = 😀 = 2 UTF-16 units
        assertEquals(2, utf16Before)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_surrogatePair_after() {
        // "a😀b"，光标在 1（emoji 前），删除后 1 个 code point
        val text = "a😀b"
        val cursorPos = 1
        val utf16After = codePointsToUtf16After(text, cursorPos, 1)
        // 1 code point = 😀 = 2 UTF-16 units
        assertEquals(2, utf16After)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_surrogatePair_deleteTwoEmojiBefore() {
        // "😀😀a"，光标在 4（'a' 前），删除前 2 个 code point
        val text = "😀😀a"
        // text.length = 5: 😀(2) + 😀(2) + a(1)
        val cursorPos = 4
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 2)
        // 2 code points = 2 emoji = 4 UTF-16 units
        assertEquals(4, utf16Before)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_surrogatePair_deleteTwoEmojiAfter() {
        // "a😀😀"，光标在 1，删除后 2 个 code point
        val text = "a😀😀"
        val cursorPos = 1
        val utf16After = codePointsToUtf16After(text, cursorPos, 2)
        // 2 code points = 2 emoji = 4 UTF-16 units
        assertEquals(4, utf16After)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_surrogatePair_deleteMoreThanAvailable() {
        // "😀"，光标在 2（emoji 后），删除前 3 个 code point（只有 1 个可用）
        val text = "😀"
        val cursorPos = 2
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 3)
        assertEquals(2, utf16Before) // 只能删 1 个 emoji = 2 UTF-16 units
    }

    // ── 混合 ASCII 和 surrogate pair ──

    @Test
    fun testDeleteSurroundingTextInCodePoints_mixed_asciiThenEmoji() {
        // "ab😀c"，光标在 4（'c' 前），删除前 2 个 code point
        val text = "ab😀c"
        // text.length = 5: a(1) + b(1) + 😀(2) + c(1)
        val cursorPos = 4
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 2)
        // 2 code points: 'b'(1) + '😀'(2) = 3 UTF-16 units
        assertEquals(3, utf16Before)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_mixed_emojiThenAscii() {
        // "a😀bc"，光标在 4（'c' 前），删除前 2 个 code point
        val text = "a😀bc"
        // indices: 0='a', 1='😀' high, 2='😀' low, 3='b', 4='c'
        val cursorPos = 4
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 2)
        // 往前数 2 code points: 'b'(1 UTF-16 unit) + '😀'(2 UTF-16 units) = 3 UTF-16 units
        assertEquals(3, utf16Before)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_mixed_beforeAndAfter() {
        // "a😀b😀c"，光标在 3（第一个 emoji 后的 'b' 前）
        val text = "a😀b😀c"
        // indices: 0='a', 1-2='😀', 3='b', 4-5='😀', 6='c'
        val cursorPos = 3
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 1)
        val utf16After = codePointsToUtf16After(text, cursorPos, 1)
        // 前 1 code point = '😀' = 2 UTF-16 units
        assertEquals(2, utf16Before)
        // 后 1 code point = 'b' = 1 UTF-16 unit
        assertEquals(1, utf16After)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_mixed_threeCodePointsBefore() {
        // "ab😀cd"，光标在 5（'d' 前），删除前 3 个 code point
        val text = "ab😀cd"
        // indices: 0='a', 1='b', 2-3='😀', 4='c', 5='d'
        val cursorPos = 5
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 3)
        // 3 code points: 'c'(1) + '😀'(2) + 'b'(1) = 4 UTF-16 units
        assertEquals(4, utf16Before)
    }

    // ── 中文字符（BMP，1 code point = 1 UTF-16 unit）──

    @Test
    fun testDeleteSurroundingTextInCodePoints_chinese_before() {
        // "你好世界"，光标在 2（'世' 前），删除前 2 个 code point
        val text = "你好世界"
        val cursorPos = 2
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 2)
        // 中文 BMP: 1 code point = 1 UTF-16 unit
        assertEquals(2, utf16Before)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_chineseAndEmoji_mixed() {
        // "你😀好"，光标在 3（'好' 前），删除前 2 个 code point
        val text = "你😀好"
        // indices: 0='你', 1-2='😀', 3='好'
        val cursorPos = 3
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 2)
        // 2 code points: '😀'(2 UTF-16 units) + '你'(1 UTF-16 unit) = 3 UTF-16 units
        assertEquals(3, utf16Before)
    }

    // ── 边界情况 ──

    @Test
    fun testDeleteSurroundingTextInCodePoints_zeroLength() {
        val text = "abc"
        val cursorPos = 1
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 0)
        val utf16After = codePointsToUtf16After(text, cursorPos, 0)
        assertEquals(0, utf16Before)
        assertEquals(0, utf16After)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_cursorAtStart() {
        val text = "abc"
        val cursorPos = 0
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 5)
        assertEquals(0, utf16Before) // 光标在最前面，前面没有字符
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_cursorAtEnd() {
        val text = "abc"
        val cursorPos = 3
        val utf16After = codePointsToUtf16After(text, cursorPos, 5)
        assertEquals(0, utf16After) // 光标在最后面，后面没有字符
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_cursorAtStart_emojiAfter() {
        val text = "😀a"
        val cursorPos = 0
        val utf16After = codePointsToUtf16After(text, cursorPos, 1)
        // 1 code point = '😀' = 2 UTF-16 units
        assertEquals(2, utf16After)
    }

    @Test
    fun testDeleteSurroundingTextInCodePoints_cursorAfterEmoji_emojiBefore() {
        val text = "a😀"
        val cursorPos = 3 // emoji 后
        val utf16Before = codePointsToUtf16Before(text, cursorPos, 1)
        // 1 code point = '😀' = 2 UTF-16 units
        assertEquals(2, utf16Before)
    }

    // ── 验证旧实现（错误）vs 新实现（正确）──

    @Test
    fun testOldImplementation_wrongForEmoji() {
        // 旧实现：直接委托 deleteSurroundingText(beforeLength, afterLength)
        // 对于 emoji，1 code point 被当成 1 UTF-16 unit，少删了
        val text = "a😀b"
        val cursorPos = 3 // 'b' 前
        val codePointBefore = 1 // 删除 1 个 code point

        // 旧实现错误结果：1 UTF-16 unit（只删了 emoji 的一半，导致乱码）
        val oldWrongResult = codePointBefore // 直接用 code point 数量当 UTF-16 数量

        // 新实现正确结果：2 UTF-16 units（完整删除 emoji）
        val correctResult = codePointsToUtf16Before(text, cursorPos, codePointBefore)

        assertNotEquals(oldWrongResult, correctResult)
        assertEquals(1, oldWrongResult) // 旧实现错误：1
        assertEquals(2, correctResult)  // 新实现正确：2
    }

    @Test
    fun testOldImplementation_correctForAscii() {
        // 对于 ASCII，旧实现和新实现结果相同
        val text = "abcde"
        val cursorPos = 3
        val codePointBefore = 2

        val oldResult = codePointBefore
        val newResult = codePointsToUtf16Before(text, cursorPos, codePointBefore)

        assertEquals(oldResult, newResult) // ASCII: 1 code point = 1 UTF-16 unit
    }
}
