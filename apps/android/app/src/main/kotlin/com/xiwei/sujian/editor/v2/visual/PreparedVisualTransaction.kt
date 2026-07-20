package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision

data class PreparedVisualTransaction(
    val transactionId: Long,
    val oldRevision: AndroidLayoutRevision?,
    val newRevision: AndroidLayoutRevision?,
    val staticPatches: List<StaticPatch>,
    val animatedSlices: List<AnimatedSlice>,
    /** Snapshot IDs whose Bitmap lifecycle this transaction owns (will release on complete/cancel). */
    val ownedSnapshotIds: Set<Long>,
    /** All snapshot IDs referenced by slices or static patches (superset of ownedSnapshotIds;
     *  may include IDs inherited from a prior transaction via rebase ownership transfer). */
    val referencedSnapshotIds: Set<Long>,
    val selectionDecoration: SelectionDecoration?,
    val preeditDecoration: PreeditDecoration?,
    val cursorTransition: CursorTransition?,
    val durationMs: Long,
    /** Block-level vertical shifts for paragraphs after the edit paragraph whose Y geometry
     *  changed but whose text content is identical. These paragraphs do NOT need per-line
     *  Bitmap snapshots — the renderer applies a uniform Y translation to the visible
     *  portion of the static new-layout text. This prevents unbounded Bitmap allocation
     *  when editing near the top of a long document (every input would otherwise capture
     *  all lines from the edit point to the document end). */
    val blockShifts: List<BlockShift> = emptyList()
) {
    data class StaticPatch(
        val newSnapshotId: Long,
        val lineIndex: Int,
        val destinationRect: android.graphics.RectF,
        val visibleSourceRects: List<android.graphics.Rect>
    )

    /**
     * A single animated visual unit within a transaction.
     *
     * Coordinate contract:
     * - [sourceRect]: crop region inside the snapshot's line bitmap (pixel coords).
     * - [destinationRect]: final position in document coordinates (no scroll offset).
     * - [fromDestinationRect]: starting position for Move slices; null means start equals
     *   [destinationRect] (no movement) or the role uses alpha-only animation.
     *
     * Byte range contract (half-open intervals):
     * - [clusterByteStart] inclusive, [clusterByteEndExclusive] exclusive.
     * - Used for rebase matching and cross-line Move deduplication; -1 means untracked.
     */
    data class AnimatedSlice(
        val role: SliceRole,
        val snapshot: AndroidLineSnapshot?,
        val sourceRect: android.graphics.Rect,
        val destinationRect: android.graphics.RectF,
        val startAlpha: Float,
        val endAlpha: Float,
        val fromDestinationRect: android.graphics.RectF? = null,
        val clusterByteStart: Int = -1,
        val clusterByteEndExclusive: Int = -1
    )

    data class SelectionDecoration(
        val startUtf16: Int,
        val endUtf16: Int,
        val rects: List<android.graphics.RectF>
    )

    data class PreeditDecoration(
        val startUtf16: Int,
        val endUtf16: Int,
        val underlineColor: Int
    )

    data class CursorTransition(
        val fromX: Float,
        val fromY: Float,
        val fromHeight: Float,
        val toX: Float,
        val toY: Float,
        val toHeight: Float,
        val shouldAnimate: Boolean
    )

    data class BlockShift(
        val paragraphStartUtf8: Int,
        val paragraphEndUtf8: Int,
        val deltaY: Float
    )
}

/** Animation slice roles. Insert: fade in (0→1). Delete: fade out (1→0).
 *  Move: same shaping, position shift (alpha stays 1).
 *  CrossfadeOld/New: shaping changed, paired fade-out + fade-in. */
enum class SliceRole {
    Insert, Delete, Move, CrossfadeOld, CrossfadeNew, Static
}

/** Lifecycle states of a visual transaction. Only Rendering/Paused produce frames. */
enum class TransactionState {
    Pending, Prepared, Rendering, Paused, Completed, Cancelled
}
