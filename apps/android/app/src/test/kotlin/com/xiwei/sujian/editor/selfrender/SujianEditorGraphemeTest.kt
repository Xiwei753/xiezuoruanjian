package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

/**
 * 方向键 grapheme boundary 逻辑测试
 *
 * 验证 prevGraphemeBoundary / nextGraphemeBoundary 的遍历逻辑。
 *
 * 注意：android.icu.text.BreakIterator 在纯 JVM 单元测试中不可用，
 * 这里测试的是 grapheme boundary 的概念逻辑，使用模拟实现。
 * 实际的 BreakIterator 行为需要在 Android instrumented test 中验证。
 */
class SujianEditorGraphemeTest {

    // ── 模拟 grapheme boundary 逻辑 ──
    // 简化版：按 code point 边界移动
    // 实际实现使用 BreakIterator.getCharacterInstance()

    /**
     * 模拟 prevGraphemeBoundary：向前移动一个 code point
     * 正确处理 surrogate pair
     */
    private fun prevCodePointBoundary(text: String, offset: Int): Int {
        if (offset <= 0) return 0
        // 如果 offset-1 是低代理且 offset-2 是高代理，跳过整个 surrogate pair
        if (offset >= 2 && Character.isLowSurrogate(text[offset - 1]) && Character.isHighSurrogate(text[offset - 2])) {
            return offset - 2
        }
        return offset - 1
    }

    /**
     * 模拟 nextGraphemeBoundary：向后移动一个 code point
     * 正确处理 surrogate pair
     */
    private fun nextCodePointBoundary(text: String, offset: Int): Int {
        if (offset >= text.length) return text.length
        // 如果当前是高代理且下一个是低代理，跳过整个 surrogate pair
        if (Character.isHighSurrogate(text[offset]) && offset + 1 < text.length && Character.isLowSurrogate(text[offset + 1])) {
            return offset + 2
        }
        return offset + 1
    }

    // ── ASCII 左右移动 ──

    @Test
    fun testPrevCodePointBoundary_ascii() {
        assertEquals(2, prevCodePointBoundary("abc", 3))
        assertEquals(1, prevCodePointBoundary("abc", 2))
        assertEquals(0, prevCodePointBoundary("abc", 1))
        assertEquals(0, prevCodePointBoundary("abc", 0))
    }

    @Test
    fun testNextCodePointBoundary_ascii() {
        assertEquals(1, nextCodePointBoundary("abc", 0))
        assertEquals(2, nextCodePointBoundary("abc", 1))
        assertEquals(3, nextCodePointBoundary("abc", 2))
        assertEquals(3, nextCodePointBoundary("abc", 3))
    }

    // ── 中文左右移动 ──

    @Test
    fun testPrevCodePointBoundary_chinese() {
        assertEquals(1, prevCodePointBoundary("你好", 2))
        assertEquals(0, prevCodePointBoundary("你好", 1))
        assertEquals(0, prevCodePointBoundary("你好", 0))
    }

    @Test
    fun testNextCodePointBoundary_chinese() {
        assertEquals(1, nextCodePointBoundary("你好", 0))
        assertEquals(2, nextCodePointBoundary("你好", 1))
        assertEquals(2, nextCodePointBoundary("你好", 2))
    }

    // ── Emoji 左右移动（surrogate pair）──

    @Test
    fun testPrevCodePointBoundary_emoji() {
        // "a😀b" → indices: 0='a', 1=high, 2=low, 3='b'
        val text = "a😀b"
        // 从 offset 3 ('b') 向左 → offset 1 (emoji 开头)
        assertEquals(1, prevCodePointBoundary(text, 3))
        // 从 offset 1 (emoji 开头) 向左 → offset 0 ('a')
        assertEquals(0, prevCodePointBoundary(text, 1))
        // 从 offset 0 向左 → 0
        assertEquals(0, prevCodePointBoundary(text, 0))
    }

    @Test
    fun testNextCodePointBoundary_emoji() {
        val text = "a😀b"
        // 从 offset 0 ('a') 向右 → offset 1 (emoji 开头)
        assertEquals(1, nextCodePointBoundary(text, 0))
        // 从 offset 1 (emoji 开头) 向右 → offset 3 ('b')
        assertEquals(3, nextCodePointBoundary(text, 1))
        // 从 offset 3 ('b') 向右 → offset 4 (文本末尾)
        assertEquals(4, nextCodePointBoundary(text, 3))
    }

    @Test
    fun testPrevCodePointBoundary_emojiAtStart() {
        // "😀a" → indices: 0=high, 1=low, 2='a'
        val text = "😀a"
        // 从 offset 2 ('a') 向左 → offset 0 (emoji 开头)
        assertEquals(0, prevCodePointBoundary(text, 2))
    }

    @Test
    fun testNextCodePointBoundary_emojiAtStart() {
        val text = "😀a"
        // 从 offset 0 (emoji 开头) 向右 → offset 2 ('a')
        assertEquals(2, nextCodePointBoundary(text, 0))
    }

    // ── 连续 emoji ──

    @Test
    fun testPrevCodePointBoundary_consecutiveEmoji() {
        // "😀😀" → indices: 0=high, 1=low, 2=high, 3=low
        val text = "😀😀"
        // 从 offset 4 (末尾) 向左 → offset 2 (第二个 emoji 开头)
        assertEquals(2, prevCodePointBoundary(text, 4))
        // 从 offset 2 向左 → offset 0 (第一个 emoji 开头)
        assertEquals(0, prevCodePointBoundary(text, 2))
    }

    @Test
    fun testNextCodePointBoundary_consecutiveEmoji() {
        val text = "😀😀"
        // 从 offset 0 向右 → offset 2 (第二个 emoji 开头)
        assertEquals(2, nextCodePointBoundary(text, 0))
        // 从 offset 2 向右 → offset 4 (末尾)
        assertEquals(4, nextCodePointBoundary(text, 2))
    }

    // ── 混合文本 ──

    @Test
    fun testPrevCodePointBoundary_mixed() {
        // "a你😀b" → indices: 0='a', 1='你', 2=high, 3=low, 4='b'
        val text = "a你😀b"
        assertEquals(2, prevCodePointBoundary(text, 4))  // 'b' → emoji 开头
        assertEquals(1, prevCodePointBoundary(text, 2))  // emoji 开头 → '你'
        assertEquals(0, prevCodePointBoundary(text, 1))  // '你' → 'a'
    }

    @Test
    fun testNextCodePointBoundary_mixed() {
        val text = "a你😀b"
        assertEquals(1, nextCodePointBoundary(text, 0))  // 'a' → '你'
        assertEquals(2, nextCodePointBoundary(text, 1))  // '你' → emoji 开头
        assertEquals(4, nextCodePointBoundary(text, 2))  // emoji 开头 → 'b'
    }

    // ── 边界情况 ──

    @Test
    fun testPrevCodePointBoundary_emptyString() {
        assertEquals(0, prevCodePointBoundary("", 0))
    }

    @Test
    fun testNextCodePointBoundary_emptyString() {
        assertEquals(0, nextCodePointBoundary("", 0))
    }

    @Test
    fun testPrevCodePointBoundary_singleChar() {
        assertEquals(0, prevCodePointBoundary("a", 1))
        assertEquals(0, prevCodePointBoundary("a", 0))
    }

    @Test
    fun testNextCodePointBoundary_singleChar() {
        assertEquals(1, nextCodePointBoundary("a", 0))
        assertEquals(1, nextCodePointBoundary("a", 1))
    }

    @Test
    fun testPrevCodePointBoundary_singleEmoji() {
        val text = "😀"
        assertEquals(0, prevCodePointBoundary(text, 2)) // emoji 后 → emoji 前
        assertEquals(0, prevCodePointBoundary(text, 0))
    }

    @Test
    fun testNextCodePointBoundary_singleEmoji() {
        val text = "😀"
        assertEquals(2, nextCodePointBoundary(text, 0)) // emoji 前 → emoji 后
        assertEquals(2, nextCodePointBoundary(text, 2))
    }

    // ── 方向键选区扩展逻辑 ──
    // 验证 Shift+方向键的选区扩展语义

    @Test
    fun testShiftLeft_extendsSelection() {
        // 模拟：光标在 offset 3，Shift+Left → anchor=0, head=2
        val text = "abc"
        val cursorPos = 3
        val anchor = 0
        val newHead = prevCodePointBoundary(text, cursorPos)
        assertEquals(2, newHead)
        // 选区：anchor=0, head=2 → start=0, end=2
        val selStart = minOf(anchor, newHead)
        val selEnd = maxOf(anchor, newHead)
        assertEquals(0, selStart)
        assertEquals(2, selEnd)
    }

    @Test
    fun testShiftRight_extendsSelection() {
        // 模拟：光标在 offset 0，Shift+Right → anchor=0, head=1
        val text = "abc"
        val cursorPos = 0
        val anchor = 0
        val newHead = nextCodePointBoundary(text, cursorPos)
        assertEquals(1, newHead)
        val selStart = minOf(anchor, newHead)
        val selEnd = maxOf(anchor, newHead)
        assertEquals(0, selStart)
        assertEquals(1, selEnd)
    }

    @Test
    fun testNoShift_collapsedSelection() {
        // 模拟：有选区 [0, 2)，无 Shift+Left → 折叠到 start=0
        val selStart = 0
        val selEnd = 2
        // 无 Shift 按左键：折叠到选区 start
        val newOffset = selStart
        assertEquals(0, newOffset)
    }

    @Test
    fun testNoShift_right_collapsedSelection() {
        // 模拟：有选区 [0, 2)，无 Shift+Right → 折叠到 end=2
        val selStart = 0
        val selEnd = 2
        // 无 Shift 按右键：折叠到选区 end
        val newOffset = selEnd
        assertEquals(2, newOffset)
    }

    // ── 上下方向键 X 坐标记忆 ──
    // 验证上下移动时 X 坐标保持不变的概念

    @Test
    fun testUpDown_rememberX() {
        // 概念验证：上下移动时记住 X 坐标
        var rememberedX: Float? = null
        val currentX = 50f

        // 按上键：记住当前 X
        rememberedX = currentX
        assertEquals(50f, rememberedX!!)

        // 按下键：使用记忆的 X
        assertEquals(50f, rememberedX!!)
    }

    @Test
    fun testUpDown_clearRememberedXOnLeftRight() {
        var rememberedX: Float? = 50f

        // 按左/右键时清除记忆
        rememberedX = null
        assertNull(rememberedX)
    }
}
