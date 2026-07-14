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

enum class SnapshotOwner {
    OwnedBySession, OwnedByTransaction, Released
}

data class OwnedRevision(
    val revision: AndroidCompositionVisualRevision,
    var owner: SnapshotOwner = SnapshotOwner.OwnedBySession
) {
    fun release() {
        check(owner != SnapshotOwner.Released) { "Double release of revision ${revision.revisionId}" }
        owner = SnapshotOwner.Released
        revision.release()
    }
}

class AndroidCompositionManager {
    private val TAG = "CompositionManager"
    private var currentOwned: OwnedRevision? = null
    private var takenByTransaction: Boolean = false

    fun setCurrent(revision: AndroidCompositionVisualRevision?) {
        val oldCurrent = currentOwned
        currentOwned = if (revision != null) OwnedRevision(revision) else null
        takenByTransaction = false

        if (oldCurrent != null && oldCurrent.owner == SnapshotOwner.OwnedBySession) {
            oldCurrent.release()
        }
    }

    fun getCurrent(): AndroidCompositionVisualRevision? = currentOwned?.revision

    fun takeCurrentForTransaction(): AndroidCompositionVisualRevision? {
        check(!takenByTransaction) {
            "takeCurrentForTransaction: double take — current revision was already taken by a transaction"
        }
        val owned = currentOwned ?: return null
        check(owned.owner == SnapshotOwner.OwnedBySession) {
            "takeCurrentForTransaction: current revision ${owned.revision.revisionId} owner is ${owned.owner}, expected OwnedBySession"
        }
        owned.owner = SnapshotOwner.OwnedByTransaction
        currentOwned = null
        takenByTransaction = true
        return owned.revision
    }

    fun clear() {
        if (currentOwned != null && currentOwned!!.owner == SnapshotOwner.OwnedBySession) {
            currentOwned!!.release()
        }
        currentOwned = null
        takenByTransaction = false
    }

    fun buildVirtualText(committedText: String, compositionReplaceRange: IntRange, preeditText: String): String {
        val start = compositionReplaceRange.first.coerceIn(0, committedText.length)
        val end = compositionReplaceRange.last.coerceIn(0, committedText.length)
        return committedText.substring(0, start) + preeditText + committedText.substring(end)
    }
}
