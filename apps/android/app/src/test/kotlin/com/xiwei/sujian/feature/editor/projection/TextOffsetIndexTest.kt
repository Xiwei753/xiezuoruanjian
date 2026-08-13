package com.xiwei.sujian.feature.editor.projection

import org.junit.Assert.assertEquals
import org.junit.Test
import java.util.Random

/**
 * [TextOffsetIndex] 单元测试。
 *
 * 覆盖：
 * - [TextOffsetIndex.rebuildFromText] + [TextOffsetIndex.utf8ToUtf16] /
 *   [TextOffsetIndex.utf16ToUtf8] 对纯 ASCII、中文、emoji、混合文本的正确性。
 * - [TextOffsetIndex.onBufferReplaced] 增量更新后查询与全量重建一致（各种编辑）。
 * - 边界情况：空文本、单段落、文末编辑、文首编辑、多 patch 序列。
 * - [TextOffsetIndex.utf8Length] / [TextOffsetIndex.utf16Length] 在增量更新后正确。
 * - 关键不变量：随机编辑序列，[TextOffsetIndex.onBufferReplaced] 后的索引查询结果
 *   与 [TextOffsetIndex.rebuildFromText] 完全一致。
 */
@Suppress("TooManyFunctions", "LargeClass")
class TextOffsetIndexTest {
    @Test
    fun emptyTextHasOneEmptyParagraph() {
        val index = TextOffsetIndex()
        index.rebuildFromText("")
        assertEquals(1, index.paragraphCount())
        assertEquals(0, index.utf8Length())
        assertEquals(0, index.utf16Length())
        assertEquals(0, index.utf8ToUtf16(0))
        assertEquals(0, index.utf16ToUtf8(0))
        // 越界查询 snap 到 0
        assertEquals(0, index.utf8ToUtf16(-1))
        assertEquals(0, index.utf16ToUtf8(-1))
        assertEquals(0, index.utf8ToUtf16(5))
        assertEquals(0, index.utf16ToUtf8(5))
    }

    @Test
    fun pureAsciiRoundTrip() {
        val text = "Hello World"
        val index = TextOffsetIndex()
        index.rebuildFromText(text)
        assertEquals(text.length, index.utf8Length())
        assertEquals(text.length, index.utf16Length())
        assertEquals(1, index.paragraphCount())
        for (i in 0..text.length) {
            assertEquals(i, index.utf8ToUtf16(i))
            assertEquals(i, index.utf16ToUtf8(i))
        }
        // 越界
        assertEquals(text.length, index.utf8ToUtf16(text.length + 5))
        assertEquals(text.length, index.utf16ToUtf8(text.length + 5))
    }

    @Test
    fun chineseTextBoundaries() {
        val text = "中文测试"
        val index = TextOffsetIndex()
        index.rebuildFromText(text)
        assertEquals(text.toByteArray(Charsets.UTF_8).size, index.utf8Length())
        assertEquals(text.length, index.utf16Length())
        assertEquals(1, index.paragraphCount())
        // 每个中文 codepoint: UTF-8 = 3 字节, UTF-16 = 1 单元
        assertEquals(0, index.utf8ToUtf16(0))
        assertEquals(0, index.utf8ToUtf16(1)) // snap 到「中」起始
        assertEquals(0, index.utf8ToUtf16(2)) // snap
        assertEquals(1, index.utf8ToUtf16(3))
        assertEquals(2, index.utf8ToUtf16(6))
        assertEquals(3, index.utf8ToUtf16(9))
        assertEquals(4, index.utf8ToUtf16(12))
        // 反向
        assertEquals(0, index.utf16ToUtf8(0))
        assertEquals(3, index.utf16ToUtf8(1))
        assertEquals(6, index.utf16ToUtf8(2))
        assertEquals(9, index.utf16ToUtf8(3))
        assertEquals(12, index.utf16ToUtf8(4))
    }

    @Test
    fun emojiTextBoundaries() {
        val text = "a😀b"
        val index = TextOffsetIndex()
        index.rebuildFromText(text)
        // a: 1 byte/1 utf16, 😀: 4 bytes UTF-8 / 2 units UTF-16, b: 1 byte/1 utf16
        assertEquals(6, index.utf8Length())
        assertEquals(4, index.utf16Length())
        assertEquals(1, index.paragraphCount())
        assertEquals(0, index.utf8ToUtf16(0))
        assertEquals(1, index.utf8ToUtf16(1))
        assertEquals(1, index.utf8ToUtf16(2)) // snap 到 😀 起始
        assertEquals(1, index.utf8ToUtf16(3)) // snap
        assertEquals(1, index.utf8ToUtf16(4)) // snap
        assertEquals(3, index.utf8ToUtf16(5))
        assertEquals(4, index.utf8ToUtf16(6))
        // 反向
        assertEquals(0, index.utf16ToUtf8(0))
        assertEquals(1, index.utf16ToUtf8(1))
        assertEquals(1, index.utf16ToUtf8(2)) // mid-surrogate snap 到 😀 起始
        assertEquals(5, index.utf16ToUtf8(3))
        assertEquals(6, index.utf16ToUtf8(4))
    }

    @Test
    fun mixedTextWithNewlines() {
        val text = "ab\n中\n😀\n"
        val index = TextOffsetIndex()
        index.rebuildFromText(text)
        // 段落 0 = "ab\n" (3 bytes/3 utf16)
        // 段落 1 = "中\n" (4 bytes/2 utf16)
        // 段落 2 = "😀\n" (5 bytes/3 utf16)
        assertEquals(3, index.paragraphCount())
        assertEquals(12, index.utf8Length())
        assertEquals(8, index.utf16Length())
        // 段落边界查询
        // 段落 0 起点 utf8=0/utf16=0
        // 段落 1 起点 utf8=3/utf16=3
        // 段落 2 起点 utf8=7/utf16=5
        assertEquals(3, index.utf8ToUtf16(3)) // 段落 1 起点
        assertEquals(5, index.utf8ToUtf16(7)) // 段落 2 起点
        assertEquals(3, index.utf16ToUtf8(3)) // 段落 1 起点
        assertEquals(7, index.utf16ToUtf8(5)) // 段落 2 起点
    }

    @Test
    fun textWithoutTrailingNewline() {
        val text = "abc\ndef"
        val index = TextOffsetIndex()
        index.rebuildFromText(text)
        // 段落 0 = "abc\n", 段落 1 = "def"（无尾换行）
        assertEquals(2, index.paragraphCount())
        assertEquals(7, index.utf8Length())
        assertEquals(7, index.utf16Length())
    }

    @Test
    fun consecutiveNewlines() {
        val text = "a\n\nb"
        val index = TextOffsetIndex()
        index.rebuildFromText(text)
        // 段落 0 = "a\n", 段落 1 = "\n", 段落 2 = "b"
        assertEquals(3, index.paragraphCount())
        assertEquals(4, index.utf8Length())
        assertEquals(4, index.utf16Length())
    }

    // ---- 增量更新测试 ----

    @Test
    fun insertCharacterAtEnd() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.insert(3, "d")
        index.onBufferReplaced(3, 3, "d", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun insertCharacterAtStart() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.insert(0, "X")
        index.onBufferReplaced(0, 0, "X", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun insertCharacterInMiddle() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.insert(1, "X")
        index.onBufferReplaced(1, 1, "X", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun deleteCharacterAtEnd() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.delete(2, 3)
        index.onBufferReplaced(2, 3, "", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun deleteCharacterAtStart() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.delete(0, 1)
        index.onBufferReplaced(0, 1, "", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun deleteCharacterInMiddle() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.delete(1, 2)
        index.onBufferReplaced(1, 2, "", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun insertNewlineSplitsParagraph() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.insert(1, "\n")
        index.onBufferReplaced(1, 1, "\n", buffer)
        assertIndexMatchesText(index, buffer.toString())
        assertEquals(2, index.paragraphCount())
    }

    @Test
    fun deleteNewlineMergesParagraphs() {
        val buffer = StringBuilder("ab\ncd")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        assertEquals(2, index.paragraphCount())
        buffer.delete(2, 3) // 删除 \n
        index.onBufferReplaced(2, 3, "", buffer)
        assertIndexMatchesText(index, buffer.toString())
        assertEquals(1, index.paragraphCount())
    }

    @Test
    fun replaceWithNewline() {
        val buffer = StringBuilder("abcdef")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.replace(2, 4, "\n")
        index.onBufferReplaced(2, 4, "\n", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun replaceSpanningMultipleParagraphs() {
        val buffer = StringBuilder("ab\ncd\nef\ngh")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        assertEquals(4, index.paragraphCount())
        // 替换从段落 1 中间到段落 2 中间，含多个 \n
        buffer.replace(3, 8, "XY\nZ")
        index.onBufferReplaced(3, 8, "XY\nZ", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun insertMultiByteCharacter() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.insert(1, "中")
        index.onBufferReplaced(1, 1, "中", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun insertEmoji() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.insert(1, "😀")
        index.onBufferReplaced(1, 1, "😀", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun deleteMultiByteCharacter() {
        val buffer = StringBuilder("a中b")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.delete(1, 2) // 删除「中」
        index.onBufferReplaced(1, 2, "", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun deleteEmoji() {
        val buffer = StringBuilder("a😀b")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.delete(1, 3) // 删除 😀（2 个 UTF-16 单元）
        index.onBufferReplaced(1, 3, "", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun replaceMultiByteWithAscii() {
        val buffer = StringBuilder("a中b文c")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        buffer.replace(1, 2, "XYZ") // 替换「中」为 XYZ
        index.onBufferReplaced(1, 2, "XYZ", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun multiplePatchesSequence() {
        val buffer = StringBuilder("Hello 世界\nabc\n😀\n")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        assertIndexMatchesText(index, buffer.toString())

        // 一系列编辑
        val edits =
            listOf(
                Triple(0, 0, "起"),
                Triple(5, 5, "\n"),
                Triple(3, 4, ""),
                Triple(0, 1, ""),
                Triple(2, 2, "😀"),
                Triple(10, 12, "替换"),
                Triple(4, 4, "\n\n"),
                Triple(7, 9, ""),
            )
        for ((start, end, ins) in edits) {
            val safeEnd = if (end > buffer.length) buffer.length else end
            val safeStart = if (start > safeEnd) safeEnd else start
            buffer.replace(safeStart, safeEnd, ins)
            index.onBufferReplaced(safeStart, safeEnd, ins, buffer)
            assertIndexMatchesText(index, buffer.toString())
        }
    }

    @Test
    fun replaceAllContent() {
        val buffer = StringBuilder("旧内容\n多行")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        val oldLen = buffer.length
        buffer.replace(0, oldLen, "全新\n内容\n")
        index.onBufferReplaced(0, oldLen, "全新\n内容\n", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun clearToEmpty() {
        val buffer = StringBuilder("abc\ndef")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        val oldLen = buffer.length
        buffer.replace(0, oldLen, "")
        index.onBufferReplaced(0, oldLen, "", buffer)
        assertIndexMatchesText(index, buffer.toString())
        assertEquals(1, index.paragraphCount())
        assertEquals(0, index.utf8Length())
        assertEquals(0, index.utf16Length())
    }

    @Test
    fun growFromEmpty() {
        val buffer = StringBuilder("")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        assertEquals(1, index.paragraphCount())

        buffer.append("第一段\n")
        index.onBufferReplaced(0, 0, "第一段\n", buffer)
        assertIndexMatchesText(index, buffer.toString())

        buffer.append("第二段")
        index.onBufferReplaced(buffer.length - 3, buffer.length - 3, "第二段", buffer)
        assertIndexMatchesText(index, buffer.toString())
    }

    @Test
    fun utf8AndUtf16LengthAfterIncrementalUpdates() {
        val buffer = StringBuilder("abc")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        assertEquals(3, index.utf8Length())
        assertEquals(3, index.utf16Length())

        buffer.insert(1, "中")
        index.onBufferReplaced(1, 1, "中", buffer)
        assertEquals(buffer.toString().toByteArray(Charsets.UTF_8).size, index.utf8Length())
        assertEquals(buffer.length, index.utf16Length())

        buffer.insert(0, "😀")
        index.onBufferReplaced(0, 0, "😀", buffer)
        assertEquals(buffer.toString().toByteArray(Charsets.UTF_8).size, index.utf8Length())
        assertEquals(buffer.length, index.utf16Length())

        buffer.delete(0, 2)
        index.onBufferReplaced(0, 2, "", buffer)
        assertEquals(buffer.toString().toByteArray(Charsets.UTF_8).size, index.utf8Length())
        assertEquals(buffer.length, index.utf16Length())
    }

    // ---- 关键不变量：随机编辑序列 ----

    @Test
    fun randomEditSequenceMatchesRebuild() {
        val random = Random(42L)
        val buffer = StringBuilder("Hello 世界\nabc\n😀\n")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        assertIndexMatchesText(index, buffer.toString())

        repeat(500) {
            val len = buffer.length
            if (len == 0) {
                val insertText = randomText(random, 8)
                buffer.insert(0, insertText)
                index.onBufferReplaced(0, 0, insertText, buffer)
            } else {
                // 随机选择插入位置和删除范围
                var start = random.nextInt(len + 1)
                start = alignToCodepoint(buffer, start)
                val maxEnd = len
                val end =
                    if (random.nextBoolean()) {
                        start // 纯插入
                    } else {
                        alignToCodepoint(buffer, start + random.nextInt(maxEnd - start + 1))
                    }
                val insertText = randomText(random, 8)
                buffer.replace(start, end, insertText)
                index.onBufferReplaced(start, end, insertText, buffer)
            }
            assertIndexMatchesText(index, buffer.toString())
        }
    }

    @Test
    fun randomEditSequenceAsciiOnly() {
        val random = Random(7L)
        val buffer = StringBuilder("hello world\nfoo bar\n")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        assertIndexMatchesText(index, buffer.toString())

        repeat(300) {
            val len = buffer.length
            if (len == 0) {
                val insertText = randomAscii(random, 10)
                buffer.insert(0, insertText)
                index.onBufferReplaced(0, 0, insertText, buffer)
            } else {
                var start = random.nextInt(len + 1)
                start = alignToCodepoint(buffer, start)
                val end =
                    if (random.nextBoolean()) {
                        start
                    } else {
                        alignToCodepoint(
                            buffer,
                            start + random.nextInt(len - start + 1),
                        )
                    }
                val insertText = randomAscii(random, 10)
                buffer.replace(start, end, insertText)
                index.onBufferReplaced(start, end, insertText, buffer)
            }
            assertIndexMatchesText(index, buffer.toString())
        }
    }

    @Test
    fun randomEditSequenceNewlineHeavy() {
        val random = Random(99L)
        val buffer = StringBuilder("a\nb\nc\nd\n")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        assertIndexMatchesText(index, buffer.toString())

        repeat(300) {
            val len = buffer.length
            if (len == 0) {
                val insertText = randomNewlineHeavy(random, 6)
                buffer.insert(0, insertText)
                index.onBufferReplaced(0, 0, insertText, buffer)
            } else {
                var start = random.nextInt(len + 1)
                start = alignToCodepoint(buffer, start)
                val end =
                    if (random.nextBoolean()) {
                        start
                    } else {
                        alignToCodepoint(
                            buffer,
                            start + random.nextInt(len - start + 1),
                        )
                    }
                val insertText = randomNewlineHeavy(random, 6)
                buffer.replace(start, end, insertText)
                index.onBufferReplaced(start, end, insertText, buffer)
            }
            assertIndexMatchesText(index, buffer.toString())
        }
    }

    @Test
    fun randomEditSequenceMultiByteHeavy() {
        val random = Random(2024L)
        val buffer = StringBuilder("中文\n测试😀\n")
        val index = TextOffsetIndex()
        index.rebuildFromText(buffer.toString())
        assertIndexMatchesText(index, buffer.toString())

        repeat(300) {
            val len = buffer.length
            if (len == 0) {
                val insertText = randomMultiByte(random, 6)
                buffer.insert(0, insertText)
                index.onBufferReplaced(0, 0, insertText, buffer)
            } else {
                var start = random.nextInt(len + 1)
                start = alignToCodepoint(buffer, start)
                val end =
                    if (random.nextBoolean()) {
                        start
                    } else {
                        alignToCodepoint(
                            buffer,
                            start + random.nextInt(len - start + 1),
                        )
                    }
                val insertText = randomMultiByte(random, 6)
                buffer.replace(start, end, insertText)
                index.onBufferReplaced(start, end, insertText, buffer)
            }
            assertIndexMatchesText(index, buffer.toString())
        }
    }

    /**
     * 将 [pos] 对齐到 codepoint 边界。若 [pos] 落在 surrogate pair 的 low surrogate 位置，
     * 回退到 high surrogate 之前，避免拆开 pair 产生孤立 surrogate。
     * 实际编辑路径（IME/光标）总会对齐到 codepoint 边界，随机测试需模拟这一约束。
     */
    private fun alignToCodepoint(
        buffer: CharSequence,
        pos: Int,
    ): Int {
        if (pos <= 0 || pos >= buffer.length) return pos
        if (Character.isLowSurrogate(buffer[pos]) && Character.isHighSurrogate(buffer[pos - 1])) {
            return pos - 1
        }
        return pos
    }

    // ---- 辅助 ----

    /**
     * 校验 [index] 的所有查询结果与对 [text] 全量重建的 [TextOffsetIndex] 完全一致，
     * 且 UTF-8/UTF-16 总长度与 [text] 的真实字节数一致。
     */
    private fun assertIndexMatchesText(
        index: TextOffsetIndex,
        text: String,
    ) {
        val full = TextOffsetIndex()
        full.rebuildFromText(text)
        assertEquals("utf8Length for '$text'", full.utf8Length(), index.utf8Length())
        assertEquals("utf16Length for '$text'", full.utf16Length(), index.utf16Length())
        assertEquals("paragraphCount for '$text'", full.paragraphCount(), index.paragraphCount())
        // 与 String 真实字节数一致
        assertEquals("real utf8 for '$text'", text.toByteArray(Charsets.UTF_8).size, index.utf8Length())
        assertEquals("real utf16 for '$text'", text.length, index.utf16Length())
        // 逐字节查询
        for (b in 0..full.utf8Length()) {
            assertEquals("utf8ToUtf16($b) for '$text'", full.utf8ToUtf16(b), index.utf8ToUtf16(b))
        }
        // 逐 UTF-16 单元查询
        for (u in 0..full.utf16Length()) {
            assertEquals("utf16ToUtf8($u) for '$text'", full.utf16ToUtf8(u), index.utf16ToUtf8(u))
        }
        // 越界查询 snap
        assertEquals("utf8ToUtf16 over for '$text'", full.utf16Length(), index.utf8ToUtf16(full.utf8Length() + 10))
        assertEquals("utf16ToUtf8 over for '$text'", full.utf8Length(), index.utf16ToUtf8(full.utf16Length() + 10))
    }

    private fun randomText(
        random: Random,
        maxLen: Int,
    ): String {
        // 用 String 而非 Char，因 😀 是 supplementary codepoint，不能用 Char 字面量。
        val chars = listOf("a", "b", "c", "d", "\n", "中", "文", "测", "试", "😀", " ", "€", "ß")
        val len = random.nextInt(maxLen + 1)
        val sb = StringBuilder()
        repeat(len) {
            sb.append(chars[random.nextInt(chars.size)])
        }
        return sb.toString()
    }

    private fun randomAscii(
        random: Random,
        maxLen: Int,
    ): String {
        val len = random.nextInt(maxLen + 1)
        val sb = StringBuilder()
        repeat(len) {
            val c = 'a' + random.nextInt(4)
            if (random.nextInt(5) == 0) {
                sb.append('\n')
            } else {
                sb.append(c)
            }
        }
        return sb.toString()
    }

    private fun randomNewlineHeavy(
        random: Random,
        maxLen: Int,
    ): String {
        val len = random.nextInt(maxLen + 1)
        val sb = StringBuilder()
        repeat(len) {
            if (random.nextInt(2) == 0) {
                sb.append('\n')
            } else {
                sb.append(('a' + random.nextInt(3)))
            }
        }
        return sb.toString()
    }

    private fun randomMultiByte(
        random: Random,
        maxLen: Int,
    ): String {
        val chars = listOf("中", "文", "😀", "€", "ß", "\n", "a")
        val len = random.nextInt(maxLen + 1)
        val sb = StringBuilder()
        repeat(len) {
            sb.append(chars[random.nextInt(chars.size)])
        }
        return sb.toString()
    }
}
