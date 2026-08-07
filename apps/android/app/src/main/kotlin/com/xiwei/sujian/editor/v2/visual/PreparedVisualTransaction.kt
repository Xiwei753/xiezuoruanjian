package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import uniffi.writer_core.EditorOperationKindDto

data class PreparedVisualTransaction(
    val transactionId: Long,
    val oldRevision: AndroidLayoutRevision?,
    val newRevision: AndroidLayoutRevision?,
    val staticPatches: List<StaticPatch>,
    val animatedSlices: List<AnimatedSlice>,
    val ownedSnapshotIds: Set<Long>,
    val referencedSnapshotIds: Set<Long>,
    val selectionDecoration: SelectionDecoration?,
    val preeditDecoration: PreeditDecoration?,
    val cursorTransition: CursorTransition?,
    val durationMs: Long,
    val blockShifts: List<BlockShift> = emptyList(),
    val operationKind: EditorOperationKindDto = EditorOperationKindDto.INSERT,
) {
    data class StaticPatch(
        val newSnapshotId: Long,
        val lineIndex: Int,
        val destinationRect: android.graphics.RectF,
        /** Sub-regions of the line Bitmap that should be drawn as static (non-animated)
         *  content. Used when the animation's hole-punching removed regions that are not
         *  covered by any animated slice but still need to be visible — e.g. a line where
         *  only some clusters are animated and the rest must be redrawn from the snapshot
         *  because the base static draw was clipped out. Each rect is in Bitmap pixel
         *  coordinates relative to the snapshot's [sourceRect] origin. */
        val visibleSourceRects: List<android.graphics.Rect>,
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
        val clusterByteEndExclusive: Int = -1,
    )

    data class SelectionDecoration(
        val startUtf16: Int,
        val endUtf16: Int,
    )

    data class PreeditDecoration(
        val startUtf16: Int,
        val endUtf16: Int,
        val underlineColor: Int,
    )

    data class CursorTransition(
        val fromX: Float,
        val fromY: Float,
        val fromHeight: Float,
        val toX: Float,
        val toY: Float,
        val toHeight: Float,
        val shouldAnimate: Boolean,
    )

    /**
     * Block-level vertical shift for a contiguous range of paragraphs after the edit
     * paragraph group whose Y geometry shifted but whose text content is identical.
     *
     * Line range convention: [startLineIndex] inclusive, [endLineIndexExclusive] exclusive
     * (half-open). The renderer uses these indices directly to clip and translate the
     * static new-layout text, avoiding per-frame UTF-8→UTF-16 offset conversion that
     * could land on the wrong line when the exclusive end coincides with a paragraph boundary.
     *
     * [deltaY] is positive when the block moved downward (newTop > oldTop).
     * The renderer interpolates: translateY = deltaY * (progress - 1), so at progress=0
     * the text is at its old position (shifted by -deltaY from the new layout) and at
     * progress=1 it rests at the new layout position (no shift).
     *
     * Merging: [AndroidVisualPlanner.mergeAdjacentBlockShifts] merges consecutive
     * BlockShifts whose line ranges are adjacent and whose deltaY is identical into a
     * single entry. This ensures the renderer performs at most one [layout.draw] per
     * merged block per frame, not one per paragraph — critical for long documents where
     * many paragraphs shift by the same amount. Geometric bounds (left/right) use the
     * min/max across all merged lines to ensure the clip rect covers every intermediate
     * line regardless of varying line widths.
     */
    data class BlockShift(
        val startLineIndex: Int,
        val endLineIndexExclusive: Int,
        val top: Float,
        val bottom: Float,
        val left: Float,
        val right: Float,
        val deltaY: Float,
        /** UTF-8 byte offset of the first line in this block. Used by rebase matching
         *  instead of [startLineIndex] because line indices shift across revisions when
         *  hard breaks are inserted/deleted — the old transaction's line N may become
         *  line N+1 in the new revision, causing line-index-based matching to fail.
         *
         *  Rebase continuity: [applyRebaseToBlockShifts] matches old/new BlockShifts by
         *  [startUtf8] and adjusts deltaY to (newDeltaY - oldCurrentTranslateY). This
         *  ensures the suffix text starts from the on-screen position of the old animation
         *  rather than jumping back to the full -newDeltaY offset. Without [startUtf8],
         *  line-index-based matching would pair the wrong BlockShifts after hard-break
         *  insertion/deletion, producing incorrect rebase adjustments and visible jumps. */
        val startUtf8: Int = -1,
        /** Exclusive UTF-8 byte offset of the last line in this block. Provides a stable
         *  document range [startUtf8, endUtf8Exclusive) for cross-revision matching.
         *  When [startUtf8] alone matches but [endUtf8Exclusive] differs, the match is
         *  downgraded from exact to approximate — this prevents pairing BlockShifts that
         *  cover different document regions after hard-break insertion/deletion, which
         *  would produce incorrect deltaY adjustments and visible jumps in the suffix text. */
        val endUtf8Exclusive: Int = -1,
    )
}

/** Animation slice roles. Insert: fade in (0→1). Delete: fade out (1→0).
 *  Move: same shaping, position shift (alpha stays 1).
 *  CrossfadeOld/New: shaping changed, paired fade-out + fade-in. */
enum class SliceRole {
    Insert,
    Delete,
    Move,
    CrossfadeOld,
    CrossfadeNew,
    Static,
}

/** Lifecycle states of a visual transaction. Only Rendering/Paused produce frames. */
enum class TransactionState {
    Pending,
    Prepared,
    Rendering,
    Paused,
    Completed,
    Cancelled,
}
