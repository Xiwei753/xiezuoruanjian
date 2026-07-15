package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import com.xiwei.sujian.model.AnimationModeData
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

class FakeVisualResource : AndroidLineVisualResource {
    var resourceId: Long = 0
    var released = false
    var releaseCount = 0
        private set

    constructor(id: Long) {
        resourceId = id
    }

    override fun record(layout: android.text.Layout, lineIdx: Int, textPaint: android.graphics.Paint, textColor: Int, scrollX: Int, scrollY: Int) {}
    override fun drawSlice(canvas: android.graphics.Canvas, sourceRect: RectF, destinationRect: RectF, alpha: Int, scale: Float) {}
    override fun release() {
        if (released) throw IllegalStateException("Double release of FakeVisualResource $resourceId")
        released = true
        releaseCount++
    }
}

@Suppress("DEPRECATION")
class CompositionOwnershipTest {

    private lateinit var manager: AndroidCompositionManager

    @Before
    fun setUp() {
        manager = AndroidCompositionManager()
    }

    private fun makeRevision(id: Long): AndroidCompositionVisualRevision {
        return AndroidCompositionVisualRevision(
            committedText = "test",
            compositionReplaceRange = HalfOpenRange(0, 0),
            preeditRangeInVirtualText = HalfOpenRange(0, 4),
            preeditText = "test",
            virtualText = "test",
            affectedParagraphRange = HalfOpenRange(0, 1),
            lineSnapshots = emptyList(),
            cursorRect = android.graphics.RectF(),
            decorationRanges = emptyList(),
            revisionId = id,
            sessionId = CompositionSessionId(id)
        )
    }

    private fun makeRevisionWithFakeResources(id: Long, resourceCount: Int): AndroidCompositionVisualRevision {
        val snapshots = (0 until resourceCount).map { i ->
            val fakeResource = FakeVisualResource(id * 100 + i)
            AndroidLineSnapshot(
                id = AndroidLineSnapshotId(id, i),
                revision = id,
                paragraphId = 0,
                visualLineOrdinal = i,
                documentByteStart = i * 10,
                documentByteEnd = (i + 1) * 10,
                platformTextStart = i * 4,
                platformTextEnd = (i + 1) * 4,
                documentRect = RectF(0f, i * 20f, 200f, (i + 1) * 20f),
                baseline = (i + 1) * 20f - 4f,
                lineImageLocalSize = RectF(0f, 0f, 200f, 20f),
                clusters = listOf(AndroidClusterSnapshot(
                    documentByteStart = i * 10,
                    documentByteEnd = (i + 1) * 10,
                    platformTextStart = i * 4,
                    platformTextEnd = (i + 1) * 4,
                    sourceRectInLineSnapshot = RectF(0f, 0f, 200f, 20f),
                    visualRectInDocument = RectF(0f, i * 20f, 200f, (i + 1) * 20f),
                    textDirection = 0,
                    shapingIdentity = "shape_$i"
                )),
                visualResource = fakeResource
            )
        }
        return AndroidCompositionVisualRevision(
            committedText = "test",
            compositionReplaceRange = HalfOpenRange(0, 0),
            preeditRangeInVirtualText = HalfOpenRange(0, 4),
            preeditText = "test",
            virtualText = "test",
            affectedParagraphRange = HalfOpenRange(0, 0 + 1),
            lineSnapshots = snapshots,
            cursorRect = android.graphics.RectF(),
            decorationRanges = emptyList(),
            revisionId = id,
            sessionId = CompositionSessionId(id)
        )
    }

    @Test
    fun setCurrent_previousReleased() {
        val rev1 = makeRevision(1)
        val rev2 = makeRevision(2)

        manager.setCurrent(rev1)
        manager.setCurrent(rev2)

        assertTrue(rev1.isReleased())
    }

    @Test
    fun takeCurrentForTransaction_transfersOwnership() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val taken = manager.takeCurrentForTransaction(100uL)
        assertNotNull(taken)
        assertEquals(1L, taken!!.revisionId)
        assertTrue(taken.owner is SnapshotOwner.OwnedByTransaction)
        assertEquals(100uL, (taken.owner as SnapshotOwner.OwnedByTransaction).transactionKey)

        assertNull(manager.getCurrent())
    }

    @Test
    fun takeCurrentForTransaction_returnsNullWhenRevisionIsWithActiveTransaction() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        manager.takeCurrentForTransaction(100uL)

        val result = manager.takeCurrentForTransaction(101uL)
        assertNull(result)
        assertEquals(100uL, manager.getActiveTransactionKey())
    }

    @Test
    fun takeCurrentForTransactionTyped_returnsRevisionWithActiveTransaction() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        manager.takeCurrentForTransaction(100uL)

        val result = manager.takeCurrentForTransactionTyped(101uL)
        assertTrue(result is TakeCurrentResult.RevisionWithActiveTransaction)
        assertEquals(100uL, (result as TakeCurrentResult.RevisionWithActiveTransaction).activeTransactionKey)
    }

    @Test
    fun takeCurrentForTransactionTyped_returnsNoRevisionWhenEmpty() {
        val result = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(result is TakeCurrentResult.NoRevisionAvailable)
    }

    @Test
    fun takeCurrentForTransactionTyped_returnsSuccessWhenAvailable() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val result = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(result is TakeCurrentResult.Success)
        assertEquals(1L, (result as TakeCurrentResult.Success).revision.revisionId)
    }

    @Test
    fun takeCurrentForTransaction_consecutivePreeditReturnsNullWhenRevisionWithActiveTransaction() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        manager.takeCurrentForTransaction(100uL)

        val result = manager.takeCurrentForTransaction(101uL)
        assertNull(result)
    }

    @Test
    fun takeCurrentForTransaction_returnsNullWhenNoRevision() {
        val result = manager.takeCurrentForTransaction(100uL)
        assertNull(result)
    }

    @Test
    fun clear_releasesSessionOwnedRevisions() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        manager.clear()

        assertNull(manager.getCurrent())
        assertTrue(rev1.isReleased())
    }

    @Test
    fun clear_doesNotReleaseTransferredRevisions() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val taken = manager.takeCurrentForTransaction(100uL)
        assertNotNull(taken)
        assertFalse(taken!!.isReleased())

        manager.clear()

        assertNull(manager.getCurrent())
        assertFalse(taken.isReleased())
    }

    @Test
    fun ownershipChain_sessionToTransactionToRelease() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val taken = manager.takeCurrentForTransaction(100uL)
        assertNotNull(taken)

        manager.clear()

        assertNull(manager.getCurrent())
        assertFalse(taken!!.isReleased())

        taken.release(SnapshotOwner.OwnedByTransaction(100uL))
        assertTrue(taken.isReleased())
    }

    @Test(expected = IllegalStateException::class)
    fun doubleRelease_throws() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        manager.clear()
        rev1.release(SnapshotOwner.OwnedBySession(CompositionSessionId(1)))
    }

    @Test(expected = IllegalStateException::class)
    fun doubleReleaseViaTransaction_throws() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        val taken = manager.takeCurrentForTransaction(100uL)
        assertNotNull(taken)
        taken!!.release(SnapshotOwner.OwnedByTransaction(100uL))
        taken.release(SnapshotOwner.OwnedByTransaction(100uL))
    }

    @Test(expected = IllegalStateException::class)
    fun wrongOwnerRelease_throws() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        rev1.release(SnapshotOwner.OwnedByTransaction(999uL))
    }

    @Test
    fun consecutiveUpdates_noLeak() {
        for (i in 1..100) {
            val rev = makeRevision(i.toLong())
            manager.setCurrent(rev)
        }
        val lastRev = manager.getCurrent()
        assertNotNull(lastRev)
        assertFalse(lastRev!!.isReleased())

        manager.clear()
        assertTrue(lastRev.isReleased())
        assertNull(manager.getCurrent())
    }

    @Test
    fun returnFromTransaction_newRevisionReturnsToSession() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val taken = manager.takeCurrentForTransaction(100uL)
        assertNotNull(taken)

        val rev2 = makeRevision(2)
        rev2.transferToTransaction(100uL)
        val gen = manager.getGeneration()
        manager.returnFromTransaction(rev2, 100uL, gen)

        assertNotNull(manager.getCurrent())
        assertEquals(2L, manager.getCurrent()!!.revisionId)
    }

    @Test
    fun returnFromTransaction_nextCompositionUpdateCanReadReturnedRevision() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)

        val taken = manager.takeCurrentForTransaction(100uL)
        val rev2 = makeRevision(2)
        rev2.transferToTransaction(100uL)
        val gen = manager.getGeneration()
        manager.returnFromTransaction(rev2, 100uL, gen)

        val takenAgain = manager.takeCurrentForTransaction(101uL)
        assertNotNull(takenAgain)
        assertEquals(2L, takenAgain!!.revisionId)
    }

    @Test
    fun snapshotOwner_hasSessionId() {
        val sessionId = CompositionSessionId(42)
        val owner = SnapshotOwner.OwnedBySession(sessionId)
        assertTrue(owner is SnapshotOwner.OwnedBySession)
        assertEquals(42, (owner as SnapshotOwner.OwnedBySession).sessionId.value)
    }

    @Test
    fun snapshotOwner_hasTransactionKey() {
        val owner = SnapshotOwner.OwnedByTransaction(123uL)
        assertTrue(owner is SnapshotOwner.OwnedByTransaction)
        assertEquals(123uL, (owner as SnapshotOwner.OwnedByTransaction).transactionKey)
    }

    @Test
    fun transactionHoldsRevisionOwnership() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        val prevRevision = manager.takeCurrentForTransaction(100uL)
        assertNotNull(prevRevision)
        assertFalse(prevRevision!!.isReleased())

        val tx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 0,
            newRevision = 1,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRevision
        )

        assertFalse(prevRevision.isReleased())
        tx.cancel("test")
        assertTrue(prevRevision.isReleased())
    }

    @Test
    fun transactionComplete_withOwnedNewRevision_returnsToSession() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        val prevRevision = manager.takeCurrentForTransaction(100uL)
        val newRevision = makeRevision(2)
        newRevision.transferToTransaction(100uL)

        var returnedRevision: OwnedVisualRevision? = null
        val tx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 0,
            newRevision = 1,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRevision,
            ownedNewRevision = newRevision,
            onTransactionComplete = { rev, key ->
                returnedRevision = rev as? AndroidCompositionVisualRevision as? AndroidCompositionVisualRevision
                ReturnFromTransactionResult.Accepted
            }
        )

        tx.complete()
        assertNotNull(returnedRevision)
        assertEquals(2L, (returnedRevision!! as AndroidCompositionVisualRevision).revisionId)
        assertFalse((returnedRevision!! as AndroidCompositionVisualRevision).isReleased())
    }

    @Test(expected = IllegalStateException::class)
    fun transactionComplete_thenCancel_throws() {
        val tx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF())
        )
        tx.complete()
        tx.cancel("should_fail")
    }

    @Test
    fun detachOldRevisionForRebase_returnsRevision() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        val oldRev = manager.takeCurrentForTransaction(100uL)

        val tx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = oldRev
        )

        val detached = tx.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
        assertNotNull(detached)
        assertNull(tx.ownedOldRevision)
    }

    @Test
    fun rebase_oldTransactionCannotReleaseTransferredResources() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val oldRev = manager.takeCurrentForTransaction(100uL)

        val tx1 = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = oldRev
        )

        val transferred = tx1.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
        assertNotNull(transferred)

        tx1.cancel("rebased")
        assertNull(tx1.ownedOldRevision)
        assertFalse(transferred!!.isReleased())
        for (snap in transferred.lineSnapshots) {
            assertFalse(snap.isReleased())
        }
    }

    @Test
    fun buildVirtualText_zeroLengthReplaceRange() {
        val result = manager.buildVirtualText("你好世界", HalfOpenRange(2, 2), "abc")
        assertEquals("你好abc世界", result)
    }

    @Test
    fun buildVirtualText_nonZeroReplaceRange() {
        val result = manager.buildVirtualText("你好世界", HalfOpenRange(1, 3), "abc")
        assertEquals("你abc界", result)
    }

    @Test
    fun buildVirtualText_emptyPreedit() {
        val result = manager.buildVirtualText("你好世界", HalfOpenRange(2, 2), "")
        assertEquals("你好世界", result)
    }

    @Test
    fun buildVirtualText_preeditLongerThanReplaceRange() {
        val result = manager.buildVirtualText("你好世界", HalfOpenRange(2, 2), "abcdefghij")
        assertEquals("你好abcdefghij世界", result)
    }

    @Test
    fun buildVirtualText_preeditShorterThanReplaceRange() {
        val result = manager.buildVirtualText("你好世界", HalfOpenRange(1, 3), "X")
        assertEquals("你X界", result)
    }

    @Test
    fun rebaseWithFakeResources_oldTransactionCannotReleaseTransferred() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val oldRev = manager.takeCurrentForTransaction(100uL)

        val tx1 = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = oldRev
        )

        val transferred = tx1.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
        assertNotNull(transferred)
        assertFalse(transferred!!.isReleased())

        tx1.cancel("rebased")
        assertFalse(transferred.isReleased())
        for (snap in transferred.lineSnapshots) {
            assertFalse(snap.isReleased())
        }
    }

    @Test
    fun rebaseWithFakeResources_newRevisionTransfer() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val oldRev = manager.takeCurrentForTransaction(100uL)
        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(100uL)

        val tx1 = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = oldRev,
            ownedNewRevision = newRev
        )

        val detachedOld = tx1.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
        val detachedNew = tx1.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
        assertNotNull(detachedNew)
        assertFalse(detachedNew!!.isReleased())

        tx1.cancel("rebased")
        assertFalse(detachedNew.isReleased())
        for (snap in detachedNew.lineSnapshots) {
            assertFalse(snap.isReleased())
        }

        assertNotNull(detachedOld)
        assertFalse(detachedOld!!.isReleased())
        detachedOld.release(SnapshotOwner.OwnedByTransaction(100uL))
        assertTrue(detachedOld.isReleased())
    }

    @Test
    fun transactionComplete_newRevisionReturnsToSession_withFakeResources() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val prevRevision = manager.takeCurrentForTransaction(100uL)
        val newRevision = makeRevisionWithFakeResources(2, 2)
        newRevision.transferToTransaction(100uL)

        var returnedRevision: OwnedVisualRevision? = null
        val tx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 0,
            newRevision = 1,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRevision,
            ownedNewRevision = newRevision,
            onTransactionComplete = { rev, key ->
                returnedRevision = rev as? AndroidCompositionVisualRevision as? AndroidCompositionVisualRevision
                ReturnFromTransactionResult.Accepted
            }
        )

        tx.complete()
        assertNotNull(returnedRevision)
        assertFalse((returnedRevision!! as AndroidCompositionVisualRevision).isReleased())
        for (snap in (returnedRevision!! as AndroidCompositionVisualRevision).lineSnapshots) {
            assertFalse(snap.isReleased())
        }
    }

    @Test
    fun consecutiveUpdatesWithFakeResources_100times_noLeak() {
        for (i in 1..100) {
            val rev = makeRevisionWithFakeResources(i.toLong(), 2)
            manager.setCurrent(rev)
        }
        val lastRev = manager.getCurrent()
        assertNotNull(lastRev)
        assertFalse(lastRev!!.isReleased())
        for (snap in lastRev.lineSnapshots) {
            assertFalse(snap.isReleased())
        }

        manager.clear()
        assertTrue(lastRev.isReleased())
    }

    @Test
    fun fakeVisualResource_doubleRelease_throws() {
        val resource = FakeVisualResource(1)
        resource.release()
        try {
            resource.release()
            fail("Expected IllegalStateException")
        } catch (_: IllegalStateException) {
        }
    }

    @Test
    fun consecutiveTransactionChain_100times_noLeak() {
        val allResources = mutableListOf<FakeVisualResource>()
        val sessionId = CompositionSessionId(1)

        var currentRev = makeRevisionWithFakeResources(1, 2)
        allResources.addAll(currentRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
        manager.setCurrent(currentRev)

        for (i in 2..100L) {
            val txKey = i.toULong()
            val prevRev = manager.takeCurrentForTransaction(txKey)
            assertNotNull(prevRev)

            val newRev = makeRevisionWithFakeResources(i, 2)
            allResources.addAll(newRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
            newRev.transferToTransaction(txKey)

            var returnedRev: OwnedVisualRevision? = null
            val tx = AndroidPlatformVisualTransaction(
                key = txKey,
                state = AndroidVisualTransactionState.Pending,
                operationKind = AndroidVisualOperationKind.CompositionUpdate,
                animationMode = AnimationModeData.GlyphAnimation,
                durationMs = 160,
                oldRevision = i - 1,
                newRevision = i,
                slices = mutableListOf(),
                staticLinePatches = mutableListOf(),
                decorationSlices = mutableListOf(),
                cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
                ownedOldRevision = prevRev,
                ownedNewRevision = newRev,
                onTransactionComplete = { rev, key ->
                    returnedRev = rev as? AndroidCompositionVisualRevision
                    ReturnFromTransactionResult.Accepted
            }
            )

            tx.complete()
            assertNotNull(returnedRev)
            assertFalse((returnedRev!! as AndroidCompositionVisualRevision).isReleased())
            manager.returnFromTransaction(returnedRev!! as AndroidCompositionVisualRevision, txKey, manager.getGeneration())
            currentRev = returnedRev as AndroidCompositionVisualRevision
        }

        val finalRev = manager.getCurrent()
        assertNotNull(finalRev)
        assertEquals(100L, finalRev!!.revisionId)
        assertFalse(finalRev.isReleased())

        val releasedCount = allResources.count { it.released }
        val unreleasedCount = allResources.count { !it.released }
        assertEquals(2, unreleasedCount)

        manager.clear()
        assertTrue(finalRev.isReleased())
        assertEquals(allResources.size, allResources.count { it.released })
    }

    @Test
    fun rebaseTransactionChain_detachedOldRevisionReleased() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val oldRev = manager.takeCurrentForTransaction(100uL)
        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(100uL)

        val tx1 = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = oldRev,
            ownedNewRevision = newRev
        )

        val detachedOld = tx1.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
        val detachedNew = tx1.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision

        assertNotNull(detachedOld)
        assertNotNull(detachedNew)
        assertFalse(detachedOld!!.isReleased())
        assertFalse(detachedNew!!.isReleased())

        detachedOld.release(SnapshotOwner.OwnedByTransaction(100uL))
        assertTrue(detachedOld.isReleased())

        tx1.cancel("rebased")
        assertFalse(detachedNew.isReleased())
    }

    @Test
    fun returnFromTransaction_wrongTransactionKey_rejectedAndReleased() {
        val rev1 = makeRevision(1)
        manager.setCurrent(rev1)
        manager.takeCurrentForTransaction(100uL)

        val rev2 = makeRevision(2)
        val result = manager.returnFromTransaction(rev2, 999uL, manager.getGeneration())

        assertTrue(result is ReturnFromTransactionResult.RejectedStale)
        assertTrue(rev2.isReleased())
        assertNull(manager.getCurrent())
        assertEquals(100uL, manager.getActiveTransactionKey())
    }

    @Test
    fun rebaseTransactionChain_withFakeResources_noLeakOrUseAfterRelease() {
        val allResources = mutableListOf<FakeVisualResource>()
        val sessionId = CompositionSessionId(1)

        var currentRev = makeRevisionWithFakeResources(1, 2)
        allResources.addAll(currentRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
        manager.setCurrent(currentRev)

        for (i in 2..100L) {
            val txKey = i.toULong()
            val prevRev = manager.takeCurrentForTransaction(txKey)
            assertNotNull(prevRev)

            val newRev = makeRevisionWithFakeResources(i, 2)
            allResources.addAll(newRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
            newRev.transferToTransaction(txKey)

            var returnedRev: OwnedVisualRevision? = null
            val tx = AndroidPlatformVisualTransaction(
                key = txKey,
                state = AndroidVisualTransactionState.Rendering,
                operationKind = AndroidVisualOperationKind.CompositionUpdate,
                animationMode = AnimationModeData.GlyphAnimation,
                durationMs = 160,
                oldRevision = i - 1,
                newRevision = i,
                slices = mutableListOf(),
                staticLinePatches = mutableListOf(),
                decorationSlices = mutableListOf(),
                cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
                ownedOldRevision = prevRev,
                ownedNewRevision = newRev,
                onTransactionComplete = { rev, key ->
                    returnedRev = rev as? AndroidCompositionVisualRevision
                    ReturnFromTransactionResult.Accepted
            }
            )

            if (i % 10 == 0L) {
                val detachedOld = tx.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
                val detachedNew = tx.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
                assertNotNull(detachedOld)
                assertNotNull(detachedNew)
                detachedOld!!.release(SnapshotOwner.OwnedByTransaction(txKey))
                manager.returnFromTransaction(detachedNew!!, txKey, manager.getGeneration())
                currentRev = detachedNew
                tx.cancel("rebased")
            } else {
                tx.complete()
                assertNotNull(returnedRev)
                assertFalse((returnedRev!! as AndroidCompositionVisualRevision).isReleased())
                manager.returnFromTransaction(returnedRev!! as AndroidCompositionVisualRevision, txKey, manager.getGeneration())
                currentRev = returnedRev!! as AndroidCompositionVisualRevision
            }
        }

        val finalRev = manager.getCurrent()
        assertNotNull(finalRev)
        assertEquals(100L, finalRev!!.revisionId)
        assertFalse(finalRev.isReleased())

        val releasedCount = allResources.count { it.released }
        val unreleasedCount = allResources.count { !it.released }
        assertEquals(2, unreleasedCount)

        manager.clear()
        assertTrue(finalRev.isReleased())
        assertEquals(allResources.size, allResources.count { it.released })
    }

    @Test
    fun commitOrCancel_takesOverActiveCompositionUpdate() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val prevRev = manager.takeCurrentForTransaction(100uL)
        assertNotNull(prevRev)

        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(100uL)
        val tx1 = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRev,
            ownedNewRevision = newRev
        )

        val takenNewRev = tx1.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
        assertNotNull(takenNewRev)
        assertFalse(takenNewRev!!.isReleased())

        tx1.onTransactionComplete = null
        tx1.cancel("superseded_by_commit_cancel")

        assertFalse(takenNewRev.isReleased())
        for (snap in takenNewRev.lineSnapshots) {
            assertFalse(snap.isReleased())
        }

        takenNewRev.release(SnapshotOwner.OwnedByTransaction(100uL))
        assertTrue(takenNewRev.isReleased())
    }

    @Test
    fun consecutiveCompositionUpdateChain_withFakeResources_100times_noLeak() {
        val allResources = mutableListOf<FakeVisualResource>()
        val sessionId = CompositionSessionId(1)

        var currentRev = makeRevisionWithFakeResources(1, 2)
        allResources.addAll(currentRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
        manager.setCurrent(currentRev)

        for (i in 2..100L) {
            val txKey = i.toULong()

            val prevRev = manager.takeCurrentForTransaction(txKey)
            if (prevRev == null) {
                val activeTxKey = manager.getActiveTransactionKey()
                if (activeTxKey != null) {
                    val fakeOldRev = makeRevisionWithFakeResources(i - 1, 2)
                    allResources.addAll(fakeOldRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
                    fakeOldRev.transferToTransaction(activeTxKey)
                    val fakeActiveTx = AndroidPlatformVisualTransaction(
                        key = activeTxKey,
                        state = AndroidVisualTransactionState.Rendering,
                        operationKind = AndroidVisualOperationKind.CompositionUpdate,
                        animationMode = AnimationModeData.GlyphAnimation,
                        durationMs = 160,
                        oldRevision = i - 1,
                        newRevision = i,
                        slices = mutableListOf(),
                        staticLinePatches = mutableListOf(),
                        decorationSlices = mutableListOf(),
                        cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
                        ownedOldRevision = fakeOldRev,
                        ownedNewRevision = currentRev
                    )
                    val takenNew = fakeActiveTx.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
                    val detachedOld = fakeActiveTx.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
                    fakeActiveTx.onTransactionComplete = null
                    fakeActiveTx.cancel("superseded")
                    if (detachedOld != null) {
                        detachedOld.release(detachedOld.owner)
                    }
                    currentRev = takenNew ?: continue
                } else {
                    continue
                }
            } else {
                currentRev = prevRev
            }

            val newRev = makeRevisionWithFakeResources(i, 2)
            allResources.addAll(newRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
            newRev.transferToTransaction(txKey)

            var returnedRev: OwnedVisualRevision? = null
            val tx = AndroidPlatformVisualTransaction(
                key = txKey,
                state = AndroidVisualTransactionState.Rendering,
                operationKind = AndroidVisualOperationKind.CompositionUpdate,
                animationMode = AnimationModeData.GlyphAnimation,
                durationMs = 160,
                oldRevision = i - 1,
                newRevision = i,
                slices = mutableListOf(),
                staticLinePatches = mutableListOf(),
                decorationSlices = mutableListOf(),
                cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
                ownedOldRevision = currentRev,
                ownedNewRevision = newRev,
                onTransactionComplete = { rev, key ->
                    returnedRev = rev as? AndroidCompositionVisualRevision
                    ReturnFromTransactionResult.Accepted
            }
            )

            if (i % 7 == 0L) {
                val detachedOld = tx.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
                val detachedNew = tx.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
                assertNotNull(detachedOld)
                assertNotNull(detachedNew)
                detachedOld!!.release(SnapshotOwner.OwnedByTransaction(txKey))
                manager.returnFromTransaction(detachedNew!!, txKey, manager.getGeneration())
                currentRev = detachedNew
                tx.cancel("rebased")
            } else {
                tx.complete()
                assertNotNull(returnedRev)
                assertFalse((returnedRev!! as AndroidCompositionVisualRevision).isReleased())
                manager.returnFromTransaction(returnedRev!! as AndroidCompositionVisualRevision, txKey, manager.getGeneration())
                currentRev = returnedRev!! as AndroidCompositionVisualRevision
            }
        }

        val finalRev = manager.getCurrent()
        assertNotNull(finalRev)
        assertEquals(100L, finalRev!!.revisionId)
        assertFalse(finalRev.isReleased())

        val unreleasedCount = allResources.count { !it.released }
        assertEquals(2, unreleasedCount)

        manager.clear()
        assertTrue(finalRev.isReleased())
        assertEquals(allResources.size, allResources.count { it.released })
    }

    @Test
    fun commitCancel_takesOverActiveTransaction_withFakeResources() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val prevRev = manager.takeCurrentForTransaction(100uL)
        assertNotNull(prevRev)

        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(100uL)
        val activeTx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRev,
            ownedNewRevision = newRev
        )

        val takenNewRev = activeTx.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
        val detachedOldRev = activeTx.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
        activeTx.onTransactionComplete = null
        activeTx.cancel("superseded_by_commit_cancel")

        if (detachedOldRev != null) {
            detachedOldRev.release(detachedOldRev.owner)
        }

        assertNotNull(takenNewRev)
        assertFalse(takenNewRev!!.isReleased())
        for (snap in takenNewRev.lineSnapshots) {
            assertFalse(snap.isReleased())
        }

        val commitTx = AndroidPlatformVisualTransaction(
            key = 200uL,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionCommitOrCancel,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 2,
            newRevision = 3,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = takenNewRev.also { it.reassignToTransaction(200uL) },
            ownedNewRevision = null
        )

        commitTx.complete()
        assertTrue(takenNewRev.isReleased())
    }

    @Test
    fun returnFromTransaction_staleTransactionKey_ignored() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        manager.takeCurrentForTransaction(100uL)

        val rev2 = makeRevisionWithFakeResources(2, 2)
        rev2.transferToTransaction(999uL)

        manager.returnFromTransaction(rev2, 999uL, manager.getGeneration())

        assertNull(manager.getCurrent())
    }

    @Test
    fun returnFromTransaction_staleGeneration_rejectedAndReleased() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        manager.takeCurrentForTransaction(100uL)
        val staleGen = manager.getGeneration()

        manager.clear()

        val rev2 = makeRevisionWithFakeResources(2, 2)
        val result = manager.returnFromTransaction(rev2, 100uL, staleGen)

        assertTrue(result is ReturnFromTransactionResult.RejectedStale)
        assertTrue(rev2.isReleased())
        assertNull(manager.getCurrent())
    }

    @Test
    fun returnFromTransaction_afterManagerCleared_staleRevisionReleased() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        manager.takeCurrentForTransaction(100uL)
        val staleGen = manager.getGeneration()

        manager.clear()

        val rev2 = makeRevisionWithFakeResources(2, 2)
        manager.setCurrent(rev2)

        val staleRev = makeRevisionWithFakeResources(3, 2)
        val result = manager.returnFromTransaction(staleRev, 100uL, staleGen)

        assertTrue(result is ReturnFromTransactionResult.RejectedStale)
        assertTrue(staleRev.isReleased())
        assertEquals(2L, manager.getCurrent()!!.revisionId)
    }

    @Test
    fun reassignToTransaction_changesOwnerTransactionKey() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val taken = manager.takeCurrentForTransaction(100uL)
        assertNotNull(taken)

        taken!!.reassignToTransaction(200uL)
        assertTrue(taken.owner is SnapshotOwner.OwnedByTransaction)
        assertEquals(200uL, (taken.owner as SnapshotOwner.OwnedByTransaction).transactionKey)

        taken.release(SnapshotOwner.OwnedByTransaction(200uL))
        assertTrue(taken.isReleased())
    }

    @Test
    fun fullCompositionUpdateChain_withRebaseAndCommit_noLeak() {
        val allResources = mutableListOf<FakeVisualResource>()
        val sessionId = CompositionSessionId(1)

        var currentRev = makeRevisionWithFakeResources(1, 2)
        allResources.addAll(currentRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
        manager.setCurrent(currentRev)

        for (i in 2..50L) {
            val txKey = i.toULong()

            val prevRev = manager.takeCurrentForTransaction(txKey)
            if (prevRev != null) {
                currentRev = prevRev
            } else {
                val activeTxKey = manager.getActiveTransactionKey()
                if (activeTxKey != null) {
                    val fakeOldRev = makeRevisionWithFakeResources(i - 1, 2)
                    allResources.addAll(fakeOldRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
                    fakeOldRev.transferToTransaction(activeTxKey)
                    val fakeActiveTx = AndroidPlatformVisualTransaction(
                        key = activeTxKey,
                        state = AndroidVisualTransactionState.Rendering,
                        operationKind = AndroidVisualOperationKind.CompositionUpdate,
                        animationMode = AnimationModeData.GlyphAnimation,
                        durationMs = 160,
                        oldRevision = i - 1,
                        newRevision = i,
                        slices = mutableListOf(),
                        staticLinePatches = mutableListOf(),
                        decorationSlices = mutableListOf(),
                        cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
                        ownedOldRevision = fakeOldRev,
                        ownedNewRevision = currentRev
                    )
                    val takenNew = fakeActiveTx.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
                    val detachedOld = fakeActiveTx.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
                    fakeActiveTx.onTransactionComplete = null
                    fakeActiveTx.cancel("superseded")
                    if (detachedOld != null) {
                        detachedOld.release(detachedOld.owner)
                    }
                    currentRev = takenNew ?: continue
                    currentRev.reassignToTransaction(txKey)
                } else {
                    continue
                }
            }

            val newRev = makeRevisionWithFakeResources(i, 2)
            allResources.addAll(newRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
            newRev.transferToTransaction(txKey)

            if (i % 5 == 0L) {
                newRev.release(SnapshotOwner.OwnedByTransaction(txKey))
                val commitTx = AndroidPlatformVisualTransaction(
                    key = txKey,
                    state = AndroidVisualTransactionState.Pending,
                    operationKind = AndroidVisualOperationKind.CompositionCommitOrCancel,
                    animationMode = AnimationModeData.GlyphAnimation,
                    durationMs = 160,
                    oldRevision = i - 1,
                    newRevision = i,
                    slices = mutableListOf(),
                    staticLinePatches = mutableListOf(),
                    decorationSlices = mutableListOf(),
                    cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
                    ownedOldRevision = currentRev,
                    ownedNewRevision = null
                )
                commitTx.complete()
                assertTrue(currentRev.isReleased())
                val nextRev = makeRevisionWithFakeResources(i + 100, 2)
                allResources.addAll(nextRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
                manager.setCurrent(nextRev)
                currentRev = nextRev
            } else {
                var returnedRev: OwnedVisualRevision? = null
                val tx = AndroidPlatformVisualTransaction(
                    key = txKey,
                    state = AndroidVisualTransactionState.Rendering,
                    operationKind = AndroidVisualOperationKind.CompositionUpdate,
                    animationMode = AnimationModeData.GlyphAnimation,
                    durationMs = 160,
                    oldRevision = i - 1,
                    newRevision = i,
                    slices = mutableListOf(),
                    staticLinePatches = mutableListOf(),
                    decorationSlices = mutableListOf(),
                    cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
                    ownedOldRevision = currentRev,
                    ownedNewRevision = newRev,
                    onTransactionComplete = { rev, key ->
                        returnedRev = rev as? AndroidCompositionVisualRevision
                        ReturnFromTransactionResult.Accepted
            }
                )

                if (i % 7 == 0L) {
                    val detachedOld = tx.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
                    val detachedNew = tx.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
                    assertNotNull(detachedOld)
                    assertNotNull(detachedNew)
                    detachedOld!!.release(SnapshotOwner.OwnedByTransaction(txKey))
                    manager.returnFromTransaction(detachedNew!!, txKey, manager.getGeneration())
                    currentRev = detachedNew
                    tx.cancel("rebased")
                } else {
                    tx.complete()
                    assertNotNull(returnedRev)
                    assertFalse((returnedRev!! as AndroidCompositionVisualRevision).isReleased())
                    manager.returnFromTransaction(returnedRev!! as AndroidCompositionVisualRevision, txKey, manager.getGeneration())
                    currentRev = returnedRev!! as AndroidCompositionVisualRevision
                }
            }
        }

        val unreleasedCount = allResources.count { !it.released }
        assertEquals(2, unreleasedCount)

        manager.clear()
        assertEquals(allResources.size, allResources.count { it.released })
    }

    @Test
    fun addTransaction_releasesPreviousOwnedOldRevision() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val prevRev = manager.takeCurrentForTransaction(100uL)
        assertNotNull(prevRev)

        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(100uL)
        val tx1 = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRev,
            ownedNewRevision = newRev
        )

        val detachedNew = tx1.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
        val detachedOld = tx1.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
        tx1.onTransactionComplete = null
        tx1.cancel("rebased")

        if (detachedOld != null) {
            detachedOld.release(detachedOld.owner)
        }

        assertNotNull(detachedNew)
        detachedNew!!.reassignToTransaction(200uL)

        val newRev2 = makeRevisionWithFakeResources(3, 2)
        newRev2.transferToTransaction(200uL)
        val tx2 = AndroidPlatformVisualTransaction(
            key = 200uL,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 2,
            newRevision = 3,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = detachedNew,
            ownedNewRevision = newRev2
        )

        tx2.complete()
        assertTrue(detachedNew.isReleased())
        for (snap in detachedNew.lineSnapshots) {
            assertTrue(snap.isReleased())
        }
    }

    @Test
    fun productionPath_typedTakeResult_handlesAllVariants() {
        val allResources = mutableListOf<FakeVisualResource>()
        val sessionId = CompositionSessionId(1)

        var currentRev = makeRevisionWithFakeResources(1, 2)
        allResources.addAll(currentRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
        manager.setCurrent(currentRev)

        for (i in 2..20L) {
            val txKey = i.toULong()

            val takeResult = manager.takeCurrentForTransactionTyped(txKey)
            var prevRevision: AndroidCompositionVisualRevision? = null
            var prevRevisionFromActiveTransaction = false

            when (takeResult) {
                is TakeCurrentResult.Success -> {
                    prevRevision = takeResult.revision
                }
                is TakeCurrentResult.RevisionWithActiveTransaction -> {
                    prevRevisionFromActiveTransaction = true
                }
                is TakeCurrentResult.NoRevisionAvailable -> {
                }
            }

            if (prevRevision == null && !prevRevisionFromActiveTransaction) {
                val newRev = makeRevisionWithFakeResources(i, 2)
                allResources.addAll(newRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
                manager.setCurrent(newRev)
                currentRev = newRev
                continue
            }

            if (prevRevision == null && prevRevisionFromActiveTransaction) {
                currentRev.reassignToTransaction(txKey)
                prevRevision = currentRev
            }

            val newRev = makeRevisionWithFakeResources(i, 2)
            allResources.addAll(newRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
            newRev.transferToTransaction(txKey)

            var returnedRev: OwnedVisualRevision? = null
            val tx = AndroidPlatformVisualTransaction(
                key = txKey,
                state = AndroidVisualTransactionState.Rendering,
                operationKind = AndroidVisualOperationKind.CompositionUpdate,
                animationMode = AnimationModeData.GlyphAnimation,
                durationMs = 160,
                oldRevision = i - 1,
                newRevision = i,
                slices = mutableListOf(),
                staticLinePatches = mutableListOf(),
                decorationSlices = mutableListOf(),
                cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
                ownedOldRevision = prevRevision,
                ownedNewRevision = newRev,
                onTransactionComplete = { rev, key ->
                    returnedRev = rev as? AndroidCompositionVisualRevision
                    ReturnFromTransactionResult.Accepted
            }
            )

            tx.complete()
            assertNotNull(returnedRev)
            assertFalse((returnedRev!! as AndroidCompositionVisualRevision).isReleased())
            manager.returnFromTransaction(returnedRev!! as AndroidCompositionVisualRevision, txKey, manager.getGeneration())
            currentRev = returnedRev as AndroidCompositionVisualRevision
        }

        val finalRev = manager.getCurrent()
        assertNotNull(finalRev)
        assertEquals(20L, finalRev!!.revisionId)
        assertFalse(finalRev.isReleased())

        val unreleasedCount = allResources.count { !it.released }
        assertEquals(2, unreleasedCount)

        manager.clear()
        assertTrue(finalRev.isReleased())
        assertEquals(allResources.size, allResources.count { it.released })
    }

    @Test
    fun productionPath_typedTakeResult_consecutivePreeditUsesRevisionWithActiveTransaction() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)

        val firstTake = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(firstTake is TakeCurrentResult.Success)
        val takenRev = (firstTake as TakeCurrentResult.Success).revision

        val secondTake = manager.takeCurrentForTransactionTyped(101uL)
        assertTrue(secondTake is TakeCurrentResult.RevisionWithActiveTransaction)
        assertEquals(100uL, (secondTake as TakeCurrentResult.RevisionWithActiveTransaction).activeTransactionKey)

        assertFalse(takenRev.isReleased())
        takenRev.release(SnapshotOwner.OwnedByTransaction(100uL))
        assertTrue(takenRev.isReleased())
    }

    @Test
    fun productionPath_typedTakeResult_noRevisionReturnsNoRevisionAvailable() {
        val takeResult = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(takeResult is TakeCurrentResult.NoRevisionAvailable)
    }

    @Test
    fun reassignActiveTransactionKey_allowsReturnFromTransactionAfterTakeover() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)

        val firstTake = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(firstTake is TakeCurrentResult.Success)
        val takenRev = (firstTake as TakeCurrentResult.Success).revision

        val secondTake = manager.takeCurrentForTransactionTyped(200uL)
        assertTrue(secondTake is TakeCurrentResult.RevisionWithActiveTransaction)

        manager.reassignActiveTransactionKey(200uL)
        takenRev.reassignToTransaction(200uL)

        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(200uL)
        val genBeforeReturn = manager.getGeneration()

        var returnedRev: OwnedVisualRevision? = null
        val tx = AndroidPlatformVisualTransaction(
            key = 200uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = takenRev,
            ownedNewRevision = newRev,
            onTransactionComplete = { rev, _ ->
                returnedRev = rev as? AndroidCompositionVisualRevision as? AndroidCompositionVisualRevision
                ReturnFromTransactionResult.Accepted
            }
        )

        tx.complete()
        assertNotNull(returnedRev)
        assertFalse((returnedRev!! as AndroidCompositionVisualRevision).isReleased())

        val lr = returnedRev!!; manager.returnFromTransaction(lr, 200uL, genBeforeReturn)
        val currentRev = manager.getCurrent()
        assertNotNull(currentRev)
        assertEquals(2L, currentRev!!.revisionId)
        assertFalse(currentRev.isReleased())
    }

    @Test
    fun reassignActiveTransactionKey_withoutReassign_returnFromTransactionFails() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)

        val firstTake = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(firstTake is TakeCurrentResult.Success)

        val secondTake = manager.takeCurrentForTransactionTyped(200uL)
        assertTrue(secondTake is TakeCurrentResult.RevisionWithActiveTransaction)

        val genBeforeReturn = manager.getGeneration()
        val newRev = makeRevisionWithFakeResources(2, 2)

        manager.returnFromTransaction(newRev, 200uL, genBeforeReturn)

        assertNull(manager.getCurrent())
        assertEquals(100uL, manager.getActiveTransactionKey())
    }

    @Test
    fun reassignActiveTransactionKey_staleTransactionKeyCleared() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)

        val firstTake = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(firstTake is TakeCurrentResult.Success)

        val secondTake = manager.takeCurrentForTransactionTyped(200uL)
        assertTrue(secondTake is TakeCurrentResult.RevisionWithActiveTransaction)

        manager.reassignActiveTransactionKey(200uL)
        assertEquals(200uL, manager.getActiveTransactionKey())

        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(200uL)
        manager.returnFromTransaction(newRev, 200uL, manager.getGeneration())

        assertNull(manager.getActiveTransactionKey())
        assertNotNull(manager.getCurrent())
    }

    @Test
    fun compositionRevision_mustTransferToTransactionBeforeReturnFromTransaction() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)

        val takeResult = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(takeResult is TakeCurrentResult.Success)

        val newRev = makeRevisionWithFakeResources(2, 2)
        assertEquals(SnapshotOwner.OwnedBySession(CompositionSessionId(2)), newRev.owner)

        newRev.transferToTransaction(100uL)
        assertTrue(newRev.owner is SnapshotOwner.OwnedByTransaction)
        assertEquals(100uL, (newRev.owner as SnapshotOwner.OwnedByTransaction).transactionKey)

        val genBeforeReturn = manager.getGeneration()
        manager.returnFromTransaction(newRev, 100uL, genBeforeReturn)
        assertNotNull(manager.getCurrent())
        assertEquals(2L, manager.getCurrent()!!.revisionId)
    }

    @Test
    fun commitCancelRevision_ownedNewRevision_releasesResourcesOnComplete() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)

        val takeResult = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(takeResult is TakeCurrentResult.Success)
        val takenRev = (takeResult as TakeCurrentResult.Success).revision

        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(100uL)

        val allResources = mutableListOf<FakeVisualResource>()
        allResources.addAll(takenRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
        allResources.addAll(newRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })

        var completedRev: AndroidCompositionVisualRevision? = null
        val tx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionCommitOrCancel,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = takenRev,
            ownedNewRevision = newRev,
            onTransactionComplete = { rev, _ ->
                completedRev = rev as? AndroidCompositionVisualRevision
                ReturnFromTransactionResult.Accepted
            }
        )

        tx.complete()
        assertNotNull(completedRev)
        assertTrue(takenRev.isReleased())
        assertFalse(completedRev!!.isReleased())

        completedRev!!.release(completedRev!!.owner)
        assertTrue(completedRev!!.isReleased())

        assertEquals(allResources.size, allResources.count { it.released })
    }

    @Test
    fun commitCancelRevision_ownedNewRevision_releasesResourcesOnCancel() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)

        val takeResult = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(takeResult is TakeCurrentResult.Success)
        val takenRev = (takeResult as TakeCurrentResult.Success).revision

        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(100uL)

        val allResources = mutableListOf<FakeVisualResource>()
        allResources.addAll(takenRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })
        allResources.addAll(newRev.lineSnapshots.mapNotNull { it.visualResource as? FakeVisualResource })

        val tx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionCommitOrCancel,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = takenRev,
            ownedNewRevision = newRev
        )

        tx.cancel("test_cancel")
        assertTrue(takenRev.isReleased())
        assertTrue(newRev.isReleased())
        assertEquals(allResources.size, allResources.count { it.released })
    }

    @Test
    fun commitCancel_withActiveTransaction_reassignsKey() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)

        val firstTake = manager.takeCurrentForTransactionTyped(100uL)
        assertTrue(firstTake is TakeCurrentResult.Success)

        val secondTake = manager.takeCurrentForTransactionTyped(200uL)
        assertTrue(secondTake is TakeCurrentResult.RevisionWithActiveTransaction)
        assertEquals(100uL, (secondTake as TakeCurrentResult.RevisionWithActiveTransaction).activeTransactionKey)

        manager.reassignActiveTransactionKey(200uL)
        assertEquals(200uL, manager.getActiveTransactionKey())

        manager.clear()
        assertNull(manager.getActiveTransactionKey())
    }

    @Test
    fun commitCancel_usesCommittedVisualRevision() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val prevRev = manager.takeCurrentForTransaction(100uL)
        assertNotNull(prevRev)

        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(100uL)
        val activeTx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Rendering,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRev,
            ownedNewRevision = newRev
        )

        val takenNewRev = activeTx.takeNewRevisionForRebase() as? AndroidCompositionVisualRevision
        val detachedOldRev = activeTx.detachOldRevisionForRebase() as? AndroidCompositionVisualRevision
        activeTx.onTransactionComplete = null
        activeTx.cancel("superseded_by_commit_cancel")

        if (detachedOldRev != null) {
            detachedOldRev.release(detachedOldRev.owner)
        }

        assertNotNull(takenNewRev)
        takenNewRev!!.reassignToTransaction(200uL)

        val committedRev = CommittedVisualRevision(
            revisionId = 3,
            sessionId = takenNewRev.sessionId,
            fullText = "committed text",
            affectedParagraphRange = HalfOpenRange(0, 1),
            lineSnapshots = emptyList(),
            cursorRect = RectF()
        )
        committedRev.transferToTransaction(200uL)

        val commitTx = AndroidPlatformVisualTransaction(
            key = 200uL,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionCommitOrCancel,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 2,
            newRevision = 3,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = takenNewRev,
            ownedNewRevision = committedRev,
            onTransactionComplete = { rev, _ ->
                rev.release(rev.owner)
                ReturnFromTransactionResult.Accepted
            }
        )

        commitTx.complete()
        assertTrue(takenNewRev.isReleased())
        assertTrue(committedRev.isReleased())
    }

    @Test
    fun returnFromTransaction_rejectedStale_releasesRevisionWithFakeResources() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        manager.takeCurrentForTransaction(100uL)
        val staleGen = manager.getGeneration()

        manager.clear()

        val staleRev = makeRevisionWithFakeResources(2, 2)
        val result = manager.returnFromTransaction(staleRev, 100uL, staleGen)

        assertTrue(result is ReturnFromTransactionResult.RejectedStale)
        assertTrue(staleRev.isReleased())
        for (snap in staleRev.lineSnapshots) {
            assertTrue(snap.isReleased())
        }
    }

    @Test
    fun transactionComplete_callbackRejectedStale_noDoubleRelease() {
        val rev1 = makeRevisionWithFakeResources(1, 2)
        manager.setCurrent(rev1)
        val prevRev = manager.takeCurrentForTransaction(100uL)
        val newRev = makeRevisionWithFakeResources(2, 2)
        newRev.transferToTransaction(100uL)
        val staleGen = manager.getGeneration() + 1

        val tx = AndroidPlatformVisualTransaction(
            key = 100uL,
            state = AndroidVisualTransactionState.Pending,
            operationKind = AndroidVisualOperationKind.CompositionUpdate,
            animationMode = AnimationModeData.GlyphAnimation,
            durationMs = 160,
            oldRevision = 1,
            newRevision = 2,
            slices = mutableListOf(),
            staticLinePatches = mutableListOf(),
            decorationSlices = mutableListOf(),
            cursorTransition = AndroidCursorTransition.snap(android.graphics.RectF()),
            ownedOldRevision = prevRev,
            ownedNewRevision = newRev,
            onTransactionComplete = { rev, _ ->
                manager.returnFromTransaction(rev, 100uL, staleGen)
            }
        )

        tx.complete()
        assertTrue(prevRev!!.isReleased())
        assertTrue(newRev.isReleased())
    }
}

class CompositionSessionTest {

    @Test
    fun createNew_zeroLengthReplaceRange() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        assertTrue(session.isActive)
        assertEquals(2, session.replaceStart)
        assertEquals(2, session.replaceEndExclusive)
        assertEquals(0, session.replaceRangeLength())
        assertEquals("你好abc世界", session.buildVirtualText())
    }

    @Test
    fun createNew_nonZeroReplaceRange() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 1,
            replaceEndExclusive = 3,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        assertEquals(1, session.replaceStart)
        assertEquals(3, session.replaceEndExclusive)
        assertEquals(2, session.replaceRangeLength())
        assertEquals("你abc界", session.buildVirtualText())
    }

    @Test
    fun preeditRangeInVirtualText_zeroLengthReplace() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        val range = session.preeditRangeInVirtualText()
        assertEquals(2, range.start)
        assertEquals(5, range.end)
    }

    @Test
    fun preeditRangeInVirtualText_nonZeroReplace() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 1,
            replaceEndExclusive = 3,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        val range = session.preeditRangeInVirtualText()
        assertEquals(1, range.start)
        assertEquals(4, range.end)
    }

    @Test
    fun updatePreedit_doesNotChangeReplaceRange() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "n",
            preeditCursorOffset = 1
        )

        assertEquals(2, session.replaceStart)
        assertEquals(2, session.replaceEndExclusive)

        val updated = session.updatePreedit("ni", 2)

        assertEquals(2, updated.replaceStart)
        assertEquals(2, updated.replaceEndExclusive)
        assertEquals("你好ni世界", updated.buildVirtualText())

        val updated2 = updated.updatePreedit("nihao", 5)

        assertEquals(2, updated2.replaceStart)
        assertEquals(2, updated2.replaceEndExclusive)
        assertEquals("你好nihao世界", updated2.buildVirtualText())
    }

    @Test
    fun emptySession_isNotActive() {
        assertFalse(CompositionSession.EMPTY.isActive)
    }

    @Test
    fun replaceRange_returnsCorrectIntRange() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 1,
            replaceEndExclusive = 3,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        assertEquals(HalfOpenRange(1, 3), session.replaceRange())
    }

    @Test
    fun consecutiveUpdates_100times_noLeak() {
        var session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "n",
            preeditCursorOffset = 1
        )

        for (i in 1..100) {
            session = session.updatePreedit("n$i", 2)
        }

        assertEquals(2, session.replaceStart)
        assertEquals(2, session.replaceEndExclusive)
        assertTrue(session.isActive)
    }

    @Test
    fun commit_returnsCorrectCommittedText() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        val (cleared, committedText) = session.commit("abc")
        assertFalse(cleared.isActive)
        assertEquals("你好abc世界", committedText)
    }

    @Test
    fun cancel_returnsInactiveSession() {
        val session = CompositionSession.createNew(
            committedRevisionId = 1,
            committedText = "你好世界",
            replaceStart = 2,
            replaceEndExclusive = 2,
            preeditText = "abc",
            preeditCursorOffset = 3
        )

        val cancelled = session.cancel()
        assertFalse(cancelled.isActive)
    }
}
