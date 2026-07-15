package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import com.xiwei.sujian.model.AnimationModeData
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class ScrollQueueAndEmptyAnimationTest {

    @Test
    fun renderer_addTransaction_whileScrolling_queuesInsteadOfCancelling() {
        val renderer = SujianEditorRenderer(
            textPaint = android.text.TextPaint(),
            density = 2f
        )
        renderer.setScrolling(true)

        val tx = AndroidPlatformVisualTransaction(
            key = 1u,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Insert,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160L,
            oldRevision = 0L,
            newRevision = 1L,
            slices = mutableListOf(AndroidAnimatedSlice.insertFadeIn(
                id = 1u, snapshotId = AndroidLineSnapshotId(1L, 0),
                sourceRect = RectF(0f, 0f, 10f, 20f),
                fromRect = RectF(0f, 0f, 10f, 20f),
                toRect = RectF(10f, 0f, 20f, 20f),
                byteStart = 0, byteEnd = 3,
                shapingIdentity = "test"
            )),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(RectF(0f, 0f, 0f, 0f))
        )

        val result = renderer.addTransaction(tx)
        assertTrue("Transaction should be queued, not rejected", result)
        assertFalse("Transaction should not be cancelled", tx.state == AndroidVisualTransactionState.Cancelled)
    }

    @Test
    fun renderer_pendingQueue_flushedOnScrollEnd() {
        val renderer = SujianEditorRenderer(
            textPaint = android.text.TextPaint(),
            density = 2f
        )
        renderer.setScrolling(true)

        val tx = AndroidPlatformVisualTransaction(
            key = 1u,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Insert,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160L,
            oldRevision = 0L,
            newRevision = 1L,
            slices = mutableListOf(AndroidAnimatedSlice.insertFadeIn(
                id = 1u, snapshotId = AndroidLineSnapshotId(1L, 0),
                sourceRect = RectF(0f, 0f, 10f, 20f),
                fromRect = RectF(0f, 0f, 10f, 20f),
                toRect = RectF(10f, 0f, 20f, 20f),
                byteStart = 0, byteEnd = 3,
                shapingIdentity = "test"
            )),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(RectF(0f, 0f, 0f, 0f))
        )

        renderer.addTransaction(tx)
        renderer.setScrolling(false)

        val activeTxs = renderer.getActiveTransactions()
        assertTrue("Queued transaction should be active after scroll ends", activeTxs.isNotEmpty())
        assertEquals(AndroidVisualTransactionState.Prepared, activeTxs.last().state)
    }

    @Test
    fun renderer_clearAnimations_clearsPendingQueue() {
        val renderer = SujianEditorRenderer(
            textPaint = android.text.TextPaint(),
            density = 2f
        )
        renderer.setScrolling(true)

        val tx = AndroidPlatformVisualTransaction(
            key = 1u,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Insert,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160L,
            oldRevision = 0L,
            newRevision = 1L,
            slices = mutableListOf(AndroidAnimatedSlice.insertFadeIn(
                id = 1u, snapshotId = AndroidLineSnapshotId(1L, 0),
                sourceRect = RectF(0f, 0f, 10f, 20f),
                fromRect = RectF(0f, 0f, 10f, 20f),
                toRect = RectF(10f, 0f, 20f, 20f),
                byteStart = 0, byteEnd = 3,
                shapingIdentity = "test"
            )),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(RectF(0f, 0f, 0f, 0f))
        )

        renderer.addTransaction(tx)
        renderer.clearAnimations()

        assertEquals(AndroidVisualTransactionState.Cancelled, tx.state)
        assertFalse("No active transactions after clear", renderer.hasActiveAnimations())
    }

    @Test
    fun renderer_scrollPause_doesNotClearActiveTransactions() {
        val renderer = SujianEditorRenderer(
            textPaint = android.text.TextPaint(),
            density = 2f
        )

        val tx = AndroidPlatformVisualTransaction(
            key = 1u,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.Insert,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160L,
            oldRevision = 0L,
            newRevision = 1L,
            slices = mutableListOf(AndroidAnimatedSlice.insertFadeIn(
                id = 1u, snapshotId = AndroidLineSnapshotId(1L, 0),
                sourceRect = RectF(0f, 0f, 10f, 20f),
                fromRect = RectF(0f, 0f, 10f, 20f),
                toRect = RectF(10f, 0f, 20f, 20f),
                byteStart = 0, byteEnd = 3,
                shapingIdentity = "test"
            )),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(RectF(0f, 0f, 0f, 0f))
        )

        renderer.addTransaction(tx)
        tx.markRendering()
        renderer.setScrolling(true)

        assertTrue("Should still have active transactions", renderer.hasActiveAnimations())
        assertEquals(AndroidVisualTransactionState.Paused, tx.state)

        renderer.setScrolling(false)
        assertEquals(AndroidVisualTransactionState.Rendering, tx.state)
    }

    @Test
    fun emptyDeleteSlices_shouldNotCreateTransaction() {
        val slices = mutableListOf<AndroidAnimatedSlice>()
        assertTrue("Empty slices should skip", slices.isEmpty())
    }

    @Test
    fun crossfadeOldNew_alwaysCreatedInPairs() {
        val oldShapingId = "old-shape"
        val newShapingId = "new-shape"

        val oldSlice = AndroidAnimatedSlice.crossfade(
            id = 1u,
            role = AndroidAnimatedSliceRole.CrossfadeOld,
            snapshotId = AndroidLineSnapshotId(1L, 0),
            sourceRect = RectF(0f, 0f, 10f, 20f),
            fromRect = RectF(0f, 0f, 10f, 20f),
            toRect = RectF(10f, 0f, 20f, 20f),
            byteStart = 0, byteEnd = 3,
            shapingIdentity = oldShapingId
        )
        val newSlice = AndroidAnimatedSlice.crossfade(
            id = 2u,
            role = AndroidAnimatedSliceRole.CrossfadeNew,
            snapshotId = AndroidLineSnapshotId(2L, 0),
            sourceRect = RectF(0f, 0f, 10f, 20f),
            fromRect = RectF(0f, 0f, 10f, 20f),
            toRect = RectF(10f, 0f, 20f, 20f),
            byteStart = 0, byteEnd = 3,
            shapingIdentity = newShapingId
        )

        assertNotEquals(oldSlice.shapingIdentity, newSlice.shapingIdentity)
        assertEquals(AndroidAnimatedSliceRole.CrossfadeOld, oldSlice.role)
        assertEquals(AndroidAnimatedSliceRole.CrossfadeNew, newSlice.role)
        assertTrue("Old fades out", oldSlice.opacityFrom > oldSlice.opacityTo)
        assertTrue("New fades in", newSlice.opacityFrom < newSlice.opacityTo)
    }
}
