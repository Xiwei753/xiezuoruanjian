package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.visual.AndroidTextAnimationEngine
import com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner
import com.xiwei.sujian.editor.v2.visual.AnimationStateSnapshot
import com.xiwei.sujian.editor.v2.visual.AnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import com.xiwei.sujian.editor.v2.visual.TransactionState
import com.xiwei.sujian.editor.v2.visual.VisualResourceStore
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision.LineRange
import com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import android.graphics.Bitmap
import android.graphics.Rect
import android.graphics.RectF
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class FinalFrameResourceLifecycleTest {

    private companion object {
        const val EDITOR_WIDTH = 720
        const val EDITOR_HEIGHT = 1280
        const val LINE_HEIGHT = 48
        const val DURATION_MS = 200L
    }

    private fun makeLayoutRevision(
        lineCount: Int,
        text: String = "Hello world"
    ): AndroidLayoutRevision {
        val lineRanges = mutableListOf<LineRange>()
        var byteOffset = 0
        for (i in 0 until lineCount) {
            val lineText = if (i == 0) text else "line $i"
            val lineBytes = lineText.toByteArray(Charsets.UTF_8).size
            lineRanges.add(LineRange(
                startUtf8 = byteOffset,
                endUtf8 = byteOffset + lineBytes,
                startUtf16 = byteOffset,
                endUtf16 = byteOffset + lineBytes,
                top = i * LINE_HEIGHT.toFloat(),
                bottom = (i + 1) * LINE_HEIGHT.toFloat(),
                left = 0f,
                right = EDITOR_WIDTH.toFloat(),
                paragraphId = 0,
                baseline = (i + 1) * LINE_HEIGHT.toFloat() - 10f
            ))
            byteOffset += lineBytes
        }
        return AndroidLayoutRevision(
            revisionId = 1L,
            editorRevision = 1L,
            widthFingerprint = EDITOR_WIDTH.toFloat(),
            fontFingerprint = "test",
            lineCount = lineRanges.size,
            lineRanges = lineRanges,
            cursorUtf8 = byteOffset,
            cursorUtf16 = text.length,
            cursorX = byteOffset.toFloat(),
            cursorY = 0f,
            cursorHeight = LINE_HEIGHT.toFloat(),
            selectionAnchorUtf8 = -1,
            selectionHeadUtf8 = -1,
            selectionAnchorUtf16 = -1,
            selectionHeadUtf16 = -1,
            compositionStartUtf16 = -1,
            compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
    }

    private fun makeSnapshot(
        id: Long,
        lineIndex: Int,
        byteStart: Int,
        byteEnd: Int
    ): AndroidLineSnapshot {
        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, LINE_HEIGHT, Bitmap.Config.ARGB_8888)
        return AndroidLineSnapshot(
            snapshotId = id,
            bitmap = bitmap,
            lineIndex = lineIndex,
            sourceRect = Rect(0, 0, EDITOR_WIDTH, LINE_HEIGHT),
            destinationRect = RectF(0f, lineIndex * LINE_HEIGHT.toFloat(), EDITOR_WIDTH.toFloat(), (lineIndex + 1) * LINE_HEIGHT.toFloat()),
            documentByteStart = byteStart,
            documentByteEndExclusive = byteEnd,
            clusters = listOf(LineClusterSnapshot(
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
        )
    }

    private fun makeInsertVisualIntent(): VisualIntent = VisualIntent(
        cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
        operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
        oldAffectedByteRanges = emptyList(),
        newAffectedByteRanges = listOf(Pair(5, 8)),
        animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
        durationMs = DURATION_MS,
        coordinatedCursor = CoordinatedCursor(0, 0, true)
    )

    private fun makeDeleteVisualIntent(): VisualIntent = VisualIntent(
        cause = uniffi.writer_core.EditorTransactionCauseDto.DELETE,
        operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
        oldAffectedByteRanges = listOf(Pair(5, 8)),
        newAffectedByteRanges = emptyList(),
        animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
        durationMs = DURATION_MS,
        coordinatedCursor = CoordinatedCursor(0, 0, true)
    )

    private fun makeLayout(text: String): android.text.Layout {
        return android.text.StaticLayout(
            text, android.text.TextPaint(), EDITOR_WIDTH,
            android.text.Layout.Alignment.ALIGN_NORMAL, 1f, 0f, false
        )
    }

    @Test
    fun insert_tickReturnsCompleteAfterDrawTrueAtEnd_resourcesNotReleasedBeforeDraw() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val planner = AndroidVisualPlanner()
        val resourceStore = VisualResourceStore()
        val engine = AndroidTextAnimationEngine(planner, resourceStore, manualTimeSource, transactionIdSource)
        val runtime = AndroidVisualRuntime(planner, engine, resourceStore)

        val oldRev = makeLayoutRevision(1, "Hello world")
        val newRev = makeLayoutRevision(1, "Hello beautiful world")
        val visualIntent = makeInsertVisualIntent()

        val newSnapshot = makeSnapshot(1L, 0, 5, 15)
        val prepared = engine.prepare(
            visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            newSnapshots = mapOf(0 to newSnapshot)
        )
        engine.submit(prepared)

        manualTimeSource.advanceByMs(1)
        val startFrameTimeMs = manualTimeSource.nowNanos() / 1_000_000

        val startLayout = makeLayout("Hello beautiful world")
        runtime.tick(
            startFrameTimeMs, startLayout, newRev, emptyList(),
            EDITOR_WIDTH, EDITOR_HEIGHT, 0f, 0f,
            true, false, 15, 15, 15
        )

        val midFrameTimeMs = startFrameTimeMs + DURATION_MS / 2
        manualTimeSource.advanceTo(midFrameTimeMs * 1_000_000L)

        val midSnapshot = engine.captureStateSnapshot(midFrameTimeMs)
        assertNotNull("At 50%, state snapshot must exist", midSnapshot)
        assertTrue(
            "At 50%, ownedResourceCount must be > 0 (Bitmaps not yet recycled)",
            midSnapshot!!.ownedResourceCount > 0
        )

        val endFrameTimeMs = startFrameTimeMs + DURATION_MS
        manualTimeSource.advanceTo(endFrameTimeMs * 1_000_000L)

        val preDrawSnapshot = engine.captureStateSnapshot(endFrameTimeMs)
        assertNotNull(
            "Before completeAfterDraw, state snapshot must exist (transaction still active)",
            preDrawSnapshot
        )
        assertTrue(
            "Before completeAfterDraw, ownedResourceCount must be > 0 (Bitmaps not yet recycled), but was ${preDrawSnapshot!!.ownedResourceCount}",
            preDrawSnapshot.ownedResourceCount > 0
        )

        val frameState = runtime.tick(
            endFrameTimeMs, startLayout, newRev, emptyList(),
            EDITOR_WIDTH, EDITOR_HEIGHT, 0f, 0f,
            true, false, 15, 15, 15
        )
        assertNotNull("tick must return FrameState at 100%", frameState)
        assertTrue(
            "tick must set completeAfterDraw=true at 100%",
            frameState!!.completeAfterDraw
        )
        assertNull(
            "Terminal transaction must not be rendered (the static renderer draws the identical final state)",
            frameState.renderInput.transaction
        )
        assertNotNull(
            "Transaction must still be active before completeIfFinished (Bitmaps not yet recycled)",
            engine.getActiveTransaction()
        )

        engine.completeIfFinished(endFrameTimeMs)

        val afterDrawSnapshot = engine.captureStateSnapshot(endFrameTimeMs)
        assertNotNull(
            "After completeIfFinished, terminal state must remain queryable (Completed)",
            afterDrawSnapshot
        )
        assertEquals(
            "After completeIfFinished, transaction state must be Completed",
            TransactionState.Completed,
            afterDrawSnapshot!!.transactionState
        )
        assertEquals(
            "After completeIfFinished, all resources must be released",
            0,
            afterDrawSnapshot.ownedResourceCount
        )
        assertFalse("After completeIfFinished, no active animation", engine.hasActiveAnimation())
    }

    @Test
    fun delete_tickReturnsCompleteAfterDrawTrueAtEnd_resourcesNotReleasedBeforeDraw() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val planner = AndroidVisualPlanner()
        val resourceStore = VisualResourceStore()
        val engine = AndroidTextAnimationEngine(planner, resourceStore, manualTimeSource, transactionIdSource)
        val runtime = AndroidVisualRuntime(planner, engine, resourceStore)

        val oldRev = makeLayoutRevision(1, "Hello beautiful world")
        val newRev = makeLayoutRevision(1, "Hello world")
        val visualIntent = makeDeleteVisualIntent()

        val oldSnapshot = makeSnapshot(1L, 0, 5, 15)
        val prepared = engine.prepare(
            visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            oldSnapshots = mapOf(0 to oldSnapshot)
        )
        engine.submit(prepared)

        manualTimeSource.advanceByMs(1)
        val startFrameTimeMs = manualTimeSource.nowNanos() / 1_000_000

        val startLayout = makeLayout("Hello world")
        runtime.tick(
            startFrameTimeMs, startLayout, newRev, emptyList(),
            EDITOR_WIDTH, EDITOR_HEIGHT, 0f, 0f,
            true, false, 5, 5, 5
        )

        val endFrameTimeMs = startFrameTimeMs + DURATION_MS
        manualTimeSource.advanceTo(endFrameTimeMs * 1_000_000L)

        val preDrawSnapshot = engine.captureStateSnapshot(endFrameTimeMs)
        assertNotNull(
            "Before completeAfterDraw for delete, state snapshot must exist",
            preDrawSnapshot
        )
        assertTrue(
            "Before completeAfterDraw for delete, ownedResourceCount must be > 0, but was ${preDrawSnapshot!!.ownedResourceCount}",
            preDrawSnapshot.ownedResourceCount > 0
        )

        val frameState = runtime.tick(
            endFrameTimeMs, startLayout, newRev, emptyList(),
            EDITOR_WIDTH, EDITOR_HEIGHT, 0f, 0f,
            true, false, 5, 5, 5
        )
        assertNotNull("tick must return FrameState at 100% for delete", frameState)
        assertTrue(
            "tick must set completeAfterDraw=true at 100% for delete",
            frameState!!.completeAfterDraw
        )
        assertNull(
            "Terminal transaction must not be rendered for delete (static renderer draws the final state)",
            frameState.renderInput.transaction
        )
        assertNotNull(
            "Transaction must still be active before completeIfFinished for delete",
            engine.getActiveTransaction()
        )

        engine.completeIfFinished(endFrameTimeMs)

        val afterDrawSnapshot = engine.captureStateSnapshot(endFrameTimeMs)
        assertNotNull(
            "After completeIfFinished for delete, terminal state must remain queryable (Completed)",
            afterDrawSnapshot
        )
        assertEquals(
            "After completeIfFinished for delete, transaction state must be Completed",
            TransactionState.Completed,
            afterDrawSnapshot!!.transactionState
        )
        assertEquals(
            "After completeIfFinished for delete, all resources must be released",
            0,
            afterDrawSnapshot.ownedResourceCount
        )
    }

    @Test
    fun tick_completeAfterDrawFalseBeforeAnimationEnd() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val planner = AndroidVisualPlanner()
        val resourceStore = VisualResourceStore()
        val engine = AndroidTextAnimationEngine(planner, resourceStore, manualTimeSource, transactionIdSource)
        val runtime = AndroidVisualRuntime(planner, engine, resourceStore)

        val oldRev = makeLayoutRevision(1, "Hello world")
        val newRev = makeLayoutRevision(1, "Hello beautiful world")
        val visualIntent = makeInsertVisualIntent()

        val newSnapshot = makeSnapshot(1L, 0, 5, 15)
        val prepared = engine.prepare(
            visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            newSnapshots = mapOf(0 to newSnapshot)
        )
        engine.submit(prepared)

        manualTimeSource.advanceByMs(1)
        val startFrameTimeMs = manualTimeSource.nowNanos() / 1_000_000

        val startLayout = makeLayout("Hello beautiful world")
        runtime.tick(
            startFrameTimeMs, startLayout, newRev, emptyList(),
            EDITOR_WIDTH, EDITOR_HEIGHT, 0f, 0f,
            true, false, 15, 15, 15
        )

        val midFrameTimeMs = startFrameTimeMs + DURATION_MS / 2
        manualTimeSource.advanceTo(midFrameTimeMs * 1_000_000L)

        val frameState = runtime.tick(
            midFrameTimeMs, startLayout, newRev, emptyList(),
            EDITOR_WIDTH, EDITOR_HEIGHT, 0f, 0f,
            true, false, 15, 15, 15
        )
        assertNotNull("tick must return FrameState at 50%", frameState)
        assertFalse(
            "tick must set completeAfterDraw=false before animation end",
            frameState!!.completeAfterDraw
        )
    }

    @Test
    fun completeAfterDraw_ownedResourceCountZeroAfterCompletion() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val planner = AndroidVisualPlanner()
        val resourceStore = VisualResourceStore()
        val engine = AndroidTextAnimationEngine(planner, resourceStore, manualTimeSource, transactionIdSource)
        val runtime = AndroidVisualRuntime(planner, engine, resourceStore)

        val oldRev = makeLayoutRevision(1, "Hello world")
        val newRev = makeLayoutRevision(1, "Hello beautiful world")
        val visualIntent = makeInsertVisualIntent()

        val newSnapshot = makeSnapshot(1L, 0, 5, 15)
        val prepared = engine.prepare(
            visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            newSnapshots = mapOf(0 to newSnapshot)
        )
        engine.submit(prepared)

        manualTimeSource.advanceByMs(1)
        val startFrameTimeMs = manualTimeSource.nowNanos() / 1_000_000

        val startLayout = makeLayout("Hello beautiful world")
        runtime.tick(
            startFrameTimeMs, startLayout, newRev, emptyList(),
            EDITOR_WIDTH, EDITOR_HEIGHT, 0f, 0f,
            true, false, 15, 15, 15
        )

        val endFrameTimeMs = startFrameTimeMs + DURATION_MS
        manualTimeSource.advanceTo(endFrameTimeMs * 1_000_000L)

        val preDrawSnapshot = engine.captureStateSnapshot(endFrameTimeMs)
        assertNotNull(preDrawSnapshot)
        assertTrue(
            "Before completion, ownedResourceCount > 0",
            preDrawSnapshot!!.ownedResourceCount > 0
        )

        val completed = engine.completeIfFinished(endFrameTimeMs)
        assertTrue("completeIfFinished must return true at 100%", completed)

        val afterDrawSnapshot = engine.captureStateSnapshot(endFrameTimeMs)
        assertNotNull(
            "After completion, terminal state must remain queryable (Completed)",
            afterDrawSnapshot
        )
        assertEquals(
            "After completion, transaction state must be Completed",
            TransactionState.Completed,
            afterDrawSnapshot!!.transactionState
        )
        assertEquals(
            "After completion, ownedResourceCount must be 0 (all Bitmaps released)",
            0,
            afterDrawSnapshot.ownedResourceCount
        )
    }
}
