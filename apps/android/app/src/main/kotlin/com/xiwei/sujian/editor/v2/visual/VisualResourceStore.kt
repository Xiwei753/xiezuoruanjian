package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot

class VisualResourceStore {
    private val snapshots = mutableMapOf<Long, AndroidLineSnapshot>()

    fun put(snapshot: AndroidLineSnapshot) {
        snapshots[snapshot.snapshotId] = snapshot
    }

    fun get(snapshotId: Long): AndroidLineSnapshot? = snapshots[snapshotId]

    fun release(snapshotId: Long) {
        val snapshot = snapshots.remove(snapshotId)
        snapshot?.bitmap?.recycle()
    }

    fun releaseAll() {
        snapshots.values.forEach { it.bitmap?.recycle() }
        snapshots.clear()
    }

    fun transferOwnership(fromSnapshotId: Long, toSnapshotId: Long): Boolean {
        val snapshot = snapshots.remove(fromSnapshotId) ?: return false
        snapshots[toSnapshotId] = snapshot
        return true
    }
}
