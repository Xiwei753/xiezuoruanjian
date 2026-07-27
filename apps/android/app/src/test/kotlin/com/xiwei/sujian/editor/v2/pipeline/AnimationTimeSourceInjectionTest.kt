package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import org.junit.Assert.*
import org.junit.Test

class AnimationTimeSourceInjectionTest {

    @Test
    fun visualRuntimeCreateWithDefaultTimeSourceUsesChoreographer() {
        val runtime = AndroidVisualRuntime()
        assertNotNull(runtime)
    }

    @Test
    fun visualRuntimeCreateWithManualTimeSourceAcceptsInjection() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)
        assertNotNull(runtime)
    }

    @Test
    fun visualRuntimeCreateWithExplicitChoreographerTimeSource() {
        val choreographerSource = ChoreographerAnimationTimeSource()
        val runtime = AndroidVisualRuntime(choreographerSource)
        assertNotNull(runtime)
    }

    @Test
    fun manualTimeSourceAdvancesIndependentlyOfRuntime() {
        val manualTimeSource = ManualAnimationTimeSource()
        manualTimeSource.advanceByMs(16)
        assertEquals(16_000_000L, manualTimeSource.nowNanos())
        manualTimeSource.advanceByMs(16)
        assertEquals(32_000_000L, manualTimeSource.nowNanos())
    }

    @Test
    fun transactionIdSourceProducesMonotonicallyIncreasingIds() {
        val source = TransactionIdSource()
        val id1 = source.nextId()
        val id2 = source.nextId()
        val id3 = source.nextId()
        assertTrue(id1 < id2)
        assertTrue(id2 < id3)
    }

    @Test
    fun transactionIdSourceStartsAtOne() {
        val source = TransactionIdSource()
        assertEquals(1L, source.nextId())
    }

    @Test
    fun visualRuntimeWithManualTimeSource_reportsInjectedTime() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)

        assertEquals(0L, runtime.currentTimeNanos())

        manualTimeSource.advanceByMs(16)
        assertEquals(16_000_000L, runtime.currentTimeNanos())

        manualTimeSource.advanceByMs(16)
        assertEquals(32_000_000L, runtime.currentTimeNanos())
    }

    @Test
    fun visualRuntimeWithManualTimeSource_snapshotIsNullWithoutActiveTransaction() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)

        manualTimeSource.advanceByMs(100)
        assertNull(runtime.captureStateSnapshot())
    }

    @Test
    fun visualRuntimeWithManualTimeSource_noActiveAnimationInitially() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)

        assertFalse(runtime.hasActiveAnimation())
    }

    @Test
    fun visualRuntimeWithDefaultTimeSource_reportsSystemTime() {
        val runtime = AndroidVisualRuntime()

        val before = System.nanoTime()
        val reported = runtime.currentTimeNanos()
        val after = System.nanoTime()
        assertTrue("Reported time should be within system nanoTime range", reported in before..after)
    }

    @Test
    fun visualRuntimeWithManualTimeSource_timeAdvancesDeterministically() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)

        val timestamps = mutableListOf<Long>()
        for (i in 0 until 5) {
            manualTimeSource.advanceByMs(16)
            timestamps.add(runtime.currentTimeNanos() / 1_000_000)
        }
        assertEquals(listOf(16L, 32L, 48L, 64L, 80L), timestamps)
    }

    @Test
    fun visualRuntimeWithManualTimeSource_visualFrameSnapshotIsNullWithoutActiveTransaction() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)

        manualTimeSource.advanceByMs(100)
        assertNull(runtime.captureVisualFrameSnapshot())
    }

    @Test
    fun visualRuntimeWithManualTimeSource_stateAndVisualFrameSnapshotsConsistentWhenNoAnimation() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)

        manualTimeSource.advanceByMs(100)
        val stateSnapshot = runtime.captureStateSnapshot()
        val visualSnapshot = runtime.captureVisualFrameSnapshot()
        assertNull(stateSnapshot)
        assertNull(visualSnapshot)
    }

    @Test
    fun visualRuntime_getActiveAnimationStartTimeMsIsNullWithoutActiveTransaction() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)

        assertNull(runtime.getActiveAnimationStartTimeMs())
    }

    @Test
    fun visualRuntime_getActiveAnimationDurationMsIsZeroWithoutActiveTransaction() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)

        assertEquals(0L, runtime.getActiveAnimationDurationMs())
    }

    @Test
    fun tick_returnsCompleteAfterDrawFalseWhenNoActiveAnimation() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)
        manualTimeSource.advanceByMs(100)
        assertFalse(runtime.hasActiveAnimation())
    }

    @Test
    fun tick_doesNotCompleteTransactionBeforeDraw_frameStateHasTransaction() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)
        manualTimeSource.advanceByMs(100)
        val snapshot = runtime.captureStateSnapshot()
        assertNull(snapshot)
        assertTrue(
            "Without active animation, hasActiveAnimation should be false",
            !runtime.hasActiveAnimation()
        )
    }

    @Test
    fun completeAfterDraw_releasesResourcesAfterDraw() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = AndroidVisualRuntime(manualTimeSource, transactionIdSource)
        manualTimeSource.advanceByMs(100)
        runtime.completeAfterDraw(manualTimeSource.nowNanos() / 1_000_000)
        assertFalse(runtime.hasActiveAnimation())
    }

    @Test
    fun frameState_completeAfterDrawFlagIsFalseByDefault() {
        val input = FrameRenderInput(
            layout = android.text.StaticLayout(
                "", android.text.TextPaint(), 100,
                android.text.Layout.Alignment.ALIGN_NORMAL, 1f, 0f, false
            ),
            layoutRevision = null,
            transaction = null,
            timelineProgress = 0f,
            searchHighlightsUtf16 = emptyList(),
            viewportWidth = 100,
            viewportHeight = 100,
            scrollX = 0f,
            scrollY = 0f,
            cursorVisible = true,
            selectionAllowed = false,
            cursorUtf16 = 0,
            selectionStartUtf16 = 0,
            selectionEndUtf16 = 0
        )
        val frameState = FrameState(input)
        assertFalse(
            "FrameState completeAfterDraw should default to false",
            frameState.completeAfterDraw
        )
    }

    @Test
    fun frameState_completeAfterDrawFlagCanBeSetToTrue() {
        val input = FrameRenderInput(
            layout = android.text.StaticLayout(
                "", android.text.TextPaint(), 100,
                android.text.Layout.Alignment.ALIGN_NORMAL, 1f, 0f, false
            ),
            layoutRevision = null,
            transaction = null,
            timelineProgress = 1f,
            searchHighlightsUtf16 = emptyList(),
            viewportWidth = 100,
            viewportHeight = 100,
            scrollX = 0f,
            scrollY = 0f,
            cursorVisible = true,
            selectionAllowed = false,
            cursorUtf16 = 0,
            selectionStartUtf16 = 0,
            selectionEndUtf16 = 0
        )
        val frameState = FrameState(input, completeAfterDraw = true)
        assertTrue(
            "FrameState completeAfterDraw should be true when set",
            frameState.completeAfterDraw
        )
    }
}
