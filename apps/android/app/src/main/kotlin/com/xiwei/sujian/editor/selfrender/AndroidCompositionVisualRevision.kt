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
    private var released = false

    fun release() {
        check(!released) { "Double release of revision $revisionId" }
        released = true
        lineSnapshots.forEach { it.release() }
    }

    fun isReleased(): Boolean = released
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

data class OwnedRevision(
    val revision: AndroidCompositionVisualRevision,
    var owner: SnapshotOwner = SnapshotOwner.OwnedBySession(CompositionSessionId(0))
) {
    fun release() {
        check(owner !is SnapshotOwner.Released) { "Double release of revision ${revision.revisionId}" }
        owner = SnapshotOwner.Released
        revision.release()
    }

    fun transferToTransaction(transactionKey: ULong) {
        check(owner is SnapshotOwner.OwnedBySession) {
            "transferToTransaction: revision ${revision.revisionId} owner is $owner, expected OwnedBySession"
        }
        owner = SnapshotOwner.OwnedByTransaction(transactionKey)
    }

    fun transferToSession(sessionId: CompositionSessionId) {
        check(owner is SnapshotOwner.OwnedByTransaction) {
            "transferToSession: revision ${revision.revisionId} owner is $owner, expected OwnedByTransaction"
        }
        owner = SnapshotOwner.OwnedBySession(sessionId)
    }
}

class AndroidCompositionManager {
    private val TAG = "CompositionManager"
    private var currentOwned: OwnedRevision? = null
    private var takenByTransactionKey: ULong? = null

    fun setCurrent(revision: AndroidCompositionVisualRevision?) {
        val oldCurrent = currentOwned
        currentOwned = if (revision != null) {
            takenByTransactionKey = null
            OwnedRevision(revision, SnapshotOwner.OwnedBySession(revision.sessionId))
        } else {
            takenByTransactionKey = null
            null
        }

        if (oldCurrent != null && oldCurrent.owner is SnapshotOwner.OwnedBySession) {
            oldCurrent.release()
        }
    }

    fun getCurrent(): AndroidCompositionVisualRevision? = currentOwned?.revision

    fun takeCurrentForTransaction(transactionKey: ULong): AndroidCompositionVisualRevision? {
        if (currentOwned == null && takenByTransactionKey != null) {
            return null
        }
        val owned = currentOwned ?: return null
        check(owned.owner is SnapshotOwner.OwnedBySession) {
            "takeCurrentForTransaction: current revision ${owned.revision.revisionId} owner is ${owned.owner}, expected OwnedBySession"
        }
        owned.transferToTransaction(transactionKey)
        takenByTransactionKey = transactionKey
        currentOwned = null
        return owned.revision
    }

    fun getActiveTransactionKey(): ULong? = takenByTransactionKey

    fun returnFromTransaction(revision: AndroidCompositionVisualRevision, transactionKey: ULong) {
        val owned = OwnedRevision(revision, SnapshotOwner.OwnedByTransaction(transactionKey))
        owned.transferToSession(revision.sessionId)
        currentOwned = owned
        takenByTransactionKey = null
    }

    fun clear() {
        if (currentOwned != null && currentOwned!!.owner is SnapshotOwner.OwnedBySession) {
            currentOwned!!.release()
        }
        currentOwned = null
        takenByTransactionKey = null
    }

    fun buildVirtualText(committedText: String, compositionReplaceRange: IntRange, preeditText: String): String {
        val start = compositionReplaceRange.first.coerceIn(0, committedText.length)
        val end = compositionReplaceRange.last.coerceIn(0, committedText.length)
        return committedText.substring(0, start) + preeditText + committedText.substring(end)
    }
}
