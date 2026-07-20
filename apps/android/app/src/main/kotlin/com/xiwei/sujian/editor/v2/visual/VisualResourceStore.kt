package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot

sealed class SnapshotOwner {
    data class OwnedBySession(val sessionId: String) : SnapshotOwner()
    data class OwnedByTransaction(val transactionKey: Long) : SnapshotOwner()
    object Released : SnapshotOwner()
}

/**
 * Ownership-tracked store for line snapshot bitmaps.
 *
 * Ownership model:
 * - [OwnedBySession]: snapshots tied to an edit session lifetime (released on session reset).
 * - [OwnedByTransaction]: snapshots tied to a single animation transaction's lifecycle.
 *   Released when the transaction completes, is cancelled, or transfers ownership to a
 *   successor transaction during rebase.
 * - [Released]: sentinel; the entry has been removed.
 *
 * Invariant: a Bitmap is recycled exactly once. Transfer (not duplicate release) is used
 * when a new transaction inherits snapshots from a prior transaction's rebase frame.
 *
 * Owner matching uses exact equality (not subtype/is-a check): [OwnedByTransaction]
 * matches only when the transactionKey is identical. This is essential because each
 * transaction has a unique key — a mismatch means a different transaction is attempting
 * the release, which must be silently ignored to prevent premature recycling of Bitmaps
 * still owned by the active transaction.
 */
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

    /**
     * Release a snapshot. [releaser] must match the current owner exactly — mismatched
     * owners are silently ignored (the Bitmap is not recycled). This prevents accidental
     * release by a wrong transaction: e.g. if transaction A owns a snapshot and transaction B
     * tries to release it, the mismatch means B's [OwnedByTransaction] key differs from A's,
     * so the release is a no-op and the Bitmap survives until A completes.
     *
     * This exact-match policy is the foundation of the two-phase ownership model in
     * [AndroidTextAnimationEngine.submit]: unreferenced snapshots are released by the
     * transaction that captured them (same key), while referenced snapshots from a prior
     * transaction are transferred (ownership change) before the old transaction releases
     * the rest. Without exact-match, the old transaction's release would also free
     * transferred snapshots, causing use-after-recycle in the new transaction.
     */
    fun release(snapshotId: Long, releaser: SnapshotOwner) {
        val entry = snapshots[snapshotId] ?: return
        if (!isOwner(entry.owner, releaser)) {
            return
        }
        entry.snapshot.bitmap?.recycle()
        entry.owner = SnapshotOwner.Released
        snapshots.remove(snapshotId)
    }

    fun releaseAll() {
        snapshots.values.forEach {
            if (it.owner !is SnapshotOwner.Released) {
                it.snapshot.bitmap?.recycle()
                it.owner = SnapshotOwner.Released
            }
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
