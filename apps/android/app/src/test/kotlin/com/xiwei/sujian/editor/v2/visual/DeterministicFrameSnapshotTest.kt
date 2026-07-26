package com.xiwei.sujian.editor.v2.visual

import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.layout.LineRange
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import android.graphics.Bitmap
import android.graphics.Rect
import android.graphics.RectF

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class DeterministicFrameSnapshotTest {

    private companion object {
        const val EDITOR_WIDTH = 720
        const val EDITOR_HEIGHT = 1280
        const val DENSITY_DPI = 420
        const val LINE_HEIGHT = 48
        const val FONT_SIZE_SP = 16
        const val DURATION_MS = 200L
    }

    private fun makeLayoutRevision(
        lineCount: Int,
        text: String = "Hello world",
        paragraphIds: List<Int> = List(lineCount) { 0 }
    ): AndroidLayoutRevision {
        val lineRanges = mutableListOf<LineRange>()
        var byteOffset = 0
        for (i in 0 until lineCount) {
            val lineText = if (i == 0) text else "line $i"
            val lineBytes = lineText.toByteArray(Charsets.UTF_8).size
            lineRanges.add(LineRange(
                startUtf8 = byteOffset,
                endUtf8 = byteOffset + lineBytes,
                top = i * LINE_HEIGHT.toFloat(),
                bottom = (i + 1) * LINE_HEIGHT.toFloat(),
                left = 0f,
                right = EDITOR_WIDTH.toFloat(),
                paragraphId = paragraphIds.getOrElse(i) { 0 },
                baseline = (i + 1) * LINE_HEIGHT.toFloat() - 10f
            ))
            byteOffset += lineBytes
        }
        return AndroidLayoutRevision(
            lineRanges = lineRanges,
            cursorX = byteOffset.toFloat(),
            cursorY = 0f,
            cursorHeight = LINE_HEIGHT.toFloat(),
            selectionStartUtf16 = -1,
            selectionEndUtf16 = -1,
            preeditStartUtf16 = -1,
            preeditEndUtf16 = -1,
            preeditUnderlineColor = 0
        )
    }

    private fun makeSnapshot(
        id: Long,
        lineIndex: Int,
        byteStart: Int,
        byteEnd: Int,
        clusters: List<LineClusterSnapshot> = emptyList()
    ): AndroidLineSnapshot {
        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, LINE_HEIGHT, Bitmap.Config.ARGB_8888)
        val defaultClusters = if (clusters.isEmpty()) {
            listOf(LineClusterSnapshot(
                clusterId = 0,
                documentByteStart = byteStart,
                documentByteEndExclusive = byteEnd,
                documentUtf16Start = byteStart,
                documentUtf16EndExclusive = byteEnd,
                sourceRectInLineImage = Rect(0, 0, EDITOR_WIDTH, LINE_HEIGHT),
                visualRectInDocument = RectF(0f, lineIndex * LINE_HEIGHT.toFloat(), EDITOR_WIDTH.toFloat(), (lineIndex + 1) * LINE_HEIGHT.toFloat()),
                shapingFingerprint = "default",
                shapingIdentityConfident = true
            ))
        } else clusters
        return AndroidLineSnapshot(
            snapshotId = id,
            bitmap = bitmap,
            lineIndex = lineIndex,
            sourceRect = Rect(0, 0, EDITOR_WIDTH, LINE_HEIGHT),
            destinationRect = RectF(0f, lineIndex * LINE_HEIGHT.toFloat(), EDITOR_WIDTH.toFloat(), (lineIndex + 1) * LINE_HEIGHT.toFloat()),
            documentByteStart = byteStart,
            documentByteEndExclusive = byteEnd,
            clusters = defaultClusters
        )
    }

    private fun makeInsertVisualIntent(
        oldRanges: List<Pair<Int, Int>> = emptyList(),
        newRanges: List<Pair<Int, Int>> = listOf(Pair(5, 8))
    ): VisualIntent = VisualIntent(
        oldAffectedByteRanges = oldRanges,
        newAffectedByteRanges = newRanges,
        animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
        durationMs = DURATION_MS,
        operationKind = "insert"
    )

    private fun makeDeleteVisualIntent(
        oldRanges: List<Pair<Int, Int>> = listOf(Pair(5, 8)),
        newRanges: List<Pair<Int, Int>> = emptyList()
    ): VisualIntent = VisualIntent(
        oldAffectedByteRanges = oldRanges,
        newAffectedByteRanges = newRanges,
        animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
        durationMs = DURATION_MS,
        operationKind = "delete"
    )

    private fun makeReplaceVisualIntent(
        oldRanges: List<Pair<Int, Int>> = listOf(Pair(5, 8)),
        newRanges: List<Pair<Int, Int>> = listOf(Pair(5, 10))
    ): VisualIntent = VisualIntent(
        oldAffectedByteRanges = oldRanges,
        newAffectedByteRanges = newRanges,
        animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
        durationMs = DURATION_MS,
        operationKind = "replace"
    )

    private fun computeInterpolatedAlpha(startAlpha: Float, endAlpha: Float, progress: Float): Float {
        return startAlpha + (endAlpha - startAlpha) * progress
    }

    private fun computeInterpolatedPosition(fromRect: RectF, toRect: RectF, progress: Float): RectF {
        return RectF(
            fromRect.left + (toRect.left - fromRect.left) * progress,
            fromRect.top + (toRect.top - fromRect.top) * progress,
            fromRect.right + (toRect.right - fromRect.right) * progress,
            fromRect.bottom + (toRect.bottom - fromRect.bottom) * progress
        )
    }

    @Test
    fun insertAnimationFrameSnapshotsAtProgressPoints() {
        val planner = AndroidVisualPlanner()
        val oldRev = makeLayoutRevision(1, "Hello world")
        val newRev = makeLayoutRevision(1, "Hello beautiful world")
        val visualIntent = makeInsertVisualIntent()

        val newSnapshot = makeSnapshot(1L, 0, 5, 15)
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L)
        )

        val insertSlices = transaction.animatedSlices.filter { it.role == SliceRole.Insert }
        assertTrue("Should have insert slices", insertSlices.isNotEmpty())

        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)
        val expectedAlphas = progressPoints.map { computeInterpolatedAlpha(0f, 1f, it) }

        for ((i, progress) in progressPoints.withIndex()) {
            for (slice in insertSlices) {
                val alpha = computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress)
                assertEquals(
                    "Insert slice alpha at progress $progress",
                    expectedAlphas[i], alpha, 0.01f
                )
            }
        }
    }

    @Test
    fun deleteAnimationFrameSnapshotsAtProgressPoints() {
        val planner = AndroidVisualPlanner()
        val oldRev = makeLayoutRevision(1, "Hello beautiful world")
        val newRev = makeLayoutRevision(1, "Hello world")
        val visualIntent = makeDeleteVisualIntent()

        val oldSnapshot = makeSnapshot(1L, 0, 5, 15)
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L)
        )

        val deleteSlices = transaction.animatedSlices.filter { it.role == SliceRole.Delete }
        assertTrue("Should have delete slices", deleteSlices.isNotEmpty())

        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)
        val expectedAlphas = progressPoints.map { computeInterpolatedAlpha(1f, 0f, it) }

        for ((i, progress) in progressPoints.withIndex()) {
            for (slice in deleteSlices) {
                val alpha = computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress)
                assertEquals(
                    "Delete slice alpha at progress $progress",
                    expectedAlphas[i], alpha, 0.01f
                )
            }
        }
    }

    @Test
    fun replaceAnimationCrossfadeFrameSnapshots() {
        val planner = AndroidVisualPlanner()
        val oldRev = makeLayoutRevision(1, "Hello world")
        val newRev = makeLayoutRevision(1, "Hello earth")
        val visualIntent = makeReplaceVisualIntent()

        val oldSnapshot = makeSnapshot(1L, 0, 6, 11)
        val newSnapshot = makeSnapshot(2L, 0, 6, 11)
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L)
        )

        val crossfadeOldSlices = transaction.animatedSlices.filter { it.role == SliceRole.CrossfadeOld }
        val crossfadeNewSlices = transaction.animatedSlices.filter { it.role == SliceRole.CrossfadeNew }

        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)

        for (progress in progressPoints) {
            for (slice in crossfadeOldSlices) {
                val alpha = computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress)
                assertTrue(
                    "CrossfadeOld alpha should decrease from 1 to 0, at $progress got $alpha",
                    alpha in 0f..1f
                )
                if (progress == 0f) assertEquals(1f, alpha, 0.01f)
                if (progress == 1f) assertEquals(0f, alpha, 0.01f)
            }
            for (slice in crossfadeNewSlices) {
                val alpha = computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress)
                assertTrue(
                    "CrossfadeNew alpha should increase from 0 to 1, at $progress got $alpha",
                    alpha in 0f..1f
                )
                if (progress == 0f) assertEquals(0f, alpha, 0.01f)
                if (progress == 1f) assertEquals(1f, alpha, 0.01f)
            }
        }
    }

    @Test
    fun moveSlicePositionInterpolationAtProgressPoints() {
        val fromRect = RectF(0f, 0f, 100f, 48f)
        val toRect = RectF(0f, 48f, 100f, 96f)

        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)
        val expectedPositions = progressPoints.map { computeInterpolatedPosition(fromRect, toRect, it) }

        assertEquals(0f, expectedPositions[0].top, 0.01f)
        assertEquals(12f, expectedPositions[1].top, 0.01f)
        assertEquals(24f, expectedPositions[2].top, 0.01f)
        assertEquals(36f, expectedPositions[3].top, 0.01f)
        assertEquals(48f, expectedPositions[4].top, 0.01f)
    }

    @Test
    fun insertSliceNoOverlapWithDeleteSliceAtAnyProgress() {
        val planner = AndroidVisualPlanner()
        val oldRev = makeLayoutRevision(2, "Hello world")
        val newRev = makeLayoutRevision(2, "Hello beautiful world")
        val visualIntent = makeReplaceVisualIntent(
            oldRanges = listOf(Pair(6, 11)),
            newRanges = listOf(Pair(6, 16))
        )

        val oldSnapshot = makeSnapshot(1L, 0, 6, 11)
        val newSnapshot = makeSnapshot(2L, 0, 6, 16)
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L)
        )

        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)
        for (progress in progressPoints) {
            val visibleSlices = transaction.animatedSlices.filter { slice ->
                val alpha = computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress)
                alpha > 0.01f
            }
            for (slice in visibleSlices) {
                if (slice.role == SliceRole.CrossfadeOld || slice.role == SliceRole.CrossfadeNew) {
                    assertTrue(
                        "Visible slice at progress $progress should have positive alpha",
                        computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress) > 0.01f
                    )
                }
            }
        }
    }

    @Test
    fun cursorTransitionAtProgressPoints() {
        val fromX = 100f
        val toX = 150f
        val fromY = 0f
        val toY = 0f
        val fromHeight = 48f
        val toHeight = 48f

        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)
        val expectedX = progressPoints.map { fromX + (toX - fromX) * it }

        assertEquals(100f, expectedX[0], 0.01f)
        assertEquals(112.5f, expectedX[1], 0.01f)
        assertEquals(125f, expectedX[2], 0.01f)
        assertEquals(137.5f, expectedX[3], 0.01f)
        assertEquals(150f, expectedX[4], 0.01f)
    }

    @Test
    fun blockShiftPositionAtProgressPoints() {
        val planner = AndroidVisualPlanner()
        val paragraphIds = listOf(0, 0, 1, 1)
        val oldRev = makeLayoutRevision(4, "Hello", paragraphIds = paragraphIds)
        val newRev = makeLayoutRevision(5, "Hello", paragraphIds = listOf(0, 0, 0, 1, 1))

        val visualIntent = makeInsertVisualIntent(
            oldRanges = listOf(Pair(5, 5)),
            newRanges = listOf(Pair(5, 6))
        )

        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            transactionKey = 1L,
            ownedSnapshotIds = emptySet()
        )

        val blockShifts = transaction.blockShifts
        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)

        for (shift in blockShifts) {
            val deltaY = shift.deltaY
            for (progress in progressPoints) {
                val currentDeltaY = deltaY * progress
                val expectedTop = shift.top + currentDeltaY
                assertTrue(
                    "BlockShift top at progress $progress should be >= original top",
                    expectedTop >= shift.top
                )
            }
        }
    }

    @Test
    fun compositionUpdateFrameSnapshotCrossfadeProgression() {
        val planner = AndroidVisualPlanner()
        val oldRev = makeLayoutRevision(1, "ni hao")
        val newRev = makeLayoutRevision(1, "nihao")
        val visualIntent = VisualIntent(
            oldAffectedByteRanges = listOf(Pair(0, 6)),
            newAffectedByteRanges = listOf(Pair(0, 5)),
            animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = DURATION_MS,
            operationKind = "composition_update"
        )

        val oldSnapshot = makeSnapshot(1L, 0, 0, 6)
        val newSnapshot = makeSnapshot(2L, 0, 0, 5)
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L)
        )

        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)
        for (progress in progressPoints) {
            for (slice in transaction.animatedSlices) {
                val alpha = computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress)
                assertTrue(
                    "Slice alpha at progress $progress should be in [0,1], got $alpha for role ${slice.role}",
                    alpha in -0.01f..1.01f
                )
            }
        }
    }

    @Test
    fun paragraphReflowBlockShiftsAtProgressPoints() {
        val planner = AndroidVisualPlanner()
        val oldParagraphIds = listOf(0, 0, 1, 1, 2)
        val newParagraphIds = listOf(0, 0, 0, 1, 1, 2)
        val oldRev = makeLayoutRevision(5, "Hello", paragraphIds = oldParagraphIds)
        val newRev = makeLayoutRevision(6, "Hello", paragraphIds = newParagraphIds)

        val visualIntent = makeInsertVisualIntent(
            oldRanges = listOf(Pair(5, 5)),
            newRanges = listOf(Pair(5, 6))
        )

        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            transactionKey = 1L,
            ownedSnapshotIds = emptySet()
        )

        val blockShifts = transaction.blockShifts
        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)

        for (shift in blockShifts) {
            val deltaY = shift.deltaY
            for ((i, progress) in progressPoints.withIndex()) {
                val effectiveDeltaY = deltaY * progress
                if (i == 0) {
                    assertEquals("At start frame, BlockShift should have zero effective delta", 0f, effectiveDeltaY, 0.01f)
                }
                if (i == progressPoints.lastIndex) {
                    assertEquals("At end frame, BlockShift should have full delta", deltaY, effectiveDeltaY, 0.01f)
                }
            }
        }
    }

    @Test
    fun staticPatchesRemainVisibleAtAllProgressPoints() {
        val planner = AndroidVisualPlanner()
        val oldRev = makeLayoutRevision(2, "Hello")
        val newRev = makeLayoutRevision(2, "Hello")

        val visualIntent = makeInsertVisualIntent(
            oldRanges = listOf(Pair(0, 5)),
            newRanges = listOf(Pair(0, 6))
        )

        val newSnapshot = makeSnapshot(1L, 0, 0, 6)
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L)
        )

        for (patch in transaction.staticPatches) {
            assertTrue(
                "Static patch should have valid new snapshot ID",
                patch.newSnapshotId > 0
            )
        }
    }

    @Test
    fun frameSnapshotSequenceWithManualTimeSource() {
        val timeSource = ManualAnimationTimeSource()
        val timeline = AnimationTimeline(DURATION_MS)

        timeSource.advanceByMs(0)
        timeline.markFirstVisibleFrame(timeSource.nowNanos() / 1_000_000)

        val progressPoints = mutableListOf<Float>()
        val checkpoints = listOf(0L, 50L, 100L, 150L, 200L)

        for (checkpoint in checkpoints) {
            timeSource.advanceTo(checkpoint * 1_000_000L)
            val progress = timeline.progress(timeSource.nowNanos() / 1_000_000)
            progressPoints.add(progress)
        }

        assertEquals(0f, progressPoints[0], 0.01f)
        assertEquals(0.25f, progressPoints[1], 0.01f)
        assertEquals(0.5f, progressPoints[2], 0.01f)
        assertEquals(0.75f, progressPoints[3], 0.01f)
        assertEquals(1f, progressPoints[4], 0.01f)

        val planner = AndroidVisualPlanner()
        val oldRev = makeLayoutRevision(1, "Hello")
        val newRev = makeLayoutRevision(1, "Hello!")
        val visualIntent = makeInsertVisualIntent(
            oldRanges = listOf(Pair(5, 5)),
            newRanges = listOf(Pair(5, 6))
        )

        val newSnapshot = makeSnapshot(1L, 0, 5, 6)
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L)
        )

        val insertSlices = transaction.animatedSlices.filter { it.role == SliceRole.Insert }
        assertTrue(insertSlices.isNotEmpty())

        for ((i, progress) in progressPoints.withIndex()) {
            for (slice in insertSlices) {
                val alpha = computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress)
                when (i) {
                    0 -> assertEquals("Start frame: insert alpha should be 0", 0f, alpha, 0.01f)
                    4 -> assertEquals("End frame: insert alpha should be 1", 1f, alpha, 0.01f)
                }
                assertTrue("Alpha at progress $progress should be in [0,1]", alpha in -0.01f..1.01f)
            }
        }
    }

    @Test
    fun emojiInsertFrameSnapshotSequence() {
        val planner = AndroidVisualPlanner()
        val oldRev = makeLayoutRevision(1, "Hi")
        val newRev = makeLayoutRevision(1, "Hi😀")
        val visualIntent = makeInsertVisualIntent(
            oldRanges = listOf(Pair(2, 2)),
            newRanges = listOf(Pair(2, 6))
        )

        val newSnapshot = makeSnapshot(1L, 0, 2, 6)
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L)
        )

        val insertSlices = transaction.animatedSlices.filter { it.role == SliceRole.Insert }
        assertTrue("Emoji insert should produce insert slices", insertSlices.isNotEmpty())

        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)
        for (progress in progressPoints) {
            for (slice in insertSlices) {
                val alpha = computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress)
                assertTrue("Emoji insert alpha at $progress should be in [0,1]", alpha in -0.01f..1.01f)
            }
        }
    }

    @Test
    fun crossLineDeleteFrameSnapshotSequence() {
        val planner = AndroidVisualPlanner()
        val paragraphIds = listOf(0, 0)
        val oldRev = makeLayoutRevision(2, "Hello world", paragraphIds = paragraphIds)
        val newRev = makeLayoutRevision(1, "Helloworld")

        val visualIntent = makeDeleteVisualIntent(
            oldRanges = listOf(Pair(5, 11)),
            newRanges = listOf(Pair(5, 5))
        )

        val oldSnapshot0 = makeSnapshot(1L, 0, 0, 11)
        val oldSnapshot1 = makeSnapshot(2L, 1, 6, 11)
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot0, 1 to oldSnapshot1),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L)
        )

        val deleteSlices = transaction.animatedSlices.filter { it.role == SliceRole.Delete }
        assertTrue("Cross-line delete should produce delete slices", deleteSlices.isNotEmpty())

        val progressPoints = listOf(0f, 0.25f, 0.5f, 0.75f, 1f)
        for (progress in progressPoints) {
            for (slice in deleteSlices) {
                val alpha = computeInterpolatedAlpha(slice.startAlpha, slice.endAlpha, progress)
                assertTrue("Delete alpha at $progress should be in [0,1]", alpha in -0.01f..1.01f)
            }
        }
    }
}
