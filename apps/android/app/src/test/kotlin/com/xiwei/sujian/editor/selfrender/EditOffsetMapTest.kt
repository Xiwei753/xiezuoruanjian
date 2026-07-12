package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test

class EditOffsetMapTest {

    @Test
    fun insert_prefixUnchanged_mapsCorrectly() {
        val map = EditOffsetMap.fromEdit(
            oldText = "ab",
            newText = "aXb",
            insertedRangeStart = 1,
            insertedRangeEnd = 2,
            isDelete = false
        )
        val oldRange = map.mapNewRangeToOld(0, 1)
        assertNotNull(oldRange)
        assertEquals(0, oldRange!!.start)
        assertEquals(1, oldRange.end)
    }

    @Test
    fun insert_suffixUnchanged_mapsCorrectly() {
        val map = EditOffsetMap.fromEdit(
            oldText = "ab",
            newText = "aXb",
            insertedRangeStart = 1,
            insertedRangeEnd = 2,
            isDelete = false
        )
        val oldRange = map.mapNewRangeToOld(2, 3)
        assertNotNull(oldRange)
        assertEquals(1, oldRange!!.start)
        assertEquals(2, oldRange.end)
    }

    @Test
    fun insert_insertedRange_returnsNull() {
        val map = EditOffsetMap.fromEdit(
            oldText = "ab",
            newText = "aXb",
            insertedRangeStart = 1,
            insertedRangeEnd = 2,
            isDelete = false
        )
        val oldRange = map.mapNewRangeToOld(1, 2)
        assertNull(oldRange)
    }

    @Test
    fun insert_isNewRangeInserted_true() {
        val map = EditOffsetMap.fromEdit(
            oldText = "ab",
            newText = "aXb",
            insertedRangeStart = 1,
            insertedRangeEnd = 2,
            isDelete = false
        )
        assertTrue(map.isNewRangeInserted(1, 2))
    }

    @Test
    fun insert_isNewRangeInserted_falseForUnchanged() {
        val map = EditOffsetMap.fromEdit(
            oldText = "ab",
            newText = "aXb",
            insertedRangeStart = 1,
            insertedRangeEnd = 2,
            isDelete = false
        )
        assertFalse(map.isNewRangeInserted(0, 1))
        assertFalse(map.isNewRangeInserted(2, 3))
    }

    @Test
    fun delete_prefixUnchanged_mapsCorrectly() {
        val map = EditOffsetMap.fromEdit(
            oldText = "abc",
            newText = "ac",
            insertedRangeStart = 0,
            insertedRangeEnd = 0,
            isDelete = true,
            deletedRangeStart = 1,
            deletedRangeEnd = 2
        )
        val newRange = map.mapOldRangeToNew(0, 1)
        assertNotNull(newRange)
        assertEquals(0, newRange!!.start)
        assertEquals(1, newRange.end)
    }

    @Test
    fun delete_suffixUnchanged_mapsCorrectly() {
        val map = EditOffsetMap.fromEdit(
            oldText = "abc",
            newText = "ac",
            insertedRangeStart = 0,
            insertedRangeEnd = 0,
            isDelete = true,
            deletedRangeStart = 1,
            deletedRangeEnd = 2
        )
        val newRange = map.mapOldRangeToNew(2, 3)
        assertNotNull(newRange)
        assertEquals(1, newRange!!.start)
        assertEquals(2, newRange.end)
    }

    @Test
    fun delete_deletedRange_returnsNull() {
        val map = EditOffsetMap.fromEdit(
            oldText = "abc",
            newText = "ac",
            insertedRangeStart = 0,
            insertedRangeEnd = 0,
            isDelete = true,
            deletedRangeStart = 1,
            deletedRangeEnd = 2
        )
        val newRange = map.mapOldRangeToNew(1, 2)
        assertNull(newRange)
    }

    @Test
    fun delete_isOldRangeDeleted_true() {
        val map = EditOffsetMap.fromEdit(
            oldText = "abc",
            newText = "ac",
            insertedRangeStart = 0,
            insertedRangeEnd = 0,
            isDelete = true,
            deletedRangeStart = 1,
            deletedRangeEnd = 2
        )
        assertTrue(map.isOldRangeDeleted(1, 2))
    }

    @Test
    fun delete_isOldRangeDeleted_falseForUnchanged() {
        val map = EditOffsetMap.fromEdit(
            oldText = "abc",
            newText = "ac",
            insertedRangeStart = 0,
            insertedRangeEnd = 0,
            isDelete = true,
            deletedRangeStart = 1,
            deletedRangeEnd = 2
        )
        assertFalse(map.isOldRangeDeleted(0, 1))
        assertFalse(map.isOldRangeDeleted(2, 3))
    }

    @Test
    fun insert_chineseText_mapsCorrectly() {
        val map = EditOffsetMap.fromEdit(
            oldText = "你好",
            newText = "你好的",
            insertedRangeStart = 6,
            insertedRangeEnd = 9,
            isDelete = false
        )
        val oldRange = map.mapNewRangeToOld(9, 12)
        assertNotNull(oldRange)
        assertEquals(3, oldRange!!.start)
        assertEquals(6, oldRange.end)
    }

    @Test
    fun insert_emptyOldText() {
        val map = EditOffsetMap.fromEdit(
            oldText = "",
            newText = "a",
            insertedRangeStart = 0,
            insertedRangeEnd = 1,
            isDelete = false
        )
        assertNull(map.mapNewRangeToOld(0, 1))
        assertTrue(map.isNewRangeInserted(0, 1))
    }

    @Test
    fun delete_emptyNewText() {
        val map = EditOffsetMap.fromEdit(
            oldText = "a",
            newText = "",
            insertedRangeStart = 0,
            insertedRangeEnd = 0,
            isDelete = true,
            deletedRangeStart = 0,
            deletedRangeEnd = 1
        )
        assertTrue(map.isOldRangeDeleted(0, 1))
        assertNull(map.mapOldRangeToNew(0, 1))
    }

    @Test
    fun insert_rangeCrossingSegments_returnsNull() {
        val map = EditOffsetMap.fromEdit(
            oldText = "ab",
            newText = "aXb",
            insertedRangeStart = 1,
            insertedRangeEnd = 2,
            isDelete = false
        )
        assertNull(map.mapNewRangeToOld(0, 3))
    }

    @Test
    fun insert_atBeginning() {
        val map = EditOffsetMap.fromEdit(
            oldText = "bc",
            newText = "abc",
            insertedRangeStart = 0,
            insertedRangeEnd = 1,
            isDelete = false
        )
        val oldRange = map.mapNewRangeToOld(1, 3)
        assertNotNull(oldRange)
        assertEquals(0, oldRange!!.start)
        assertEquals(2, oldRange.end)
    }

    @Test
    fun insert_atEnd() {
        val map = EditOffsetMap.fromEdit(
            oldText = "ab",
            newText = "abc",
            insertedRangeStart = 2,
            insertedRangeEnd = 3,
            isDelete = false
        )
        val oldRange = map.mapNewRangeToOld(0, 2)
        assertNotNull(oldRange)
        assertEquals(0, oldRange!!.start)
        assertEquals(2, oldRange.end)
    }
}
