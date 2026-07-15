package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

data class AndroidCompositionVisualRevision(
    val committedText: String,
    val compositionReplaceRange: IntRange,
    val preeditRangeInVirtualText: IntRange,
    val preeditText: String,
    val virtualText: String,
    val affectedParagraphRange: IntRange,
    val lineSnapshots: List<AndroidLineSnapshot>,
    val cursorRect: RectF,
    val decorationRanges: List<IntRange>,
    val revisionId: Long = 0,
    val sessionId: CompositionSessionId = CompositionSessionId(0)
) {
    var owner: SnapshotOwner = SnapshotOwner.OwnedBySession(sessionId)
        private set

    fun release(releaser: SnapshotOwner) {
        check(owner !is SnapshotOwner.Released) { "Double release of revision $revisionId by $releaser, already Released" }
        check(owner == releaser) { "Illegal release of revision $revisionId by $releaser, owner is $owner" }
        owner = SnapshotOwner.Released
        lineSnapshots.forEach { it.release() }
    }

    fun transferToTransaction(transactionKey: ULong) {
        check(owner is SnapshotOwner.OwnedBySession) {
            "transferToTransaction: revision $revisionId owner is $owner, expected OwnedBySession"
        }
        owner = SnapshotOwner.OwnedByTransaction(transactionKey)
        lineSnapshots.forEach { it.transferToRevision(revisionId) }
    }

    fun reassignToTransaction(newTransactionKey: ULong) {
        check(owner is SnapshotOwner.OwnedByTransaction) {
            "reassignToTransaction: revision $revisionId owner is $owner, expected OwnedByTransaction"
        }
        owner = SnapshotOwner.OwnedByTransaction(newTransactionKey)
        lineSnapshots.forEach { it.transferToRevision(revisionId) }
    }

    fun transferToSession(sessionId: CompositionSessionId) {
        check(owner is SnapshotOwner.OwnedByTransaction) {
            "transferToSession: revision $revisionId owner is $owner, expected OwnedByTransaction"
        }
        owner = SnapshotOwner.OwnedBySession(sessionId)
        lineSnapshots.forEach { it.transferToRevision(revisionId) }
    }

    fun isReleased(): Boolean = owner is SnapshotOwner.Released
}

data class AndroidDecorationSlice(
    val rangeUtf16: IntRange,
    val kind: DecorationKind
)

enum class DecorationKind {
    Underline, ComposingCursor, SegmentColor
}

sealed class SnapshotOwner {
    data class OwnedBySession(val sessionId: CompositionSessionId) : SnapshotOwner()
    data class OwnedByTransaction(val transactionKey: ULong) : SnapshotOwner()
    object Released : SnapshotOwner()
}

class AndroidCompositionManager {
    private val TAG = "CompositionManager"
    private var currentRevision: AndroidCompositionVisualRevision? = null
    private var takenByTransactionKey: ULong? = null

    fun setCurrent(revision: AndroidCompositionVisualRevision?) {
        val oldCurrent = currentRevision
        currentRevision = if (revision != null) {
            takenByTransactionKey = null
            revision
        } else {
            takenByTransactionKey = null
            null
        }

        if (oldCurrent != null && oldCurrent.owner is SnapshotOwner.OwnedBySession) {
            oldCurrent.release(SnapshotOwner.OwnedBySession(oldCurrent.sessionId))
        }
    }

    fun getCurrent(): AndroidCompositionVisualRevision? = currentRevision

    fun takeCurrentForTransaction(transactionKey: ULong): AndroidCompositionVisualRevision? {
        if (currentRevision != null && takenByTransactionKey != null) {
            throw IllegalStateException("takeCurrentForTransaction: illegal double take, already taken by $takenByTransactionKey, new request $transactionKey")
        }
        val rev = currentRevision ?: return null
        check(rev.owner is SnapshotOwner.OwnedBySession) {
            "takeCurrentForTransaction: current revision ${rev.revisionId} owner is ${rev.owner}, expected OwnedBySession"
        }
        rev.transferToTransaction(transactionKey)
        takenByTransactionKey = transactionKey
        currentRevision = null
        return rev
    }

    fun getActiveTransactionKey(): ULong? = takenByTransactionKey

    fun returnFromTransaction(revision: AndroidCompositionVisualRevision, transactionKey: ULong) {
        if (takenByTransactionKey != transactionKey) {
            return
        }
        check(revision.owner is SnapshotOwner.OwnedByTransaction && (revision.owner as SnapshotOwner.OwnedByTransaction).transactionKey == transactionKey) {
            "returnFromTransaction: revision ${revision.revisionId} owner is ${revision.owner}, expected OwnedByTransaction($transactionKey)"
        }
        revision.transferToSession(revision.sessionId)
        currentRevision = revision
        takenByTransactionKey = null
    }

    fun clear() {
        if (currentRevision != null && currentRevision!!.owner is SnapshotOwner.OwnedBySession) {
            currentRevision!!.release(SnapshotOwner.OwnedBySession(currentRevision!!.sessionId))
        }
        currentRevision = null
        takenByTransactionKey = null
    }

    fun buildVirtualText(committedText: String, compositionReplaceRange: IntRange, preeditText: String): String {
        val start = compositionReplaceRange.first.coerceIn(0, committedText.length)
        val end = compositionReplaceRange.last.coerceIn(0, committedText.length)
        return committedText.substring(0, start) + preeditText + committedText.substring(end)
    }
}
