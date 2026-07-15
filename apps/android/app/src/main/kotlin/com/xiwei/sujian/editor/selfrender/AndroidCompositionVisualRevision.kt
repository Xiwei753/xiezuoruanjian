package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF

interface OwnedVisualRevision {
    val revisionId: Long
    val lineSnapshots: List<AndroidLineSnapshot>
    val owner: SnapshotOwner
    val sessionId: CompositionSessionId

    fun release(releaser: SnapshotOwner)
    fun transferToTransaction(transactionKey: ULong)
    fun reassignToTransaction(newTransactionKey: ULong)
    fun transferToSession(sid: CompositionSessionId)
    fun isReleased(): Boolean
}

data class AndroidCompositionVisualRevision(
    override val revisionId: Long = 0,
    override val sessionId: CompositionSessionId = CompositionSessionId(0),
    val committedText: String,
    val compositionReplaceRange: HalfOpenRange,
    val preeditRangeInVirtualText: HalfOpenRange,
    val preeditText: String,
    val virtualText: String,
    val affectedParagraphRange: HalfOpenRange,
    override val lineSnapshots: List<AndroidLineSnapshot>,
    val cursorRect: RectF,
    val decorationRanges: List<HalfOpenRange>
) : OwnedVisualRevision {
    override var owner: SnapshotOwner = SnapshotOwner.OwnedBySession(sessionId)
        private set

    override fun release(releaser: SnapshotOwner) {
        check(owner !is SnapshotOwner.Released) { "Double release of revision $revisionId by $releaser, already Released" }
        check(owner == releaser) { "Illegal release of revision $revisionId by $releaser, owner is $owner" }
        owner = SnapshotOwner.Released
        lineSnapshots.forEach { it.release(releaser) }
    }

    override fun transferToTransaction(transactionKey: ULong) {
        check(owner is SnapshotOwner.OwnedBySession) {
            "transferToTransaction: revision $revisionId owner is $owner, expected OwnedBySession"
        }
        owner = SnapshotOwner.OwnedByTransaction(transactionKey)
        lineSnapshots.forEach {
            it.transferToTransaction(transactionKey)
        }
    }

    override fun reassignToTransaction(newTransactionKey: ULong) {
        check(owner is SnapshotOwner.OwnedByTransaction) {
            "reassignToTransaction: revision $revisionId owner is $owner, expected OwnedByTransaction"
        }
        owner = SnapshotOwner.OwnedByTransaction(newTransactionKey)
        lineSnapshots.forEach {
            it.reassignToTransaction(newTransactionKey)
        }
    }

    override fun transferToSession(sid: CompositionSessionId) {
        check(owner is SnapshotOwner.OwnedByTransaction) {
            "transferToSession: revision $revisionId owner is $owner, expected OwnedByTransaction"
        }
        owner = SnapshotOwner.OwnedBySession(sid)
        lineSnapshots.forEach {
            it.transferToSession(sid)
        }
    }

    override fun isReleased(): Boolean = owner is SnapshotOwner.Released
}

data class CommittedVisualRevision(
    override val revisionId: Long = 0,
    override val sessionId: CompositionSessionId = CompositionSessionId(0),
    val fullText: String,
    val affectedParagraphRange: HalfOpenRange,
    override val lineSnapshots: List<AndroidLineSnapshot>,
    val cursorRect: RectF
) : OwnedVisualRevision {
    override var owner: SnapshotOwner = SnapshotOwner.OwnedBySession(sessionId)
        private set

    override fun release(releaser: SnapshotOwner) {
        check(owner !is SnapshotOwner.Released) { "Double release of committed revision $revisionId by $releaser, already Released" }
        check(owner == releaser) { "Illegal release of committed revision $revisionId by $releaser, owner is $owner" }
        owner = SnapshotOwner.Released
        lineSnapshots.forEach { it.release(releaser) }
    }

    override fun transferToTransaction(transactionKey: ULong) {
        check(owner is SnapshotOwner.OwnedBySession) {
            "transferToTransaction: committed revision $revisionId owner is $owner, expected OwnedBySession"
        }
        owner = SnapshotOwner.OwnedByTransaction(transactionKey)
        lineSnapshots.forEach {
            it.transferToTransaction(transactionKey)
        }
    }

    override fun reassignToTransaction(newTransactionKey: ULong) {
        check(owner is SnapshotOwner.OwnedByTransaction) {
            "reassignToTransaction: committed revision $revisionId owner is $owner, expected OwnedByTransaction"
        }
        owner = SnapshotOwner.OwnedByTransaction(newTransactionKey)
        lineSnapshots.forEach {
            it.reassignToTransaction(newTransactionKey)
        }
    }

    override fun transferToSession(sid: CompositionSessionId) {
        check(owner is SnapshotOwner.OwnedByTransaction) {
            "transferToSession: committed revision $revisionId owner is $owner, expected OwnedByTransaction"
        }
        owner = SnapshotOwner.OwnedBySession(sid)
        lineSnapshots.forEach {
            it.transferToSession(sid)
        }
    }

    override fun isReleased(): Boolean = owner is SnapshotOwner.Released
}

data class AndroidDecorationSlice(
    val rangeUtf16: HalfOpenRange,
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

sealed class TakeCurrentResult {
    data class Success(val revision: AndroidCompositionVisualRevision) : TakeCurrentResult()
    object NoRevisionAvailable : TakeCurrentResult()
    data class RevisionWithActiveTransaction(val activeTransactionKey: ULong) : TakeCurrentResult()
}

sealed class ReturnFromTransactionResult {
    object Accepted : ReturnFromTransactionResult()
    data class RejectedStale(val revision: OwnedVisualRevision) : ReturnFromTransactionResult()
}

class AndroidCompositionManager {
    private val TAG = "CompositionManager"
    private var currentRevision: AndroidCompositionVisualRevision? = null
    private var takenByTransactionKey: ULong? = null
    private var generation: Long = 0

    fun setCurrent(revision: AndroidCompositionVisualRevision?) {
        val oldCurrent = currentRevision
        currentRevision = if (revision != null) {
            takenByTransactionKey = null
            generation++
            revision
        } else {
            takenByTransactionKey = null
            generation++
            null
        }

        if (oldCurrent != null && oldCurrent.owner is SnapshotOwner.OwnedBySession) {
            oldCurrent.release(SnapshotOwner.OwnedBySession(oldCurrent.sessionId))
        }
    }

    fun getCurrent(): AndroidCompositionVisualRevision? = currentRevision

    fun getActiveTransactionKey(): ULong? = takenByTransactionKey

    fun getGeneration(): Long = generation

    @Deprecated("Use takeCurrentForTransactionTyped() instead. Returning null for both 'no revision' and 'revision with active transaction' is ambiguous.", ReplaceWith("takeCurrentForTransactionTyped(transactionKey)"))
    fun takeCurrentForTransaction(transactionKey: ULong): AndroidCompositionVisualRevision? {
        if (currentRevision == null && takenByTransactionKey != null) {
            return null
        }
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

    fun takeCurrentForTransactionTyped(transactionKey: ULong): TakeCurrentResult {
        if (currentRevision == null && takenByTransactionKey != null) {
            return TakeCurrentResult.RevisionWithActiveTransaction(takenByTransactionKey!!)
        }
        if (currentRevision != null && takenByTransactionKey != null) {
            throw IllegalStateException("takeCurrentForTransaction: illegal double take, already taken by $takenByTransactionKey, new request $transactionKey")
        }
        val rev = currentRevision ?: return TakeCurrentResult.NoRevisionAvailable
        check(rev.owner is SnapshotOwner.OwnedBySession) {
            "takeCurrentForTransaction: current revision ${rev.revisionId} owner is ${rev.owner}, expected OwnedBySession"
        }
        rev.transferToTransaction(transactionKey)
        takenByTransactionKey = transactionKey
        currentRevision = null
        return TakeCurrentResult.Success(rev)
    }

    fun reassignActiveTransactionKey(newTransactionKey: ULong) {
        check(takenByTransactionKey != null) {
            "reassignActiveTransactionKey: no active transaction to reassign"
        }
        takenByTransactionKey = newTransactionKey
    }

    fun returnFromTransaction(revision: OwnedVisualRevision, transactionKey: ULong, expectedGeneration: Long): ReturnFromTransactionResult {
        if (expectedGeneration != generation) {
            return ReturnFromTransactionResult.RejectedStale(revision)
        }
        if (takenByTransactionKey != transactionKey) {
            return ReturnFromTransactionResult.RejectedStale(revision)
        }
        check(revision.owner is SnapshotOwner.OwnedByTransaction && (revision.owner as SnapshotOwner.OwnedByTransaction).transactionKey == transactionKey) {
            "returnFromTransaction: revision ${revision.revisionId} owner is ${revision.owner}, expected OwnedByTransaction($transactionKey)"
        }
        if (revision is AndroidCompositionVisualRevision) {
            revision.transferToSession(revision.sessionId)
            currentRevision = revision
        }
        takenByTransactionKey = null
        generation++
        return ReturnFromTransactionResult.Accepted
    }

    fun clear() {
        if (currentRevision != null && currentRevision!!.owner is SnapshotOwner.OwnedBySession) {
            currentRevision!!.release(SnapshotOwner.OwnedBySession(currentRevision!!.sessionId))
        }
        currentRevision = null
        takenByTransactionKey = null
        generation++
    }

    fun buildVirtualText(committedText: String, compositionReplaceRange: HalfOpenRange, preeditText: String): String {
        val start = compositionReplaceRange.start.coerceIn(0, committedText.length)
        val end = compositionReplaceRange.end.coerceIn(0, committedText.length)
        return committedText.substring(0, start) + preeditText + committedText.substring(end)
    }
}
