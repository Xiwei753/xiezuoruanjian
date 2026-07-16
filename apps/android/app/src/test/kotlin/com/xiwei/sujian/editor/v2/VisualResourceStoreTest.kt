package com.xiwei.sujian.editor.v2

import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import org.junit.Assert.*
import org.junit.Test
import android.graphics.Bitmap

class VisualResourceStoreTest {

    @Test
    fun putAndGetSnapshot() {
        val store = VisualResourceStore()
        val bitmap = Bitmap.createBitmap(100, 20, Bitmap.Config.ARGB_8888)
        val snapshot = AndroidLineSnapshot(1, 0, bitmap, android.graphics.Rect(0, 0, 100, 20))
        store.put(snapshot)

        val retrieved = store.get(1)
        assertNotNull(retrieved)
        assertEquals(0, retrieved!!.lineIndex)
    }

    @Test
    fun releaseRemovesSnapshot() {
        val store = VisualResourceStore()
        val bitmap = Bitmap.createBitmap(100, 20, Bitmap.Config.ARGB_8888)
        val snapshot = AndroidLineSnapshot(2, 0, bitmap, android.graphics.Rect(0, 0, 100, 20))
        store.put(snapshot)
        store.release(2)

        assertNull(store.get(2))
    }

    @Test
    fun releaseAllClearsEverything() {
        val store = VisualResourceStore()
        val bitmap1 = Bitmap.createBitmap(100, 20, Bitmap.Config.ARGB_8888)
        val bitmap2 = Bitmap.createBitmap(100, 20, Bitmap.Config.ARGB_8888)
        store.put(AndroidLineSnapshot(1, 0, bitmap1, android.graphics.Rect(0, 0, 100, 20)))
        store.put(AndroidLineSnapshot(2, 1, bitmap2, android.graphics.Rect(0, 0, 100, 20)))
        store.releaseAll()

        assertNull(store.get(1))
        assertNull(store.get(2))
    }

    @Test
    fun transferOwnershipMovesSnapshot() {
        val store = VisualResourceStore()
        val bitmap = Bitmap.createBitmap(100, 20, Bitmap.Config.ARGB_8888)
        val snapshot = AndroidLineSnapshot(1, 0, bitmap, android.graphics.Rect(0, 0, 100, 20))
        store.put(snapshot)

        assertTrue(store.transferOwnership(1, 10))
        assertNull(store.get(1))
        assertNotNull(store.get(10))
    }

    @Test
    fun transferOwnershipReturnsFalseForMissing() {
        val store = VisualResourceStore()
        assertFalse(store.transferOwnership(999, 1000))
    }
}
