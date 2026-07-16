package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot

sealed class SnapshotOwner {
    data class OwnedBySession(val sessionId: String) : SnapshotOwner()
    data class OwnedByTransaction(val transactionKey: Long) : SnapshotOwner()
    object Released : SnapshotOwner()
}

class VisualResourceStore {
    private val snapshots = mutableMapOf<Long, OwnedSnapshot>()

    private data class OwnedSnapshot(
        val snapshot: AndroidLineSnapshot,
        var owner: SnapshotOwner
    )

    fun put(snapshot: AndroidLineSnapshot, owner: SnapshotOwner = SnapshotOwner.OwnedByTransaction(System.nanoTime())) {
        snapshots[snapshot.snapshotId] = OwnedSnapshot(snapshot, owner)
    }

    fun get(snapshotId: Long): AndroidLineSnapshot? = snapshots[snapshotId]?.snapshot

    fun release(snapshotId: Long, releaser: SnapshotOwner) {
        val entry = snapshots[snapshotId] ?: return
        if (!isOwner(entry.owner, releaser)) {
            throw IllegalStateException("Cannot release snapshot $snapshotId: owner mismatch. Current: ${entry.owner}, Releaser: $releaser")
        }
        entry.snapshot.bitmap?.recycle()
        entry.owner = SnapshotOwner.Released
        snapshots.remove(snapshotId)
    }

    fun releaseAll() {
        snapshots.values.forEach {
            it.snapshot.bitmap?.recycle()
            it.owner = SnapshotOwner.Released
        }
        snapshots.clear()
    }

    fun transferOwnership(fromSnapshotId: Long, toOwner: SnapshotOwner): Boolean {
        val entry = snapshots[fromSnapshotId] ?: return false
        entry.owner = toOwner
        return true
    }

    fun getOwner(snapshotId: Long): SnapshotOwner? = snapshots[snapshotId]?.owner

    private fun isOwner(current: SnapshotOwner, requester: SnapshotOwner): Boolean {
        return when {
            current is SnapshotOwner.Released -> false
            current is SnapshotOwner.OwnedBySession && requester is SnapshotOwner.OwnedBySession -> current.sessionId == requester.sessionId
            current is SnapshotOwner.OwnedByTransaction && requester is SnapshotOwner.OwnedByTransaction -> current.transactionKey == requester.transactionKey
            else -> false
        }
    }
}
