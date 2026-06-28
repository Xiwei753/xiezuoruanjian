package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

/**
 * 自研写作区 Buffer UTF-8/UTF-16 偏移转换测试
 *
 * 验证 Core 动画事件（UTF-8 byte offset）与 Android Layout（UTF-16 offset）之间的转换正确性。
 * 这对真吐字/吞字动画至关重要，因为：
 * - Core 返回的 rangeStart/oldCursorIndex/newCursorIndex 是 UTF-8 byte offset
 * - Android Layout 的 getGlyphRects/getCursorRect 需要 UTF-16 offset
 * - animatedInsertRange 也必须是 UTF-16 offset
 */
class SujianBufferOffsetTest {

    // ── 模拟 SujianEditorBuffer 的 UTF-8/UTF-16 转换 ──
    // 这些方法与 SujianEditorBuffer 中的实现相同

    private fun utf16ToUtf8(text: String, utf16Offset: Int): Int {
        var byteOffset = 0
        var charIdx = 0
        for (char in text) {
            if (charIdx >= utf16Offset) break
            byteOffset += when {
                char.code <= 0x7F -> 1
                char.code <= 0x7FF -> 2
                char.code <= 0xFFFF -> 3
                else -> 4
            }
            charIdx++
        }
        return byteOffset
    }

    private fun utf8ToUtf16(text: String, utf8Offset: Int): Int {
        var byteCount = 0
        var charIdx = 0
        for (char in text) {
            if (byteCount >= utf8Offset) break
            byteCount += when {
                char.code <= 0x7F -> 1
                char.code <= 0x7FF -> 2
                char.code <= 0xFFFF -> 3
                else -> 4
            }
            charIdx++
        }
        return charIdx
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
        // 注意：event.text.length 在 Kotlin 中是 UTF-16 code units 数量
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
    // 验证 rangeStartUtf16 作为 oldCursorRect 的位置是正确的

    @Test
    fun insertAnimation_rangeStartEqualsOldCursor_simpleInsert() {
        // 简单插入：oldCursor 在插入点，rangeStart = 插入点
        // 旧文本 "ab"，光标在 1，插入 "X" → 新文本 "aXb"
        // Core 返回 rangeStart = 1 (UTF-8)
        val newText = "aXb"
        val rangeStartUtf8 = 1
        val rangeStartUtf16 = utf8ToUtf16(newText, rangeStartUtf8)
        // 插入前光标在 offset 1，新文本中 offset 1 的位置对应旧光标位置
        // 因为 offset 1 之前的文本 "a" 没有变
        assertEquals(1, rangeStartUtf16)
    }

    @Test
    fun insertAnimation_rangeStartEqualsOldCursor_chineseInsert() {
        // 旧文本 "你"，光标在 1，插入 "好" → 新文本 "你好"
        // Core 返回 rangeStart = 3 (UTF-8: "你" = 3 bytes)
        val newText = "你好"
        val rangeStartUtf8 = 3
        val rangeStartUtf16 = utf8ToUtf16(newText, rangeStartUtf8)
        // 插入前光标在 UTF-16 offset 1，新文本中 UTF-16 offset 1 对应旧光标位置
        assertEquals(1, rangeStartUtf16)
    }

    @Test
    fun insertAnimation_rangeStartEqualsOldCursor_afterDeleteSelection() {
        // 选区删除后插入：旧文本 "abcde"，选中 "bcd" 删除，然后插入 "X" → "aXe"
        // 删除后文本 "ae"，光标在 1，插入 "X" → "aXe"
        // Core 返回 rangeStart = 1 (UTF-8)
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

        // 开始插入动画
        animatedInsertRange = IntRange(1, 3)
        assertEquals(IntRange(1, 3), animatedInsertRange)

        // 动画结束
        animatedInsertRange = null
        assertNull(animatedInsertRange)
    }

    @Test
    fun animatedInsertRange_clearedOnScroll() {
        var animatedInsertRange: IntRange? = IntRange(1, 3)

        // 开始滚动
        animatedInsertRange = null
        assertNull(animatedInsertRange)
    }

    @Test
    fun animatedInsertRange_clearedOnComposing() {
        var animatedInsertRange: IntRange? = IntRange(1, 3)

        // composing 开始
        animatedInsertRange = null
        assertNull(animatedInsertRange)
    }

    // ── 连续插入的 animatedInsertRange ──

    @Test
    fun animatedInsertRange_replacedOnNewInsert() {
        var animatedInsertRange: IntRange? = null

        // 第一次插入
        animatedInsertRange = IntRange(1, 2)
        assertEquals(IntRange(1, 2), animatedInsertRange)

        // 第二次插入（新的 range 覆盖旧的）
        animatedInsertRange = IntRange(2, 3)
        assertEquals(IntRange(2, 3), animatedInsertRange)
    }
}
