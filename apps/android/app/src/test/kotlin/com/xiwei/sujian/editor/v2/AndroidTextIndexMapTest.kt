package com.xiwei.sujian.editor.v2

import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import org.junit.Assert.*
import org.junit.Test

class AndroidTextIndexMapTest {

    @Test
    fun asciiTextUtf8EqualsUtf16() {
        val map = AndroidTextIndexMap.fromText("Hello")

        for (i in 0..5) {
            assertEquals(i, map.utf8ToUtf16(i))
            assertEquals(i, map.utf16ToUtf8(i))
        }
    }

    @Test
    fun chineseTextUtf8ToUtf16() {
        val map = AndroidTextIndexMap.fromText("你好")

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(1, map.utf8ToUtf16(3))
        assertEquals(2, map.utf8ToUtf16(6))
    }

    @Test
    fun chineseTextUtf16ToUtf8() {
        val map = AndroidTextIndexMap.fromText("你好")

        assertEquals(0, map.utf16ToUtf8(0))
        assertEquals(3, map.utf16ToUtf8(1))
        assertEquals(6, map.utf16ToUtf8(2))
    }

    @Test
    fun mixedAsciiAndChinese() {
        val map = AndroidTextIndexMap.fromText("a你b")

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(1, map.utf8ToUtf16(1))
        assertEquals(2, map.utf8ToUtf16(4))
        assertEquals(3, map.utf8ToUtf16(5))
        assertEquals(3, map.utf8ToUtf16(8))
    }

    @Test
    fun outOfRangeReturnsLength() {
        val map = AndroidTextIndexMap.fromText("ab")

        assertEquals(2, map.utf8ToUtf16(100))
        assertEquals(2, map.utf16ToUtf8(100))
    }

    @Test
    fun emojiBasicMapping() {
        val map = AndroidTextIndexMap.fromText("a😀b")

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(1, map.utf8ToUtf16(1))
        assertEquals(3, map.utf8ToUtf16(5))
        assertEquals(4, map.utf8ToUtf16(6))
        assertEquals(1, map.utf16ToUtf8(1))
        assertEquals(5, map.utf16ToUtf8(3))
        assertEquals(6, map.utf16ToUtf8(4))
    }

    @Test
    fun surrogatePairEmoji() {
        val map = AndroidTextIndexMap.fromText("🎉")

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(2, map.utf8ToUtf16(4))
        assertEquals(0, map.utf16ToUtf8(0))
        assertEquals(4, map.utf16ToUtf8(2))
    }

    @Test
    fun multipleEmoji() {
        val map = AndroidTextIndexMap.fromText("😀🎉")

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(2, map.utf8ToUtf16(4))
        assertEquals(4, map.utf8ToUtf16(8))
        assertEquals(0, map.utf16ToUtf8(0))
        assertEquals(4, map.utf16ToUtf8(2))
        assertEquals(8, map.utf16ToUtf8(4))
    }

    @Test
    fun combiningCharacter() {
        val map = AndroidTextIndexMap.fromText("e\u0301")

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(1, map.utf8ToUtf16(1))
        assertEquals(2, map.utf8ToUtf16(3))
        assertEquals(0, map.utf16ToUtf8(0))
        assertEquals(1, map.utf16ToUtf8(1))
        assertEquals(3, map.utf16ToUtf8(2))
    }

    @Test
    fun mixedChineseEmojiAscii() {
        val map = AndroidTextIndexMap.fromText("你😀好")

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(1, map.utf8ToUtf16(3))
        assertEquals(3, map.utf8ToUtf16(7))
        assertEquals(4, map.utf8ToUtf16(10))
        assertEquals(0, map.utf16ToUtf8(0))
        assertEquals(3, map.utf16ToUtf8(1))
        assertEquals(7, map.utf16ToUtf8(3))
        assertEquals(10, map.utf16ToUtf8(4))
    }

    @Test
    fun utf16LengthWithEmoji() {
        val map = AndroidTextIndexMap.fromText("a😀b")

        assertEquals(4, map.getUtf16Length())
        assertEquals(6, map.getUtf8Length())
    }

    @Test
    fun utf16LengthChineseOnly() {
        val map = AndroidTextIndexMap.fromText("你好")

        assertEquals(2, map.getUtf16Length())
        assertEquals(6, map.getUtf8Length())
    }

    @Test
    fun zeroLengthText() {
        val map = AndroidTextIndexMap.fromText("")

        assertEquals(0, map.getUtf16Length())
        assertEquals(0, map.getUtf8Length())
        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(0, map.utf16ToUtf8(0))
    }

    @Test
    fun utf8NonBoundaryRoundsToNearest() {
        val map = AndroidTextIndexMap.fromText("你好")

        val at1 = map.utf8ToUtf16(1)
        val at2 = map.utf8ToUtf16(2)
        assertEquals(0, at1)
        assertEquals(0, at2)
    }

    @Test
    fun utf16SurrogateInteriorPosition() {
        val map = AndroidTextIndexMap.fromText("a😀b")

        val at1 = map.utf16ToUtf8(2)
        assertEquals(1, at1)
    }

    @Test
    fun flagEmoji() {
        val map = AndroidTextIndexMap.fromText("🇨🇳")

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(4, map.utf8ToUtf16(8))
        assertEquals(0, map.utf16ToUtf8(0))
        assertEquals(8, map.utf16ToUtf8(4))
        assertEquals(4, map.getUtf16Length())
        assertEquals(8, map.getUtf8Length())
    }

    @Test
    fun rangeConversions() {
        val map = AndroidTextIndexMap.fromText("a你b")

        val utf16Range = map.utf8RangeToUtf16(1, 5)
        assertEquals(1..3, utf16Range)

        val utf8Range = map.utf16RangeToUtf8(1, 3)
        assertEquals(Pair(1, 5), utf8Range)
    }
}
