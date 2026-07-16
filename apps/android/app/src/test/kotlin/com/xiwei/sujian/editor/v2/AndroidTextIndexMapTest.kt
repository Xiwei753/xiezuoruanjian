package com.xiwei.sujian.editor.v2

import com.xiwei.sujian.editor.v2.input.AndroidTextIndexMap
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import org.junit.Assert.*
import org.junit.Test

class AndroidTextIndexMapTest {

    @Test
    fun asciiTextUtf8EqualsUtf16() {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        val map = AndroidTextIndexMap(mirror)

        for (i in 0..5) {
            assertEquals(i, map.utf8ToUtf16(i))
            assertEquals(i, map.utf16ToUtf8(i))
        }
    }

    @Test
    fun chineseTextUtf8ToUtf16() {
        val mirror = DisplayTextMirror()
        mirror.loadText("你好", 6)
        val map = AndroidTextIndexMap(mirror)

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(1, map.utf8ToUtf16(3))
        assertEquals(2, map.utf8ToUtf16(6))
    }

    @Test
    fun chineseTextUtf16ToUtf8() {
        val mirror = DisplayTextMirror()
        mirror.loadText("你好", 6)
        val map = AndroidTextIndexMap(mirror)

        assertEquals(0, map.utf16ToUtf8(0))
        assertEquals(3, map.utf16ToUtf8(1))
        assertEquals(6, map.utf16ToUtf8(2))
    }

    @Test
    fun mixedAsciiAndChinese() {
        val mirror = DisplayTextMirror()
        mirror.loadText("a你b", 8)
        val map = AndroidTextIndexMap(mirror)

        assertEquals(0, map.utf8ToUtf16(0))
        assertEquals(1, map.utf8ToUtf16(1))
        assertEquals(2, map.utf8ToUtf16(4))
        assertEquals(3, map.utf8ToUtf16(5))
        assertEquals(4, map.utf8ToUtf16(8))
    }

    @Test
    fun outOfRangeReturnsLength() {
        val mirror = DisplayTextMirror()
        mirror.loadText("ab", 2)
        val map = AndroidTextIndexMap(mirror)

        assertEquals(2, map.utf8ToUtf16(100))
        assertEquals(2, map.utf16ToUtf8(100))
    }
}
