package com.xiwei.sujian.feature.editor.visual.planner

import com.xiwei.sujian.feature.editor.layout.AndroidLayoutRevision
import com.xiwei.sujian.feature.editor.projection.CoordinatedCursor
import com.xiwei.sujian.feature.editor.projection.OffsetMap
import com.xiwei.sujian.feature.editor.projection.OffsetMapEntry
import com.xiwei.sujian.feature.editor.projection.OffsetMapKind
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * #606: Verifies that [AffectedLayoutPlanner.buildOffsetMapper],
 * [AffectedLayoutPlanner.buildReverseOffsetMapper] and
 * [AffectedLayoutPlanner.buildStandaloneReverseOffsetMapper] consume Core's
 * [VisualIntent.offsetMap] directly instead of reconstructing offset mappings
 * from old/new affected byte ranges.
 *
 * Positive: when offsetMap is non-null, the mapper uses Core's entries.
 * Negative: when offsetMap is null (cursor-only), the mapper returns identity.
 */
class AffectedLayoutPlannerOffsetMapTest {
    private val planner = AffectedLayoutPlanner()

    /** Minimal revision — content irrelevant for offset mapper tests. */
    private fun makeRevision(revisionId: Long): AndroidLayoutRevision =
        AndroidLayoutRevision(
            revisionId = revisionId,
            editorRevision = revisionId,
            widthFingerprint = 100f,
            fontFingerprint = "fp",
            lineCount = 1,
            lineRanges =
                listOf(
                    AndroidLayoutRevision.LineRange(
                        startUtf8 = 0,
                        endUtf8 = 20,
                        startUtf16 = 0,
                        endUtf16 = 20,
                        top = 0f,
                        bottom = 20f,
                        baseline = 16f,
                        left = 0f,
                        right = 100f,
                    ),
                ),
            cursorUtf8 = 0,
            cursorUtf16 = 0,
            cursorX = 0f,
            cursorY = 0f,
            cursorHeight = 20f,
            selectionAnchorUtf8 = 0,
            selectionHeadUtf8 = 0,
            selectionAnchorUtf16 = 0,
            selectionHeadUtf16 = 0,
            compositionStartUtf16 = 0,
            compositionEndUtf16 = 0,
            snapshotHandles = emptyList(),
        )

    private fun makeVisualIntent(offsetMap: OffsetMap?): VisualIntent =
        VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(5, 10)),
            newAffectedByteRanges = listOf(Pair(5, 12)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160,
            coordinatedCursor = CoordinatedCursor(0, 0, false),
            offsetMap = offsetMap,
        )

    // ── buildOffsetMapper (old → new) ──

    /**
     * Positive: OffsetMap with an IDENTITY entry maps old offsets to the same new offsets.
     */
    @Test
    fun buildOffsetMapper_identityEntry_mapsToSameOffset() {
        val offsetMap =
            OffsetMap(
                entries =
                    listOf(
                        OffsetMapEntry(oldByteOffset = 0, newByteOffset = 0, length = 5, kind = OffsetMapKind.IDENTITY),
                    ),
            )
        val visualIntent = makeVisualIntent(offsetMap)
        val mapper = planner.buildOffsetMapper(visualIntent, makeRevision(0), makeRevision(1))

        assertEquals(0, mapper(0))
        assertEquals(2, mapper(2))
        assertEquals(4, mapper(4))
    }

    /**
     * Positive: OffsetMap with a SHIFTED entry maps old offsets to shifted new offsets.
     * Entry: old [10, 20) → new [12, 22). old 15 → new 17.
     */
    @Test
    fun buildOffsetMapper_shiftedEntry_mapsToShiftedOffset() {
        val offsetMap =
            OffsetMap(
                entries =
                    listOf(
                        OffsetMapEntry(oldByteOffset = 0, newByteOffset = 0, length = 5, kind = OffsetMapKind.IDENTITY),
                        OffsetMapEntry(
                            oldByteOffset = 10,
                            newByteOffset = 12,
                            length = 10,
                            kind = OffsetMapKind.SHIFTED,
                        ),
                    ),
            )
        val visualIntent = makeVisualIntent(offsetMap)
        val mapper = planner.buildOffsetMapper(visualIntent, makeRevision(0), makeRevision(1))

        assertEquals(12, mapper(10))
        assertEquals(17, mapper(15))
        assertEquals(21, mapper(19))
    }

    /**
     * Negative: when offsetMap is null (cursor-only), mapper returns identity.
     */
    @Test
    fun buildOffsetMapper_nullOffsetMap_returnsIdentity() {
        val visualIntent = makeVisualIntent(offsetMap = null)
        val mapper = planner.buildOffsetMapper(visualIntent, makeRevision(0), makeRevision(1))

        assertEquals(0, mapper(0))
        assertEquals(7, mapper(7))
        assertEquals(15, mapper(15))
    }

    /**
     * Negative: offset in the changed region (not covered by any entry) returns null.
     */
    @Test
    fun buildOffsetMapper_offsetInChangedRegion_returnsNull() {
        val offsetMap =
            OffsetMap(
                entries =
                    listOf(
                        OffsetMapEntry(oldByteOffset = 0, newByteOffset = 0, length = 5, kind = OffsetMapKind.IDENTITY),
                        OffsetMapEntry(
                            oldByteOffset = 10,
                            newByteOffset = 12,
                            length = 10,
                            kind = OffsetMapKind.SHIFTED,
                        ),
                    ),
            )
        val visualIntent = makeVisualIntent(offsetMap)
        val mapper = planner.buildOffsetMapper(visualIntent, makeRevision(0), makeRevision(1))

        // offset 5-9 is in the changed region (gap between entries)
        assertNull(mapper(5))
        assertNull(mapper(7))
        assertNull(mapper(9))
    }

    /**
     * Negative: empty entries list returns identity mapper.
     */
    @Test
    fun buildOffsetMapper_emptyEntries_returnsIdentity() {
        val offsetMap = OffsetMap(entries = emptyList())
        val visualIntent = makeVisualIntent(offsetMap)
        val mapper = planner.buildOffsetMapper(visualIntent, makeRevision(0), makeRevision(1))

        assertEquals(5, mapper(5))
    }

    // ── buildReverseOffsetMapper (new → old) ──

    /**
     * Positive: reverse mapper with SHIFTED entry maps new offsets back to old offsets.
     * Entry: old [10, 20) → new [12, 22). new 17 → old 15.
     */
    @Test
    fun buildReverseOffsetMapper_shiftedEntry_mapsBackToOldOffset() {
        val offsetMap =
            OffsetMap(
                entries =
                    listOf(
                        OffsetMapEntry(oldByteOffset = 0, newByteOffset = 0, length = 5, kind = OffsetMapKind.IDENTITY),
                        OffsetMapEntry(
                            oldByteOffset = 10,
                            newByteOffset = 12,
                            length = 10,
                            kind = OffsetMapKind.SHIFTED,
                        ),
                    ),
            )
        val visualIntent = makeVisualIntent(offsetMap)
        val mapper = planner.buildReverseOffsetMapper(visualIntent, makeRevision(0), makeRevision(1))

        assertEquals(10, mapper(12))
        assertEquals(15, mapper(17))
        assertEquals(19, mapper(21))
    }

    /**
     * Negative: reverse mapper with null offsetMap returns identity.
     */
    @Test
    fun buildReverseOffsetMapper_nullOffsetMap_returnsIdentity() {
        val visualIntent = makeVisualIntent(offsetMap = null)
        val mapper = planner.buildReverseOffsetMapper(visualIntent, makeRevision(0), makeRevision(1))

        assertEquals(7, mapper(7))
    }

    /**
     * Negative: reverse mapper returns null for new offsets in the changed region.
     */
    @Test
    fun buildReverseOffsetMapper_offsetInChangedRegion_returnsNull() {
        val offsetMap =
            OffsetMap(
                entries =
                    listOf(
                        OffsetMapEntry(oldByteOffset = 0, newByteOffset = 0, length = 5, kind = OffsetMapKind.IDENTITY),
                        OffsetMapEntry(
                            oldByteOffset = 10,
                            newByteOffset = 12,
                            length = 10,
                            kind = OffsetMapKind.SHIFTED,
                        ),
                    ),
            )
        val visualIntent = makeVisualIntent(offsetMap)
        val mapper = planner.buildReverseOffsetMapper(visualIntent, makeRevision(0), makeRevision(1))

        // new offsets 5-11 are in the changed region (gap between entries)
        assertNull(mapper(5))
        assertNull(mapper(11))
    }

    // ── buildStandaloneReverseOffsetMapper (new → old, no revisions) ──

    /**
     * Positive: standalone reverse mapper uses Core's OffsetMap.
     */
    @Test
    fun buildStandaloneReverseOffsetMapper_shiftedEntry_mapsBackToOldOffset() {
        val offsetMap =
            OffsetMap(
                entries =
                    listOf(
                        OffsetMapEntry(oldByteOffset = 0, newByteOffset = 0, length = 5, kind = OffsetMapKind.IDENTITY),
                        OffsetMapEntry(
                            oldByteOffset = 10,
                            newByteOffset = 12,
                            length = 10,
                            kind = OffsetMapKind.SHIFTED,
                        ),
                    ),
            )
        val visualIntent = makeVisualIntent(offsetMap)
        val mapper = planner.buildStandaloneReverseOffsetMapper(visualIntent)

        assertEquals(10, mapper(12))
        assertEquals(15, mapper(17))
        assertEquals(19, mapper(21))
    }

    /**
     * Negative: standalone reverse mapper with null offsetMap returns identity.
     */
    @Test
    fun buildStandaloneReverseOffsetMapper_nullOffsetMap_returnsIdentity() {
        val visualIntent = makeVisualIntent(offsetMap = null)
        val mapper = planner.buildStandaloneReverseOffsetMapper(visualIntent)

        assertEquals(7, mapper(7))
    }

    /**
     * Negative: standalone reverse mapper returns null for changed region.
     */
    @Test
    fun buildStandaloneReverseOffsetMapper_offsetInChangedRegion_returnsNull() {
        val offsetMap =
            OffsetMap(
                entries =
                    listOf(
                        OffsetMapEntry(oldByteOffset = 0, newByteOffset = 0, length = 5, kind = OffsetMapKind.IDENTITY),
                        OffsetMapEntry(
                            oldByteOffset = 10,
                            newByteOffset = 12,
                            length = 10,
                            kind = OffsetMapKind.SHIFTED,
                        ),
                    ),
            )
        val visualIntent = makeVisualIntent(offsetMap)
        val mapper = planner.buildStandaloneReverseOffsetMapper(visualIntent)

        assertNull(mapper(5))
        assertNull(mapper(11))
    }
}
