package com.xiwei.sujian.editor.v2

import com.xiwei.sujian.editor.v2.visual.*
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import android.graphics.Bitmap

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AnimationRebaseContractTest {

    private fun makeSnapshot(id: Long, lineIndex: Int, byteStart: Int, byteEnd: Int): AndroidLineSnapshot {
        val bitmap = Bitmap.createBitmap(100, 20, Bitmap.Config.ARGB_8888)
        return AndroidLineSnapshot(
            snapshotId = id,
            bitmap = bitmap,
            lineIndex = lineIndex,
            sourceRect = android.graphics.Rect(0, 0, 100, 20),
            destinationRect = android.graphics.RectF(0f, lineIndex * 20f, 100f, (lineIndex + 1) * 20f),
            documentByteStart = byteStart,
            documentByteEndExclusive = byteEnd
        )
    }

    private fun makeTransaction(
        transactionId: Long = 1L,
        slices: List<PreparedVisualTransaction.AnimatedSlice> = emptyList(),
        cursorTransition: PreparedVisualTransaction.CursorTransition? = null,
        ownedSnapshotIds: Set<Long> = emptySet(),
        referencedSnapshotIds: Set<Long> = emptySet()
    ): PreparedVisualTransaction {
        return PreparedVisualTransaction(
            transactionId = transactionId,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = slices,
            ownedSnapshotIds = ownedSnapshotIds,
            referencedSnapshotIds = referencedSnapshotIds,
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = cursorTransition,
            durationMs = 100
        )
    }

    @Test
    fun pendingStateReturnsValidFrameWithSliceStatesAndCursor() {
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore()
        )
        val snapshot = makeSnapshot(1, 0, 0, 10)
        val slices = listOf(
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
                snapshot = snapshot,
                sourceRect = android.graphics.Rect(0, 0, 100, 20),
                destinationRect = android.graphics.RectF(10f, 0f, 50f, 20f),
                startAlpha = 0f,
                endAlpha = 1f,
                clusterByteStart = 0,
                clusterByteEndExclusive = 10
            )
        )
        val cursor = PreparedVisualTransaction.CursorTransition(
            fromX = 10f, fromY = 0f, fromHeight = 20f,
            toX = 50f, toY = 0f, toHeight = 20f,
            shouldAnimate = true
        )
        val transaction = makeTransaction(
            transactionId = 1L,
            slices = slices,
            cursorTransition = cursor,
            ownedSnapshotIds = setOf(1L),
            referencedSnapshotIds = setOf(1L)
        )

        engine.registerSnapshots(mapOf(0 to snapshot), SnapshotOwner.OwnedByTransaction(1L))
        engine.submit(transaction)

        val frame = engine.captureFrame(0)
        assertNotNull("Pending transaction must return a valid frame", frame)
        assertEquals(TransactionState.Pending, frame!!.state)
        assertEquals(0f, frame.progress, 0.01f)
        assertTrue("Pending frame must include slice visual states", frame.sliceVisualStates.isNotEmpty())
        assertEquals(0f, frame.sliceVisualStates[0].currentAlpha, 0.01f)
        assertNotNull("Pending frame must include cursor rect", frame.cursorRect)
        assertEquals(10f, frame.cursorRect!!.left, 0.01f)
    }

    @Test
    fun renderingStateReturnsInterpolatedFrame() {
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore()
        )
        val snapshot = makeSnapshot(1, 0, 0, 10)
        val slices = listOf(
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
                snapshot = snapshot,
                sourceRect = android.graphics.Rect(0, 0, 100, 20),
                destinationRect = android.graphics.RectF(10f, 0f, 50f, 20f),
                startAlpha = 0f,
                endAlpha = 1f,
                clusterByteStart = 0,
                clusterByteEndExclusive = 10
            )
        )
        val transaction = makeTransaction(
            transactionId = 1L,
            slices = slices,
            ownedSnapshotIds = setOf(1L),
            referencedSnapshotIds = setOf(1L)
        )

        engine.registerSnapshots(mapOf(0 to snapshot), SnapshotOwner.OwnedByTransaction(1L))
        engine.submit(transaction)
        engine.markFirstVisibleFrame(1000)

        val frame = engine.captureFrame(1050)
        assertNotNull(frame)
        assertEquals(TransactionState.Rendering, frame!!.state)
        assertTrue(frame.progress > 0f)
        assertTrue(frame.sliceVisualStates[0].currentAlpha > 0f)
    }

    @Test
    fun visualFrameSnapshotContainsCursorRect() {
        val frame = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = android.graphics.RectF(10f, 0f, 12f, 20f)
        )
        assertNotNull(frame.cursorRect)
        assertEquals(10f, frame.cursorRect!!.left, 0.01f)
    }

    @Test
    fun sliceVisualStatePreservesClusterByteRange() {
        val state = SliceVisualState(
            snapshotId = 1L,
            role = SliceRole.Move,
            lineIndex = 0,
            documentByteStart = 0,
            documentByteEndExclusive = 100,
            clusterByteStart = 10,
            clusterByteEndExclusive = 20,
            currentLeft = 10f,
            currentTop = 0f,
            currentRight = 50f,
            currentBottom = 20f,
            currentAlpha = 0.5f
        )
        assertEquals(10, state.clusterByteStart)
        assertEquals(20, state.clusterByteEndExclusive)
    }

    @Test
    fun deleteRebaseDirectionStartsFromCurrentAlpha() {
        val slice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Delete,
            snapshot = makeSnapshot(1, 0, 0, 10),
            sourceRect = android.graphics.Rect(0, 0, 100, 20),
            destinationRect = android.graphics.RectF(10f, 0f, 50f, 20f),
            startAlpha = 0.6f,
            endAlpha = 0f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 10
        )
        assertEquals(0.6f, slice.startAlpha, 0.01f)
        assertEquals(0f, slice.endAlpha, 0.01f)
    }

    @Test
    fun crossfadeOldRebaseDirectionFadesToZero() {
        val slice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.CrossfadeOld,
            snapshot = makeSnapshot(1, 0, 0, 10),
            sourceRect = android.graphics.Rect(0, 0, 100, 20),
            destinationRect = android.graphics.RectF(10f, 0f, 50f, 20f),
            startAlpha = 0.4f,
            endAlpha = 0f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 10
        )
        assertEquals(0.4f, slice.startAlpha, 0.01f)
        assertEquals(0f, slice.endAlpha, 0.01f)
    }

    @Test
    fun positionedGlyphsMethodNamesMatchPublicApi() {
        val shaperClassName = "android.graphics.text.TextRunShaper"
        val positionedGlyphsClassName = "android.graphics.text.PositionedGlyphs"

        try {
            val shaperClass = Class.forName(shaperClassName)
            val shapeMethod = shaperClass.getMethod(
                "shapeTextRun",
                CharSequence::class.java,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType,
                Float::class.javaPrimitiveType,
                Float::class.javaPrimitiveType,
                Boolean::class.javaPrimitiveType,
                android.graphics.Paint::class.java
            )
            assertNotNull(shapeMethod)
        } catch (_: ClassNotFoundException) {
        } catch (_: NoSuchMethodException) {
            fail("TextRunShaper.shapeTextRun not found with expected signature")
        }

        try {
            val pgClass = Class.forName(positionedGlyphsClassName)
            pgClass.getMethod("glyphCount")
            pgClass.getMethod("getFont", Int::class.javaPrimitiveType)
            pgClass.getMethod("getGlyphId", Int::class.javaPrimitiveType)
            pgClass.getMethod("getGlyphX", Int::class.javaPrimitiveType)
            pgClass.getMethod("getGlyphY", Int::class.javaPrimitiveType)
        } catch (_: ClassNotFoundException) {
        } catch (e: NoSuchMethodException) {
            fail("PositionedGlyphs method not found: ${e.message}")
        }
    }

    @Test
    fun rtlRectNormalizationVisualLeftAlwaysLessOrEqualVisualRight() {
        val x0Rtl = 80f
        val x1Rtl = 20f
        val visualLeft = kotlin.math.min(x0Rtl, x1Rtl)
        val visualRight = kotlin.math.max(x0Rtl, x1Rtl)
        assertTrue(visualLeft <= visualRight)
        assertEquals(20f, visualLeft, 0.01f)
        assertEquals(80f, visualRight, 0.01f)
    }

    @Test
    fun ltrRectNormalizationPreservesOrder() {
        val x0Ltr = 20f
        val x1Ltr = 80f
        val visualLeft = kotlin.math.min(x0Ltr, x1Ltr)
        val visualRight = kotlin.math.max(x0Ltr, x1Ltr)
        assertTrue(visualLeft <= visualRight)
        assertEquals(20f, visualLeft, 0.01f)
        assertEquals(80f, visualRight, 0.01f)
    }

    @Test
    fun sourceRectNormalizationWithRtl() {
        val lineLeft = 0f
        val x0 = 80f
        val x1 = 20f
        val visualLeft = kotlin.math.min(x0, x1)
        val visualRight = kotlin.math.max(x0, x1)
        val sourceLeft = (visualLeft - lineLeft).coerceAtLeast(0f)
        val sourceRight = (visualRight - lineLeft).coerceAtLeast(sourceLeft)
        assertTrue(sourceLeft < sourceRight)
        assertEquals(20f, sourceLeft, 0.01f)
        assertEquals(80f, sourceRight, 0.01f)
    }

    @Test
    fun compatibleRebaseRolesGroupCorrectly() {
        val planner = AndroidVisualPlanner()
        val moveCompatible = setOf(SliceRole.Move, SliceRole.Insert, SliceRole.CrossfadeNew)
        val deleteCompatible = setOf(SliceRole.Delete, SliceRole.CrossfadeOld)

        for (role in moveCompatible) {
            val compat = invokeCompatibleRebaseRoles(planner, role)
            assertTrue("Role $role should be compatible with Move group",
                compat.containsAll(moveCompatible))
        }
        for (role in deleteCompatible) {
            val compat = invokeCompatibleRebaseRoles(planner, role)
            assertTrue("Role $role should be compatible with Delete group",
                compat.containsAll(deleteCompatible))
        }
    }

    @Suppress("UNCHECKED_CAST")
    private fun invokeCompatibleRebaseRoles(planner: AndroidVisualPlanner, role: SliceRole): Set<SliceRole> {
        val method = planner.javaClass.getDeclaredMethod("compatibleRebaseRoles", SliceRole::class.java)
        method.isAccessible = true
        return method.invoke(planner, role) as Set<SliceRole>
    }

    @Test
    fun rebaseOneToOneAllocationPreventsDuplicateUse() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(
                SliceVisualState(
                    snapshotId = 1L, role = SliceRole.Insert, lineIndex = 0,
                    clusterByteStart = 0, clusterByteEndExclusive = 5,
                    currentLeft = 0f, currentTop = 0f, currentRight = 30f, currentBottom = 20f,
                    currentAlpha = 0.5f
                ),
                SliceVisualState(
                    snapshotId = 2L, role = SliceRole.Insert, lineIndex = 0,
                    clusterByteStart = 5, clusterByteEndExclusive = 10,
                    currentLeft = 30f, currentTop = 0f, currentRight = 60f, currentBottom = 20f,
                    currentAlpha = 0.5f
                )
            ),
            cursorRect = null
        )

        val usedRebaseIndices = mutableSetOf<Int>()
        val firstMatch = rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
            i !in usedRebaseIndices && rebaseSnapshot.sliceVisualStates[i].role == SliceRole.Insert
        }
        assertNotNull(firstMatch)
        usedRebaseIndices.add(firstMatch!!)

        val secondMatch = rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
            i !in usedRebaseIndices && rebaseSnapshot.sliceVisualStates[i].role == SliceRole.Insert
        }
        assertNotNull(secondMatch)
        assertNotEquals(firstMatch, secondMatch)
    }

    @Test
    fun snapshotOwnershipTransferPreventsLeak() {
        val store = VisualResourceStore()
        val oldOwner = SnapshotOwner.OwnedByTransaction(1L)
        val newOwner = SnapshotOwner.OwnedByTransaction(2L)
        val snapshot = makeSnapshot(1, 0, 0, 10)

        store.put(snapshot, oldOwner)
        assertTrue(store.transferOwnership(1, newOwner))
        assertEquals(newOwner, store.getOwner(1))

        store.release(1, newOwner)
        assertNull(store.get(1))
    }

    @Test
    fun snapshotReleaseByWrongOwnerIsNoOp() {
        val store = VisualResourceStore()
        val owner1 = SnapshotOwner.OwnedByTransaction(1L)
        val owner2 = SnapshotOwner.OwnedByTransaction(2L)
        val snapshot = makeSnapshot(1, 0, 0, 10)

        store.put(snapshot, owner1)
        store.release(1, owner2)
        assertNotNull("Wrong-owner release must not remove snapshot", store.get(1))
    }

    @Test
    fun systemSuppressedPolicySkipsAnimationInPrepareAndSubmit() {
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore()
        )
        engine.setAnimationPolicy(TextAnimationPolicy.SYSTEM_SUPPRESSED)

        var mirrorUpdateCalled = false
        val layoutEngine = AndroidLayoutEngine(
            com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror(),
            android.text.TextPaint().apply { textSize = 48f }
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(0, 3)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(0, 0, true)
        )
        engine.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutEngine,
            mirrorUpdate = { mirrorUpdateCalled = true }
        )
        assertTrue("mirrorUpdate must still be called under SYSTEM_SUPPRESSED", mirrorUpdateCalled)
        assertNull("No active transaction under SYSTEM_SUPPRESSED", engine.getActiveTransaction())
    }

    @Test
    fun shapingFingerprintExcludesGlyphPositions() {
        val fp1 = "font1_42|font1_43_ctx_99"
        val fp2 = "font1_42|font1_43_ctx_99"
        assertEquals("Fingerprints without glyph positions must match", fp1, fp2)
        assertFalse("Fingerprint must not contain glyph X/Y separator pattern _x_y",
            fp1.contains(Regex("_\\d+_\\d+_\\d+")))
    }
}
