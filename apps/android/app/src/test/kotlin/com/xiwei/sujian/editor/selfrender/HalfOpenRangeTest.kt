package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

class HalfOpenRangeTest {
    @Test
    fun basicProperties() {
        val range = HalfOpenRange(1, 5)
        assertEquals(1, range.start)
        assertEquals(5, range.end)
        assertEquals(4, range.length)
        assertFalse(range.isEmpty)
    }
    
    @Test
    fun emptyRange() {
        val range = HalfOpenRange(3, 3)
        assertEquals(0, range.length)
        assertTrue(range.isEmpty)
        assertFalse(range.contains(3))
    }
    
    @Test
    fun contains() {
        val range = HalfOpenRange(2, 6)
        assertFalse(range.contains(1))
        assertTrue(range.contains(2))
        assertTrue(range.contains(5))
        assertFalse(range.contains(6))
    }
    
    @Test
    fun overlaps() {
        val r1 = HalfOpenRange(0, 5)
        val r2 = HalfOpenRange(3, 8)
        assertTrue(r1.overlaps(r2))
        assertTrue(r2.overlaps(r1))
        
        val r3 = HalfOpenRange(5, 10)
        assertFalse(r1.overlaps(r3)) // [0,5) 和 [5,10) 不相交（半开）
    }
    
    @Test
    fun emoji_surrogatePair() {
        // 😀 = U+1F600, UTF-16 占 2 个 code unit
        val text = "a😀b"
        assertEquals(4, text.length)
        val emojiRange = HalfOpenRange(1, 3) // [1, 3) = 2 code units
        assertEquals(2, emojiRange.length)
    }
    
    @Test
    fun newline_range() {
        val range = HalfOpenRange(5, 6)
        assertTrue(range.contains(5))
        assertFalse(range.contains(6))
    }
    
    @Test
    fun lineEnd_boundary() {
        // 行尾边界：半开区间不包含 end
        val lineRange = HalfOpenRange(0, 10)
        val excludeRange = HalfOpenRange(8, 10)
        // 不相交检查：lineEnd <= excludeStart || lineStart >= excludeEnd
        assertFalse(lineRange.end <= excludeRange.start || lineRange.start >= excludeRange.end)
    }
    
    @Test(expected = IllegalArgumentException::class)
    fun invalidRange_throws() {
        HalfOpenRange(5, 3) // start > end
    }
}
