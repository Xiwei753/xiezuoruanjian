package com.xiwei.sujian.editor.v2

import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.visual.SnapshotOwner
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import org.junit.Assert.*
import org.junit.Test
import android.graphics.Bitmap

class VisualResourceStoreTest {

    private fun makeSnapshot(id: Long, lineIndex: Int): AndroidLineSnapshot {
        val bitmap = Bitmap.createBitmap(100, 20, Bitmap.Config.ARGB_8888)
        return AndroidLineSnapshot(
            snapshotId = id,
            bitmap = bitmap,
            lineIndex = lineIndex,
            sourceRect = android.graphics.Rect(0, 0, 100, 20),
            destinationRect = android.graphics.RectF(0f, 0f, 100f, 20f)
        )
    }

    @Test
    fun putAndGetSnapshot() {
        val store = VisualResourceStore()
        val snapshot = makeSnapshot(1, 0)
        store.put(snapshot)

        val retrieved = store.get(1)
        assertNotNull(retrieved)
        assertEquals(0, retrieved!!.lineIndex)
    }

    @Test
    fun releaseRemovesSnapshot() {
        val store = VisualResourceStore()
        val snapshot = makeSnapshot(2, 0)
        store.put(snapshot)
        store.release(2, SnapshotOwner.OwnedByTransaction(System.nanoTime()))

        assertNull(store.get(2))
    }

    @Test
    fun releaseAllClearsEverything() {
        val store = VisualResourceStore()
        store.put(makeSnapshot(1, 0))
        store.put(makeSnapshot(2, 1))
        store.releaseAll()

        assertNull(store.get(1))
        assertNull(store.get(2))
    }

    @Test
    fun transferOwnershipMovesSnapshot() {
        val store = VisualResourceStore()
        val snapshot = makeSnapshot(1, 0)
        store.put(snapshot)

        assertTrue(store.transferOwnership(1, SnapshotOwner.OwnedByTransaction(10)))
        assertNull(store.get(1))
    }

    @Test
    fun transferOwnershipReturnsFalseForMissing() {
        val store = VisualResourceStore()
        assertFalse(store.transferOwnership(999, SnapshotOwner.OwnedByTransaction(1000)))
    }
}
