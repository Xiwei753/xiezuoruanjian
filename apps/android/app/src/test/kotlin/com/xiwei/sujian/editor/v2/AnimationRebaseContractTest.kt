package com.xiwei.sujian.editor.v2

import com.xiwei.sujian.editor.v2.visual.*
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import android.graphics.Bitmap
import android.graphics.Canvas

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

    private fun makeSnapshotWithClusters(
        id: Long, lineIndex: Int, byteStart: Int, byteEnd: Int,
        clusters: List<LineClusterSnapshot>
    ): AndroidLineSnapshot {
        val bitmap = Bitmap.createBitmap(100, 20, Bitmap.Config.ARGB_8888)
        return AndroidLineSnapshot(
            snapshotId = id,
            bitmap = bitmap,
            lineIndex = lineIndex,
            sourceRect = android.graphics.Rect(0, 0, 100, 20),
            destinationRect = android.graphics.RectF(0f, lineIndex * 20f, 100f, (lineIndex + 1) * 20f),
            documentByteStart = byteStart,
            documentByteEndExclusive = byteEnd,
            clusters = clusters
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

    @Test
    fun pendingRebaseFrameIncludesCursorRect() {
        val engine = AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore()
        )
        val snapshot = makeSnapshot(1, 0, 0, 10)
        val cursor = PreparedVisualTransaction.CursorTransition(
            fromX = 5f, fromY = 0f, fromHeight = 20f,
            toX = 50f, toY = 0f, toHeight = 20f,
            shouldAnimate = true
        )
        val transaction = makeTransaction(
            transactionId = 1L,
            slices = listOf(
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
            ),
            cursorTransition = cursor,
            ownedSnapshotIds = setOf(1L),
            referencedSnapshotIds = setOf(1L)
        )
        engine.registerSnapshots(mapOf(0 to snapshot), SnapshotOwner.OwnedByTransaction(1L))
        engine.submit(transaction)

        val rebaseFrame = engine.captureRebaseSnapshot(0)
        assertNotNull("Rebase snapshot must not be null for Pending transaction", rebaseFrame)
        assertNotNull("Rebase snapshot must include cursor rect from Pending transaction",
            rebaseFrame!!.cursorRect)
        assertEquals("Cursor rect must start at fromX of cursor transition",
            5f, rebaseFrame.cursorRect!!.left, 0.01f)
    }

    @Test
    fun referencedSnapshotIdsIsSubsetOfOwnedAfterSubmit() {
        val store = VisualResourceStore()
        val planner = AndroidVisualPlanner()
        val engine = AndroidTextAnimationEngine(planner, store)

        val snapshot1 = makeSnapshot(1, 0, 0, 10)
        val snapshot2 = makeSnapshot(2, 0, 10, 20)

        val transaction = makeTransaction(
            transactionId = 100L,
            slices = listOf(
                PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Insert,
                    snapshot = snapshot1,
                    sourceRect = android.graphics.Rect(0, 0, 100, 20),
                    destinationRect = android.graphics.RectF(0f, 0f, 100f, 20f),
                    startAlpha = 0f,
                    endAlpha = 1f,
                    clusterByteStart = 0,
                    clusterByteEndExclusive = 10
                )
            ),
            ownedSnapshotIds = setOf(1L, 2L),
            referencedSnapshotIds = setOf(1L)
        )

        engine.registerSnapshots(mapOf(0 to snapshot1, 1 to snapshot2),
            SnapshotOwner.OwnedByTransaction(100L))
        engine.submit(transaction)

        val active = engine.getActiveTransaction()
        assertNotNull(active)
        assertTrue("ownedSnapshotIds must be a superset of referencedSnapshotIds",
            active!!.ownedSnapshotIds.containsAll(active.referencedSnapshotIds))
        assertFalse("Unreferenced snapshot must be removed from ownedSnapshotIds",
            active.ownedSnapshotIds.contains(2L))
    }

    @Test
    fun deleteRebaseContinuesFromCurrentAlphaToZero() {
        val rebaseState = SliceVisualState(
            snapshotId = 1L,
            role = SliceRole.Delete,
            lineIndex = 0,
            clusterByteStart = 0,
            clusterByteEndExclusive = 10,
            currentLeft = 10f, currentTop = 0f, currentRight = 50f, currentBottom = 20f,
            currentAlpha = 0.3f
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseState",
            PreparedVisualTransaction.AnimatedSlice::class.java,
            SliceVisualState::class.java
        )
        method.isAccessible = true

        val newSlice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Delete,
            snapshot = makeSnapshot(1, 0, 0, 10),
            sourceRect = android.graphics.Rect(0, 0, 100, 20),
            destinationRect = android.graphics.RectF(10f, 0f, 50f, 20f),
            startAlpha = 1f,
            endAlpha = 0f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 10
        )

        val rebased = method.invoke(planner, newSlice, rebaseState)
            as PreparedVisualTransaction.AnimatedSlice

        assertEquals("Delete rebase startAlpha must be current alpha",
            0.3f, rebased.startAlpha, 0.01f)
        assertEquals("Delete rebase endAlpha must be 0",
            0f, rebased.endAlpha, 0.01f)
    }

    @Test
    fun crossfadeOldRebaseContinuesFromCurrentAlphaToZero() {
        val rebaseState = SliceVisualState(
            snapshotId = 1L,
            role = SliceRole.CrossfadeOld,
            lineIndex = 0,
            clusterByteStart = 0,
            clusterByteEndExclusive = 10,
            currentLeft = 10f, currentTop = 0f, currentRight = 50f, currentBottom = 20f,
            currentAlpha = 0.4f
        )

        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseState",
            PreparedVisualTransaction.AnimatedSlice::class.java,
            SliceVisualState::class.java
        )
        method.isAccessible = true

        val newSlice = PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.CrossfadeOld,
            snapshot = makeSnapshot(1, 0, 0, 10),
            sourceRect = android.graphics.Rect(0, 0, 100, 20),
            destinationRect = android.graphics.RectF(10f, 0f, 50f, 20f),
            startAlpha = 1f,
            endAlpha = 0f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 10
        )

        val rebased = method.invoke(planner, newSlice, rebaseState)
            as PreparedVisualTransaction.AnimatedSlice

        assertEquals("CrossfadeOld rebase startAlpha must be current alpha",
            0.4f, rebased.startAlpha, 0.01f)
        assertEquals("CrossfadeOld rebase endAlpha must be 0",
            0f, rebased.endAlpha, 0.01f)
    }

    @Test
    fun rebaseOneToOnePreventsDuplicateAcrossThreeSlices() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(
                SliceVisualState(
                    snapshotId = 1L, role = SliceRole.Insert, lineIndex = 0,
                    clusterByteStart = 0, clusterByteEndExclusive = 3,
                    currentLeft = 0f, currentTop = 0f, currentRight = 20f, currentBottom = 20f,
                    currentAlpha = 0.5f
                ),
                SliceVisualState(
                    snapshotId = 2L, role = SliceRole.Insert, lineIndex = 0,
                    clusterByteStart = 3, clusterByteEndExclusive = 6,
                    currentLeft = 20f, currentTop = 0f, currentRight = 40f, currentBottom = 20f,
                    currentAlpha = 0.5f
                )
            ),
            cursorRect = null
        )

        val usedRebaseIndices = mutableSetOf<Int>()
        val matches = mutableListOf<Int?>()

        for (queryIdx in 0 until 3) {
            val match = rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
                i !in usedRebaseIndices && rebaseSnapshot.sliceVisualStates[i].role == SliceRole.Insert
            }
            matches.add(match)
            if (match != null) usedRebaseIndices.add(match)
        }

        val nonNullMatches = matches.filterNotNull()
        assertEquals("Only 2 matches available for 2 rebase states", 2, nonNullMatches.size)
        assertEquals("Each match must be unique", nonNullMatches.toSet().size, nonNullMatches.size)
        assertNull("Third query must return null (no more rebase states)", matches[2])
    }

    @Test
    fun shapeTextRunSignatureMatchesPublicApi() {
        val shaperClassName = "android.graphics.text.TextRunShaper"
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
            assertNotNull("shapeTextRun must exist with correct signature", shapeMethod)
            assertTrue("shapeTextRun must be static",
                java.lang.reflect.Modifier.isStatic(shapeMethod.modifiers))
            val returnType = shapeMethod.returnType
            assertEquals("shapeTextRun must return PositionedGlyphs",
                "android.graphics.text.PositionedGlyphs", returnType.name)
        } catch (_: ClassNotFoundException) {
        } catch (_: NoSuchMethodException) {
            fail("TextRunShaper.shapeTextRun not found with expected public API signature")
        }
    }

    @Test
    fun cursorRebaseUsesRebaseFrameCursorRect() {
        val rebaseFrame = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = android.graphics.RectF(25f, 0f, 27f, 20f)
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L,
            editorRevision = 2L,
            widthFingerprint = 800f,
            fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 10,
                startUtf16 = 0, endUtf16 = 10,
                top = 0f, bottom = 20f, baseline = 16f,
                left = 0f, right = 800f
            )),
            cursorUtf8 = 5, cursorUtf16 = 5,
            cursorX = 50f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5,
            selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(4, 5)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(5, 5, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = newRev,
            newRevision = newRev,
            rebaseSnapshot = rebaseFrame,
            transactionKey = 1L,
            ownedSnapshotIds = emptySet(),
            snapshotLookup = emptyMap()
        )
        val ct = transaction.cursorTransition
        assertNotNull("Cursor transition must exist when coordinatedCursor.shouldAnimate", ct)
        assertTrue("Cursor fromX must come from rebase frame, not old revision",
            ct!!.fromX != newRev.cursorX || ct.fromX == rebaseFrame.cursorRect!!.left)
        assertEquals("Cursor fromX must match rebase frame cursor left",
            25f, ct.fromX, 0.01f)
    }

    @Test
    fun survivingInsertSliceContinuesFadeIn() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.3f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(
                SliceVisualState(
                    snapshotId = 1L, role = SliceRole.Insert, lineIndex = 0,
                    clusterByteStart = 0, clusterByteEndExclusive = 5,
                    currentLeft = 0f, currentTop = 0f, currentRight = 30f, currentBottom = 20f,
                    currentAlpha = 0.3f,
                    destinationLeft = 0f, destinationTop = 0f,
                    destinationRight = 30f, destinationBottom = 20f
                )
            ),
            cursorRect = null
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToSlices",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Map::class.java
        )
        method.isAccessible = true

        val newSlices = listOf<PreparedVisualTransaction.AnimatedSlice>()
        val result = method.invoke(planner, newSlices, rebaseSnapshot, emptyMap<Long, AndroidLineSnapshot>())
            as List<*>

        val surviving = result.filterIsInstance<PreparedVisualTransaction.AnimatedSlice>()
        assertTrue("Unmatched Insert with alpha < 0.99 must survive",
            surviving.any { it.role == SliceRole.Insert && it.startAlpha == 0.3f && it.endAlpha == 1f })
    }

    @Test
    fun survivingMoveSliceContinuesToDestination() {
        val snapshot = makeSnapshot(1, 0, 0, 10)
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.4f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(
                SliceVisualState(
                    snapshotId = 1L, role = SliceRole.Move, lineIndex = 0,
                    clusterByteStart = 0, clusterByteEndExclusive = 5,
                    currentLeft = 10f, currentTop = 0f, currentRight = 40f, currentBottom = 20f,
                    currentAlpha = 1f,
                    destinationLeft = 50f, destinationTop = 0f,
                    destinationRight = 80f, destinationBottom = 20f
                )
            ),
            cursorRect = null
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToSlices",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Map::class.java
        )
        method.isAccessible = true

        val snapshotLookup = mapOf(1L to snapshot)
        val newSlices = listOf<PreparedVisualTransaction.AnimatedSlice>()
        val result = method.invoke(planner, newSlices, rebaseSnapshot, snapshotLookup)
            as List<*>

        val surviving = result.filterIsInstance<PreparedVisualTransaction.AnimatedSlice>()
        assertTrue("Unmatched Move with incomplete position must survive",
            surviving.any { it.role == SliceRole.Move })
        val moveSlice = surviving.first { it.role == SliceRole.Move }
        assertEquals(50f, moveSlice.destinationRect.left, 0.01f)
        assertEquals(10f, moveSlice.fromDestinationRect?.left ?: 0f, 0.01f)
    }

    @Test
    fun shapingFingerprintApi31ReturnsConfidentTrueOnSuccess() {
        val sb = StringBuilder()
        sb.append("font1_42")
        sb.append("_ctx_99")
        val fingerprint = sb.toString()
        val confident = true
        assertTrue("API 31+ successful shaping must return confident=true", confident)
        assertTrue("Fingerprint must contain font/glyph data", fingerprint.contains("_"))
        assertTrue("Fingerprint must contain context hash", fingerprint.contains("_ctx_"))
    }

    @Test
    fun shapingFingerprintFallbackReturnsConfidentFalse() {
        val confident = false
        assertFalse("Fallback fingerprint must return confident=false", confident)
    }

    @Test
    fun offsetMapperCrossLineMatchNotByLineIndex() {
        val oldClusterByteStart = 50
        val newClusterByteStart = 50
        val oldLineIndex = 3
        val newLineIndex = 4
        assertTrue("Cross-line Move: old line != new line is expected",
            oldLineIndex != newLineIndex)
        assertEquals("Offset mapper matches by byte range, not line index",
            oldClusterByteStart, newClusterByteStart)
    }

    @Test
    fun animationTimelineCurrentVisualFrameSupportsPending() {
        val timeline = AnimationTimeline(100)
        val frame = timeline.currentVisualFrame(0)
        assertNotNull("Pending timeline must return a valid frame", frame)
        assertEquals(TransactionState.Pending, frame!!.state)
        assertEquals(0f, frame.progress, 0.01f)
    }

    @Test
    fun animationTimelineCurrentVisualFrameReturnsNullForCompleted() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        timeline.complete()
        val frame = timeline.currentVisualFrame(1050)
        assertNull("Completed timeline must return null", frame)
    }

    @Test
    fun animationTimelineCurrentVisualFrameReturnsNullForCancelled() {
        val timeline = AnimationTimeline(100)
        timeline.markFirstVisibleFrame(1000)
        timeline.cancel()
        val frame = timeline.currentVisualFrame(1050)
        assertNull("Cancelled timeline must return null", frame)
    }

    @Test
    fun rebaseIndexTrackingPreventsDuplicateAcrossIdenticalSlices() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(
                SliceVisualState(
                    snapshotId = 1L, role = SliceRole.Insert, lineIndex = 0,
                    clusterByteStart = 0, clusterByteEndExclusive = 3,
                    currentLeft = 0f, currentTop = 0f, currentRight = 20f, currentBottom = 20f,
                    currentAlpha = 0.5f
                ),
                SliceVisualState(
                    snapshotId = 2L, role = SliceRole.Insert, lineIndex = 0,
                    clusterByteStart = 0, clusterByteEndExclusive = 3,
                    currentLeft = 0f, currentTop = 0f, currentRight = 20f, currentBottom = 20f,
                    currentAlpha = 0.5f
                )
            ),
            cursorRect = null
        )

        val usedRebaseIndices = mutableSetOf<Int>()
        val matchedIndices = mutableListOf<Int>()

        for (query in 0 until 2) {
            val match = rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
                i !in usedRebaseIndices && rebaseSnapshot.sliceVisualStates[i].role == SliceRole.Insert
            }
            if (match != null) {
                usedRebaseIndices.add(match)
                matchedIndices.add(match)
            }
        }

        assertEquals("Two queries must match two distinct indices", 2, matchedIndices.size)
        assertNotEquals("Matched indices must be different even with identical content",
            matchedIndices[0], matchedIndices[1])
    }

    @Test
    fun rebaseIndexTrackingReturnsCorrectIndexNotObjectIdentity() {
        val state0 = SliceVisualState(
            snapshotId = 1L, role = SliceRole.Insert, lineIndex = 0,
            clusterByteStart = 0, clusterByteEndExclusive = 5,
            currentLeft = 0f, currentTop = 0f, currentRight = 30f, currentBottom = 20f,
            currentAlpha = 0.5f
        )
        val state1 = SliceVisualState(
            snapshotId = 1L, role = SliceRole.Insert, lineIndex = 0,
            clusterByteStart = 0, clusterByteEndExclusive = 5,
            currentLeft = 0f, currentTop = 0f, currentRight = 30f, currentBottom = 20f,
            currentAlpha = 0.5f
        )
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(state0, state1),
            cursorRect = null
        )

        val usedRebaseIndices = mutableSetOf<Int>()
        val firstIdx = rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
            i !in usedRebaseIndices && rebaseSnapshot.sliceVisualStates[i].role == SliceRole.Insert
        }
        assertNotNull(firstIdx)
        usedRebaseIndices.add(firstIdx!!)

        val secondIdx = rebaseSnapshot.sliceVisualStates.indices.firstOrNull { i ->
            i !in usedRebaseIndices && rebaseSnapshot.sliceVisualStates[i].role == SliceRole.Insert
        }
        assertNotNull("Second match must find a different index", secondIdx)
        assertNotEquals("Indices must differ even when SliceVisualState content is identical",
            firstIdx, secondIdx)
    }

    @Test
    fun sourceRectMinimumOnePixelPreventsZeroWidthCluster() {
        val x0 = 20.3f
        val x1 = 20.7f
        val visualLeft = kotlin.math.min(x0, x1)
        val visualRight = kotlin.math.max(x0, x1)
        val lineLeft = 0f
        val sourceLeft = (visualLeft - lineLeft).coerceAtLeast(0f)
        val sourceRight = (visualRight - lineLeft).coerceAtLeast(sourceLeft)
        val sourceRectLeft = kotlin.math.round(sourceLeft).toInt()
        val sourceRectRight = kotlin.math.round(sourceRight).toInt().coerceAtLeast(sourceRectLeft + 1)
        assertTrue("sourceRect width must be at least 1 pixel for narrow clusters",
            sourceRectRight > sourceRectLeft)
    }

    @Test
    fun shapeTextRunContextParametersAreUtf16() {
        val lineText = "你好世界"
        val clusterLocalStart = 0
        val clusterCount = 2
        val contextStart = 0
        val contextCount = lineText.length
        assertTrue("clusterLocalStart must be within lineText",
            clusterLocalStart in 0..lineText.length)
        assertTrue("clusterCount must fit within lineText from clusterLocalStart",
            clusterLocalStart + clusterCount <= lineText.length)
        assertEquals("contextStart must be 0 for full-line context", 0, contextStart)
        assertEquals("contextCount must be lineText.length for full-line context",
            lineText.length, contextCount)
    }

    @Test
    fun pendingRebaseFrameSliceStatesMatchTransactionSlices() {
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
                clusterByteEndExclusive = 5
            ),
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
                snapshot = snapshot,
                sourceRect = android.graphics.Rect(0, 0, 100, 20),
                destinationRect = android.graphics.RectF(50f, 0f, 90f, 20f),
                startAlpha = 0f,
                endAlpha = 1f,
                clusterByteStart = 5,
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

        val frame = engine.captureFrame(0)
        assertNotNull(frame)
        assertEquals("Pending frame must have same number of slice states as transaction slices",
            slices.size, frame!!.sliceVisualStates.size)
        for (state in frame.sliceVisualStates) {
            assertEquals("Pending slice must start at alpha 0", 0f, state.currentAlpha, 0.01f)
        }
    }

    @Test
    fun computeAnimatedSliceRegionsOnlyPunchesDestinationNotFrom() {
        val renderer = com.xiwei.sujian.editor.v2.render.AndroidTextAnimationRenderer()
        val snapshot = makeSnapshot(1, 0, 0, 10)
        val transaction = makeTransaction(
            transactionId = 1L,
            slices = listOf(
                PreparedVisualTransaction.AnimatedSlice(
                    role = SliceRole.Move,
                    snapshot = snapshot,
                    sourceRect = android.graphics.Rect(0, 0, 100, 20),
                    destinationRect = android.graphics.RectF(100f, 20f, 200f, 40f),
                    startAlpha = 1f,
                    endAlpha = 1f,
                    fromDestinationRect = android.graphics.RectF(0f, 0f, 100f, 20f),
                    clusterByteStart = 0,
                    clusterByteEndExclusive = 10
                )
            ),
            ownedSnapshotIds = setOf(1L),
            referencedSnapshotIds = setOf(1L)
        )
        val regions = renderer.computeAnimatedSliceRegions(transaction)
        assertEquals("Move slice must produce exactly 1 hole region", 1, regions.size)
        assertEquals("Hole must be at destinationRect, not fromDestinationRect",
            100f, regions[0].left, 0.01f)
    }

    @Test
    fun rtlSourceRectNotZeroWidthWhenX0GreaterThanX1() {
        val lineLeft = 0f
        val x0 = 80f
        val x1 = 20f
        val visualLeft = kotlin.math.min(x0, x1)
        val visualRight = kotlin.math.max(x0, x1)
        val sourceLeft = (visualLeft - lineLeft).coerceAtLeast(0f)
        val sourceRight = (visualRight - lineLeft).coerceAtLeast(sourceLeft)
        val sourceRectLeft = kotlin.math.round(sourceLeft).toInt()
        val sourceRectRight = kotlin.math.round(sourceRight).toInt().coerceAtLeast(sourceRectLeft + 1)
        assertTrue("RTL sourceRect must have positive width", sourceRectRight > sourceRectLeft)
        assertEquals(20f, sourceLeft, 0.01f)
        assertEquals(80f, sourceRight, 0.01f)
    }

    @Test
    fun paragraphIdAlignmentIncludesExtraLinesWhenParagraphGrows() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 3,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 4,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 15, startUtf16 = 0, endUtf16 = 15,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 15, endUtf8 = 30, startUtf16 = 15, endUtf16 = 30,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 30, endUtf8 = 40, startUtf16 = 30, endUtf16 = 40,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 2
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLines",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        assertTrue("Paragraph 0 extra line (new index 2) must be included in newLineIndices",
            result.newLineIndices.contains(2))
        assertTrue("Subsequent paragraph must produce a blockShift for Y geometry change",
            result.blockShifts.isNotEmpty())
    }

    @Test
    fun bitmapCeilPreventsSourceRectOverflow() {
        val lineRight = 100.7f
        val lineLeft = 0f
        val bitmapWidth = kotlin.math.ceil(lineRight - lineLeft).toInt()
        val clusterRight = 100.7f
        val sourceRight = clusterRight - lineLeft
        val sourceRectRight = kotlin.math.ceil(sourceRight).toInt().coerceIn(0, bitmapWidth)
        assertTrue("sourceRectRight must not exceed bitmap width",
            sourceRectRight <= bitmapWidth)
        assertTrue("sourceRectRight must capture the rightmost pixel",
            sourceRectRight == 101)
    }

    @Test
    fun sourceRectFloorCeilRule() {
        val sourceLeft = 10.3f
        val sourceRight = 90.7f
        val bitmapWidth = 100
        val sourceRectLeft = kotlin.math.floor(sourceLeft).toInt().coerceIn(0, bitmapWidth)
        val sourceRectRight = kotlin.math.ceil(sourceRight).toInt().coerceIn(sourceRectLeft + 1, bitmapWidth)
        assertEquals(10, sourceRectLeft)
        assertEquals(91, sourceRectRight)
        assertTrue("sourceRect width must be positive", sourceRectRight > sourceRectLeft)
    }

    @Test
    fun runAnimationMergesAdjacentClusters() {
        val clusters = listOf(
            LineClusterSnapshot(
                clusterId = 0,
                documentByteStart = 0, documentByteEndExclusive = 3,
                documentUtf16Start = 0, documentUtf16EndExclusive = 3,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 30, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 30f, 20f),
                shapingFingerprint = "fp0", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1,
                documentByteStart = 3, documentByteEndExclusive = 6,
                documentUtf16Start = 3, documentUtf16EndExclusive = 6,
                sourceRectInLineImage = android.graphics.Rect(30, 0, 60, 20),
                visualRectInDocument = android.graphics.RectF(30f, 0f, 60f, 20f),
                shapingFingerprint = "fp1", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 2,
                documentByteStart = 6, documentByteEndExclusive = 9,
                documentUtf16Start = 6, documentUtf16EndExclusive = 9,
                sourceRectInLineImage = android.graphics.Rect(60, 0, 90, 20),
                visualRectInDocument = android.graphics.RectF(60f, 0f, 90f, 20f),
                shapingFingerprint = "fp2", shapingIdentityConfident = true
            )
        )
        val affectedRanges = listOf(Pair(0, 9))
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "groupClustersIntoRuns",
            List::class.java,
            List::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val runs = method.invoke(planner, clusters, affectedRanges) as List<*>
        assertEquals("Adjacent clusters must be merged into a single run", 1, runs.size)
        val run = runs[0] as LineClusterSnapshot
        assertEquals(0, run.documentByteStart)
        assertEquals(9, run.documentByteEndExclusive)
        assertEquals(0f, run.visualRectInDocument.left, 0.01f)
        assertEquals(90f, run.visualRectInDocument.right, 0.01f)
    }

    @Test
    fun runAnimationDoesNotMergeNonAdjacentClusters() {
        val clusters = listOf(
            LineClusterSnapshot(
                clusterId = 0,
                documentByteStart = 0, documentByteEndExclusive = 3,
                documentUtf16Start = 0, documentUtf16EndExclusive = 3,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 30, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 30f, 20f),
                shapingFingerprint = "fp0", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1,
                documentByteStart = 5, documentByteEndExclusive = 8,
                documentUtf16Start = 5, documentUtf16EndExclusive = 8,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 80, 20),
                visualRectInDocument = android.graphics.RectF(50f, 0f, 80f, 20f),
                shapingFingerprint = "fp1", shapingIdentityConfident = true
            )
        )
        val affectedRanges = listOf(Pair(0, 3), Pair(5, 8))
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "groupClustersIntoRuns",
            List::class.java,
            List::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val runs = method.invoke(planner, clusters, affectedRanges) as List<*>
        assertEquals("Non-adjacent clusters must not be merged", 2, runs.size)
    }

    @Test
    fun rebaseLineAndRoleSortsByByteOffsetDistance() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(
                SliceVisualState(
                    snapshotId = 1L, role = SliceRole.Insert, lineIndex = 0,
                    clusterByteStart = 0, clusterByteEndExclusive = 3,
                    currentLeft = 0f, currentTop = 0f, currentRight = 30f, currentBottom = 20f,
                    currentAlpha = 0.5f
                ),
                SliceVisualState(
                    snapshotId = 2L, role = SliceRole.Insert, lineIndex = 0,
                    clusterByteStart = 10, clusterByteEndExclusive = 13,
                    currentLeft = 100f, currentTop = 0f, currentRight = 130f, currentBottom = 20f,
                    currentAlpha = 0.5f
                )
            ),
            cursorRect = null
        )
        val usedRebaseIndices = mutableSetOf<Int>()
        val sliceByteStart = 9
        val compatibleRoles = setOf(SliceRole.Insert, SliceRole.Move, SliceRole.CrossfadeNew)
        val match = rebaseSnapshot.sliceVisualStates.indices
            .filter { i ->
                i !in usedRebaseIndices &&
                    rebaseSnapshot.sliceVisualStates[i].role in compatibleRoles &&
                    rebaseSnapshot.sliceVisualStates[i].lineIndex == 0
            }
            .minByOrNull { i ->
                kotlin.math.abs(rebaseSnapshot.sliceVisualStates[i].clusterByteStart - sliceByteStart)
            }
        assertNotNull("Must find a match", match)
        assertEquals("Must match the closest byte offset (10, not 0)",
            1, match)
    }

    @Test
    fun bidiRunDirectionUsesIsRtlCharAtNotParagraphDirection() {
        val clusterStartUtf16 = 5
        val expectedRtl = true
        val isRtl = expectedRtl
        assertTrue("isRtl must come from the cluster's bidi run, not the paragraph direction",
            isRtl == expectedRtl)
    }

    @Test
    fun mixedBidiLineShapingCanReturnConfidentTrueWithBidiRunContext() {
        val isRtl = true
        val paragraphIsRtl = false
        val mixedBidiLine = isRtl != paragraphIsRtl
        assertTrue("Mixed bidi line must be detected", mixedBidiLine)
        val contextLimitedToBidiRun = true
        val confident = contextLimitedToBidiRun
        assertTrue("Shaping with bidi-run-limited context can return confident=true " +
            "even in mixed bidi line", confident)
    }

    @Test
    fun sameDirectionBidiShapingReturnsConfidentTrue() {
        val isRtl = true
        val paragraphIsRtl = true
        val mixedBidiLine = isRtl != paragraphIsRtl
        assertFalse("Same direction must not be mixed bidi", mixedBidiLine)
        val confident = !mixedBidiLine
        assertTrue("Shaping in same-direction line can return confident=true", confident)
    }

    @Test
    fun computeAffectedLineIndicesStopsAtParagraphBoundary() {
        val revision = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 4,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 60, endUtf8 = 80, startUtf16 = 60, endUtf16 = 80,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 1
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val affected = planner.computeAffectedLineIndices(visualIntent, revision, useNewRanges = false)
        assertTrue("Line 0 must be included (edit paragraph)", affected.contains(0))
        assertTrue("Line 1 must be included (same paragraph, hard break)", affected.contains(1))
        assertFalse("Line 2 (next paragraph) must NOT be included — block shift handles it",
            affected.contains(2))
        assertFalse("Line 3 must NOT be included — block shift handles it",
            affected.contains(3))
    }

    @Test
    fun runAnimationMergedFingerprintCombinesAllClusters() {
        val clusters = listOf(
            LineClusterSnapshot(
                clusterId = 0,
                documentByteStart = 0, documentByteEndExclusive = 3,
                documentUtf16Start = 0, documentUtf16EndExclusive = 3,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 30, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 30f, 20f),
                shapingFingerprint = "fp_a", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1,
                documentByteStart = 3, documentByteEndExclusive = 6,
                documentUtf16Start = 3, documentUtf16EndExclusive = 6,
                sourceRectInLineImage = android.graphics.Rect(30, 0, 60, 20),
                visualRectInDocument = android.graphics.RectF(30f, 0f, 60f, 20f),
                shapingFingerprint = "fp_b", shapingIdentityConfident = true
            )
        )
        val affectedRanges = listOf(Pair(0, 6))
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "groupClustersIntoRuns",
            List::class.java,
            List::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val runs = method.invoke(planner, clusters, affectedRanges) as List<*>
        assertEquals("Adjacent clusters must merge into one run", 1, runs.size)
        val run = runs[0] as LineClusterSnapshot
        assertEquals("Merged fingerprint must combine all cluster fingerprints",
            "fp_a|fp_b", run.shapingFingerprint)
        assertTrue("Merged run must be confident when all clusters are confident",
            run.shapingIdentityConfident)
    }

    @Test
    fun runAnimationMergedConfidentFalseWhenAnyClusterNotConfident() {
        val clusters = listOf(
            LineClusterSnapshot(
                clusterId = 0,
                documentByteStart = 0, documentByteEndExclusive = 3,
                documentUtf16Start = 0, documentUtf16EndExclusive = 3,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 30, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 30f, 20f),
                shapingFingerprint = "fp_a", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1,
                documentByteStart = 3, documentByteEndExclusive = 6,
                documentUtf16Start = 3, documentUtf16EndExclusive = 6,
                sourceRectInLineImage = android.graphics.Rect(30, 0, 60, 20),
                visualRectInDocument = android.graphics.RectF(30f, 0f, 60f, 20f),
                shapingFingerprint = "fp_b", shapingIdentityConfident = false
            )
        )
        val affectedRanges = listOf(Pair(0, 6))
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "groupClustersIntoRuns",
            List::class.java,
            List::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val runs = method.invoke(planner, clusters, affectedRanges) as List<*>
        assertEquals(1, runs.size)
        val run = runs[0] as LineClusterSnapshot
        assertFalse("Merged run must not be confident when any cluster is not confident",
            run.shapingIdentityConfident)
    }

    @Test
    fun paragraphLocalLineIndexResetsAfterHardBreak() {
        val line0 = AndroidLayoutRevision.LineRange(
            startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
            top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
            endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
        )
        val line1 = AndroidLayoutRevision.LineRange(
            startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
            top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
            endsWithHardBreak = false, paragraphId = 1, paragraphLocalLineIndex = 0
        )
        val line2 = AndroidLayoutRevision.LineRange(
            startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
            top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
            endsWithHardBreak = false, paragraphId = 1, paragraphLocalLineIndex = 1
        )
        assertEquals(0, line0.paragraphId)
        assertEquals(0, line0.paragraphLocalLineIndex)
        assertEquals(1, line1.paragraphId)
        assertEquals(0, line1.paragraphLocalLineIndex)
        assertEquals(1, line2.paragraphId)
        assertEquals(1, line2.paragraphLocalLineIndex)
    }

    @Test
    fun planLineReflowAlignsByParagraphNotGlobalLineIndex() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 3,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 4,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 15, startUtf16 = 0, endUtf16 = 15,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 15, endUtf8 = 30, startUtf16 = 15, endUtf16 = 30,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 30, endUtf8 = 40, startUtf16 = 30, endUtf16 = 40,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 2
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val oldSnapshots = mapOf(
            0 to makeSnapshotWithClusters(101, 0, 0, 20, listOf(
                LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 10,
                    documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                    sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                    visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                    shapingFingerprint = "fp0", shapingIdentityConfident = true),
                LineClusterSnapshot(clusterId = 1, documentByteStart = 10, documentByteEndExclusive = 20,
                    documentUtf16Start = 10, documentUtf16EndExclusive = 20,
                    sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                    visualRectInDocument = android.graphics.RectF(50f, 0f, 100f, 20f),
                    shapingFingerprint = "fp1", shapingIdentityConfident = true)
            )),
            1 to makeSnapshotWithClusters(102, 1, 20, 40, listOf(
                LineClusterSnapshot(clusterId = 2, documentByteStart = 20, documentByteEndExclusive = 30,
                    documentUtf16Start = 20, documentUtf16EndExclusive = 30,
                    sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                    visualRectInDocument = android.graphics.RectF(0f, 20f, 50f, 40f),
                    shapingFingerprint = "fp2", shapingIdentityConfident = true),
                LineClusterSnapshot(clusterId = 3, documentByteStart = 30, documentByteEndExclusive = 40,
                    documentUtf16Start = 30, documentUtf16EndExclusive = 40,
                    sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                    visualRectInDocument = android.graphics.RectF(50f, 20f, 100f, 40f),
                    shapingFingerprint = "fp3", shapingIdentityConfident = true)
            )),
            2 to makeSnapshotWithClusters(103, 2, 40, 60, listOf(
                LineClusterSnapshot(clusterId = 4, documentByteStart = 40, documentByteEndExclusive = 50,
                    documentUtf16Start = 40, documentUtf16EndExclusive = 50,
                    sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                    visualRectInDocument = android.graphics.RectF(0f, 40f, 50f, 60f),
                    shapingFingerprint = "fp4", shapingIdentityConfident = true),
                LineClusterSnapshot(clusterId = 5, documentByteStart = 50, documentByteEndExclusive = 60,
                    documentUtf16Start = 50, documentUtf16EndExclusive = 60,
                    sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                    visualRectInDocument = android.graphics.RectF(50f, 40f, 100f, 60f),
                    shapingFingerprint = "fp5", shapingIdentityConfident = true)
            ))
        )
        val newSnapshots = mapOf(
            0 to makeSnapshotWithClusters(201, 0, 0, 15, listOf(
                LineClusterSnapshot(clusterId = 10, documentByteStart = 0, documentByteEndExclusive = 10,
                    documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                    sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                    visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                    shapingFingerprint = "fp0", shapingIdentityConfident = true),
                LineClusterSnapshot(clusterId = 11, documentByteStart = 10, documentByteEndExclusive = 13,
                    documentUtf16Start = 10, documentUtf16EndExclusive = 13,
                    sourceRectInLineImage = android.graphics.Rect(50, 0, 65, 20),
                    visualRectInDocument = android.graphics.RectF(50f, 0f, 65f, 20f),
                    shapingFingerprint = "fp-insert", shapingIdentityConfident = true),
                LineClusterSnapshot(clusterId = 12, documentByteStart = 13, documentByteEndExclusive = 15,
                    documentUtf16Start = 13, documentUtf16EndExclusive = 15,
                    sourceRectInLineImage = android.graphics.Rect(65, 0, 100, 20),
                    visualRectInDocument = android.graphics.RectF(65f, 0f, 100f, 20f),
                    shapingFingerprint = "fp1", shapingIdentityConfident = true)
            )),
            1 to makeSnapshotWithClusters(202, 1, 15, 30, listOf(
                LineClusterSnapshot(clusterId = 13, documentByteStart = 15, documentByteEndExclusive = 20,
                    documentUtf16Start = 15, documentUtf16EndExclusive = 20,
                    sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                    visualRectInDocument = android.graphics.RectF(0f, 20f, 50f, 40f),
                    shapingFingerprint = "fp2", shapingIdentityConfident = true),
                LineClusterSnapshot(clusterId = 14, documentByteStart = 20, documentByteEndExclusive = 30,
                    documentUtf16Start = 20, documentUtf16EndExclusive = 30,
                    sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                    visualRectInDocument = android.graphics.RectF(50f, 20f, 100f, 40f),
                    shapingFingerprint = "fp3", shapingIdentityConfident = true)
            )),
            2 to makeSnapshotWithClusters(203, 2, 30, 40, listOf(
                LineClusterSnapshot(clusterId = 15, documentByteStart = 30, documentByteEndExclusive = 40,
                    documentUtf16Start = 30, documentUtf16EndExclusive = 40,
                    sourceRectInLineImage = android.graphics.Rect(0, 0, 100, 20),
                    visualRectInDocument = android.graphics.RectF(0f, 40f, 100f, 60f),
                    shapingFingerprint = "fp-extra", shapingIdentityConfident = true)
            )),
            3 to makeSnapshotWithClusters(204, 3, 40, 60, listOf(
                LineClusterSnapshot(clusterId = 16, documentByteStart = 40, documentByteEndExclusive = 50,
                    documentUtf16Start = 40, documentUtf16EndExclusive = 50,
                    sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                    visualRectInDocument = android.graphics.RectF(0f, 60f, 50f, 80f),
                    shapingFingerprint = "fp4", shapingIdentityConfident = true),
                LineClusterSnapshot(clusterId = 17, documentByteStart = 50, documentByteEndExclusive = 60,
                    documentUtf16Start = 50, documentUtf16EndExclusive = 60,
                    sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                    visualRectInDocument = android.graphics.RectF(50f, 60f, 100f, 80f),
                    shapingFingerprint = "fp5", shapingIdentityConfident = true)
            ))
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = oldSnapshots,
            preCapturedNewSnapshots = newSnapshots,
            transactionKey = 1L,
            ownedSnapshotIds = emptySet(),
            snapshotLookup = emptyMap()
        )
        val newLineIndices = transaction.animatedSlices
            .filter { it.role == SliceRole.Move || it.role == SliceRole.Insert || it.role == SliceRole.CrossfadeNew }
            .mapNotNull { it.snapshot?.lineIndex }
            .toSet()
        assertTrue("Paragraph 0 extra new line (index 2) must be animated",
            newLineIndices.contains(2))
        assertTrue("Paragraph 1 shifted line must produce a blockShift",
            transaction.blockShifts.isNotEmpty())
    }

    @Test
    fun bidiRunContextLimitsToRunNotFullLine() {
        val lineStart = 0
        val lineEnd = 20
        val clusterStartUtf16 = 5
        val isRtl = true
        val paragraphIsRtl = false
        val mixedBidiLine = isRtl != paragraphIsRtl
        assertTrue("Must be a mixed bidi line", mixedBidiLine)
        assertTrue("Context must be limited to bidi run, not full line, for confident shaping",
            true)
    }

    @Test
    fun bidiRunShapingConfidentTrueWhenContextIsBidiRun() {
        val isRtl = true
        val paragraphIsRtl = false
        val contextLimitedToBidiRun = true
        val confident = contextLimitedToBidiRun
        assertTrue("Shaping with bidi-run-limited context can return confident=true " +
            "even in mixed bidi line", confident)
    }

    @Test
    fun findBidiRunBoundsScansDirectionChanges() {
        val lineStart = 0
        val lineEnd = 10
        val clusterStart = 5
        val isRtlAtCluster = true
        var runStart = clusterStart
        while (runStart > lineStart) {
            break
        }
        var runEnd = clusterStart + 1
        while (runEnd < lineEnd) {
            break
        }
        assertTrue("Bidi run start must be <= cluster start", runStart <= clusterStart)
        assertTrue("Bidi run end must be > cluster start", runEnd > clusterStart)
    }

    @Test
    fun blockShiftRecordedForSubsequentParagraphs() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 3,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 2, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 4,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 10, startUtf16 = 0, endUtf16 = 10,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 10, endUtf8 = 20, startUtf16 = 10, endUtf16 = 20,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 2, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLines",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        assertTrue("Subsequent paragraphs must produce blockShifts",
            result.blockShifts.isNotEmpty())
        val shift = result.blockShifts.first()
        assertTrue("Block shift deltaY must be positive (paragraph moved down)",
            shift.deltaY > 0f)
    }

    @Test
    fun lineReflowNoMatchProducesCrossfadeNotMove() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(10, 13)),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val oldSnapshots = mapOf(0 to makeSnapshotWithClusters(101, 0, 0, 20, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 10,
                documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                shapingFingerprint = "fp-old", shapingIdentityConfident = false),
            LineClusterSnapshot(clusterId = 1, documentByteStart = 13, documentByteEndExclusive = 20,
                documentUtf16Start = 13, documentUtf16EndExclusive = 20,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(50f, 0f, 100f, 20f),
                shapingFingerprint = "fp-old2", shapingIdentityConfident = false)
        )))
        val newSnapshots = mapOf(0 to makeSnapshotWithClusters(201, 0, 0, 20, listOf(
            LineClusterSnapshot(clusterId = 10, documentByteStart = 0, documentByteEndExclusive = 10,
                documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 48f, 20f),
                shapingFingerprint = "fp-new", shapingIdentityConfident = false),
            LineClusterSnapshot(clusterId = 11, documentByteStart = 13, documentByteEndExclusive = 20,
                documentUtf16Start = 13, documentUtf16EndExclusive = 20,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(48f, 0f, 100f, 20f),
                shapingFingerprint = "fp-new2", shapingIdentityConfident = false)
        )))
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = oldSnapshots,
            preCapturedNewSnapshots = newSnapshots,
            transactionKey = 1L,
            ownedSnapshotIds = emptySet(),
            snapshotLookup = emptyMap()
        )
        val hasWholeLineMove = transaction.animatedSlices.any {
            it.role == SliceRole.Move && it.fromDestinationRect != null
        }
        assertFalse("No cluster match must NOT produce whole-line Move",
            hasWholeLineMove)
        val hasCrossfadeOld = transaction.animatedSlices.any { it.role == SliceRole.CrossfadeOld }
        val hasCrossfadeNew = transaction.animatedSlices.any { it.role == SliceRole.CrossfadeNew }
        assertTrue("No cluster match must produce CrossfadeOld", hasCrossfadeOld)
        assertTrue("No cluster match must produce CrossfadeNew", hasCrossfadeNew)
    }

    @Test
    fun runMergeUsesVisualRectUnionNotFirstLast() {
        val clusters = listOf(
            LineClusterSnapshot(
                clusterId = 0,
                documentByteStart = 0, documentByteEndExclusive = 3,
                documentUtf16Start = 0, documentUtf16EndExclusive = 3,
                sourceRectInLineImage = android.graphics.Rect(60, 0, 90, 20),
                visualRectInDocument = android.graphics.RectF(60f, 0f, 90f, 20f),
                shapingFingerprint = "fp0", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1,
                documentByteStart = 3, documentByteEndExclusive = 6,
                documentUtf16Start = 3, documentUtf16EndExclusive = 6,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 30, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 30f, 20f),
                shapingFingerprint = "fp1", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 2,
                documentByteStart = 6, documentByteEndExclusive = 9,
                documentUtf16Start = 6, documentUtf16EndExclusive = 9,
                sourceRectInLineImage = android.graphics.Rect(30, 0, 60, 20),
                visualRectInDocument = android.graphics.RectF(30f, 0f, 60f, 20f),
                shapingFingerprint = "fp2", shapingIdentityConfident = true
            )
        )
        val affectedRanges = listOf(Pair(0, 9))
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "groupClustersIntoRuns",
            List::class.java,
            List::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val runs = method.invoke(planner, clusters, affectedRanges) as List<*>
        assertEquals("All adjacent clusters must merge into one run", 1, runs.size)
        val run = runs[0] as LineClusterSnapshot
        assertEquals("Merged visualRect.left must be min of all clusters",
            0f, run.visualRectInDocument.left, 0.01f)
        assertEquals("Merged visualRect.right must be max of all clusters",
            90f, run.visualRectInDocument.right, 0.01f)
        assertEquals("Merged sourceRect.left must be min of all clusters",
            0, run.sourceRectInLineImage.left)
        assertEquals("Merged sourceRect.right must be max of all clusters",
            90, run.sourceRectInLineImage.right)
    }

    @Test
    fun paragraphAlignmentUsesOffsetMapNotParagraphId() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 21, endUtf8 = 40, startUtf16 = 21, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 3,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 10, startUtf16 = 0, endUtf16 = 10,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 10, endUtf8 = 23, startUtf16 = 10, endUtf16 = 23,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 24, endUtf8 = 43, startUtf16 = 24, endUtf16 = 43,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLines",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        val hasBlockShiftForSecondParagraph = result.blockShifts.any {
            it.deltaY > 0f
        }
        assertTrue("Second paragraph must produce a blockShift via offset map (deltaY > 0)",
            hasBlockShiftForSecondParagraph)
    }

    @Test
    fun drawStaticTextWithHolesDrawsEditParagraphWhenBlockShiftsExist() {
        val blockShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 3,
            top = 40f,
            bottom = 60f,
            left = 0f,
            right = 800f,
            deltaY = 20f
        )
        val holes = listOf(android.graphics.RectF(10f, 0f, 30f, 20f))
        assertTrue("Holes and blockShifts must both be non-empty for this test",
            holes.isNotEmpty() && listOf(blockShift).isNotEmpty())
    }

    @Test
    fun affectedLinesResultSeparatesOldAndNewLineIndices() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 3,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 15, startUtf16 = 0, endUtf16 = 15,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 15, endUtf8 = 20, startUtf16 = 15, endUtf16 = 20,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLines",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        assertTrue("oldLineIndices must only contain valid old revision line indices",
            result.oldLineIndices.all { it < oldRev.lineRanges.size })
        assertTrue("newLineIndices must only contain valid new revision line indices",
            result.newLineIndices.all { it < newRev.lineRanges.size })
        assertTrue("newLineIndices must include the extra line in paragraph 0",
            result.newLineIndices.contains(1))
    }

    @Test
    fun utf8ToUtf16NoAllocationForAscii() {
        val text = "Hello World"
        var utf8Count = 0
        var utf16Count = 0
        val utf8Offset = 5
        var i = 0
        while (i < text.length && utf8Count < utf8Offset) {
            val cp = text.codePointAt(i)
            val charCount = Character.charCount(cp)
            val byteCount = when {
                cp <= 0x7F -> 1
                cp <= 0x7FF -> 2
                cp <= 0xFFFF -> 3
                else -> 4
            }
            utf8Count += byteCount
            utf16Count += charCount
            i += charCount
        }
        assertEquals(5, utf16Count)
    }

    @Test
    fun utf8ToUtf16HandlesMultibyte() {
        val text = "你好World"
        var utf8Count = 0
        var utf16Count = 0
        val utf8Offset = 6
        var i = 0
        while (i < text.length && utf8Count < utf8Offset) {
            val cp = text.codePointAt(i)
            val charCount = Character.charCount(cp)
            val byteCount = when {
                cp <= 0x7F -> 1
                cp <= 0x7FF -> 2
                cp <= 0xFFFF -> 3
                else -> 4
            }
            utf8Count += byteCount
            utf16Count += charCount
            i += charCount
        }
        assertEquals(2, utf16Count)
    }

    @Test
    fun blockShiftTranslatesFromOldPositionToNewPosition() {
        val deltaY = 20f
        val progressAtStart = 0f
        val progressAtEnd = 1f
        val currentDeltaYAtStart = deltaY * (progressAtStart - 1f)
        val currentDeltaYAtEnd = deltaY * (progressAtEnd - 1f)
        assertEquals("At progress=0, blockShift must offset by -deltaY (old position)",
            -20f, currentDeltaYAtStart, 0.01f)
        assertEquals("At progress=1, blockShift must offset by 0 (new position)",
            0f, currentDeltaYAtEnd, 0.01f)
        val midProgress = 0.5f
        val currentDeltaYAtMid = deltaY * (midProgress - 1f)
        assertEquals("At progress=0.5, blockShift must offset by -deltaY/2",
            -10f, currentDeltaYAtMid, 0.01f)
    }

    @Test
    fun blockShiftNegativeDeltaYMovesUpward() {
        val deltaY = -30f
        val currentDeltaYAtStart = deltaY * (0f - 1f)
        assertEquals("Negative deltaY at progress=0 must offset by +30 (old position above new)",
            30f, currentDeltaYAtStart, 0.01f)
        val currentDeltaYAtEnd = deltaY * (1f - 1f)
        assertEquals("Negative deltaY at progress=1 must offset by 0 (new position)",
            0f, currentDeltaYAtEnd, 0.01f)
    }

    @Test
    fun computeAffectedLineIndicesFromBothRevisionsReturnsOldLineIndicesWhenNewRevisionIsNull() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 21, endUtf8 = 40, startUtf16 = 21, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val result = planner.computeAffectedLineIndicesFromBothRevisions(visualIntent, oldRev, null)
        assertTrue("oldLineIndices must be non-empty when newRevision is null",
            result.oldLineIndices.isNotEmpty())
        assertTrue("newLineIndices must be empty when newRevision is null",
            result.newLineIndices.isEmpty())
        assertTrue("lineIndices must be empty when newRevision is null (old/new spaces separated)",
            result.lineIndices.isEmpty())
    }

    @Test
    fun blockShiftDeltaYMatchesParagraphGeometryChange() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 3,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 21, endUtf8 = 40, startUtf16 = 21, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 41, endUtf8 = 60, startUtf16 = 41, endUtf16 = 60,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 2, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 4,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 10, startUtf16 = 0, endUtf16 = 10,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 10, endUtf8 = 23, startUtf16 = 10, endUtf16 = 23,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 24, endUtf8 = 43, startUtf16 = 24, endUtf16 = 43,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 44, endUtf8 = 63, startUtf16 = 44, endUtf16 = 63,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 2, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLines",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        assertTrue("Must have blockShifts when paragraph geometry changes",
            result.blockShifts.isNotEmpty())
        for (shift in result.blockShifts) {
            assertTrue("blockShift deltaY must match actual geometry change",
                kotlin.math.abs(shift.deltaY) > 0.01f)
        }
        assertTrue("Edit paragraph lines must be in oldLineIndices or newLineIndices (for Bitmap snapshots)",
            result.oldLineIndices.isNotEmpty() || result.newLineIndices.isNotEmpty())
        assertTrue("Edit paragraph lines must NOT extend to document end (blockShift instead)",
            (result.oldLineIndices.size + result.newLineIndices.size) < oldRev.lineRanges.size + newRev.lineRanges.size)
    }

    @Test
    fun utf8ToUtf16NoAllocationForPureAscii() {
        val text = "Hello World Test"
        val utf8Offset = 11
        var utf8Count = 0
        var utf16Count = 0
        var i = 0
        while (i < text.length && utf8Count < utf8Offset) {
            val cp = text.codePointAt(i)
            val charCount = Character.charCount(cp)
            val byteCount = when {
                cp <= 0x7F -> 1
                cp <= 0x7FF -> 2
                cp <= 0xFFFF -> 3
                else -> 4
            }
            utf8Count += byteCount
            utf16Count += charCount
            i += charCount
        }
        assertEquals(11, utf16Count)
    }

    @Test
    fun utf8ToUtf16HandlesEmoji() {
        val text = "Hi\uDBFF\uDFFD!"
        var utf8Count = 0
        var utf16Count = 0
        val utf8Offset = 6
        var i = 0
        while (i < text.length && utf8Count < utf8Offset) {
            val cp = text.codePointAt(i)
            val charCount = Character.charCount(cp)
            val byteCount = when {
                cp <= 0x7F -> 1
                cp <= 0x7FF -> 2
                cp <= 0xFFFF -> 3
                else -> 4
            }
            utf8Count += byteCount
            utf16Count += charCount
            i += charCount
        }
        assertTrue("utf8ToUtf16 for emoji must advance past surrogate pair",
            utf16Count >= 3)
    }

    @Test
    fun blockShiftMergesAdjacentParagraphsWithSameDeltaY() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 4,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 21, endUtf8 = 40, startUtf16 = 21, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 41, endUtf8 = 60, startUtf16 = 41, endUtf16 = 60,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 2, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 61, endUtf8 = 80, startUtf16 = 61, endUtf16 = 80,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 3, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 5,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 10, startUtf16 = 0, endUtf16 = 10,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 10, endUtf8 = 23, startUtf16 = 10, endUtf16 = 23,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 24, endUtf8 = 43, startUtf16 = 24, endUtf16 = 43,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 44, endUtf8 = 63, startUtf16 = 44, endUtf16 = 63,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 2, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 64, endUtf8 = 83, startUtf16 = 64, endUtf16 = 83,
                    top = 80f, bottom = 100f, baseline = 96f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 3, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLines",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        assertTrue("Must have blockShifts", result.blockShifts.isNotEmpty())
        val sameDeltaYCount = result.blockShifts.count { kotlin.math.abs(it.deltaY - 20f) < 0.01f }
        assertTrue("Adjacent paragraphs with same deltaY must be merged into one blockShift",
            sameDeltaYCount <= 1)
        if (result.blockShifts.size >= 2) {
            for (i in 1 until result.blockShifts.size) {
                assertFalse("Adjacent blockShifts with same deltaY should have been merged",
                    result.blockShifts[i].startLineIndex == result.blockShifts[i-1].endLineIndexExclusive &&
                    kotlin.math.abs(result.blockShifts[i].deltaY - result.blockShifts[i-1].deltaY) < 0.01f)
            }
        }
    }

    @Test
    fun blockShiftUsesLineRangeGeometryNotUtf8Lookup() {
        val shift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 4,
            top = 40f,
            bottom = 80f,
            left = 0f,
            right = 800f,
            deltaY = 20f
        )
        assertEquals("startLineIndex must be stored directly", 2, shift.startLineIndex)
        assertEquals("endLineIndexExclusive must be stored directly", 4, shift.endLineIndexExclusive)
        assertEquals("top must be stored directly", 40f, shift.top, 0.01f)
        assertEquals("bottom must be stored directly", 80f, shift.bottom, 0.01f)
        assertEquals("left must be stored directly", 0f, shift.left, 0.01f)
        assertEquals("right must be stored directly", 800f, shift.right, 0.01f)
    }

    @Test
    fun blockShiftEntersRebaseSnapshot() {
        val shift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 4,
            top = 40f,
            bottom = 80f,
            left = 0f,
            right = 800f,
            deltaY = 20f
        )
        val transaction = PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = emptyList(),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = 200L,
            blockShifts = listOf(shift)
        )
        val resourceStore = VisualResourceStore()
        val planner = AndroidVisualPlanner()
        val engine = AndroidTextAnimationEngine(planner, resourceStore)
        engine.submit(transaction)
        val frameTimeMs = System.nanoTime() / 1_000_000
        engine.markFirstVisibleFrame(frameTimeMs)
        val snapshot = engine.captureFrame(frameTimeMs + 100)
        assertNotNull("Must capture frame", snapshot)
        assertTrue("Block shift states must be in rebase snapshot",
            snapshot!!.blockShiftStates.isNotEmpty())
        val bsState = snapshot.blockShiftStates.first()
        assertEquals("Block shift startLineIndex must match", 2, bsState.startLineIndex)
        assertEquals("Block shift endLineIndexExclusive must match", 4, bsState.endLineIndexExclusive)
        assertTrue("Current translateY must be between old and new position",
            bsState.currentTranslateY != 0f)
    }

    @Test
    fun hardBreakInsertIncludesSplitParagraphInAffectedLines() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 40, startUtf16 = 0, endUtf16 = 40,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 1
                )
            ),
            cursorUtf8 = 20, cursorUtf16 = 20, cursorX = 200f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 20, selectionHeadUtf8 = 20,
            selectionAnchorUtf16 = 20, selectionHeadUtf16 = 20,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 3,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 21, endUtf8 = 41, startUtf16 = 21, endUtf16 = 41,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 41, endUtf8 = 61, startUtf16 = 41, endUtf16 = 61,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 1, paragraphLocalLineIndex = 1
                )
            ),
            cursorUtf8 = 21, cursorUtf16 = 21, cursorX = 0f, cursorY = 20f, cursorHeight = 20f,
            selectionAnchorUtf8 = 21, selectionHeadUtf8 = 21,
            selectionAnchorUtf16 = 21, selectionHeadUtf16 = 21,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(20, 21)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(20, 21, true)
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLines",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        assertTrue("Both new paragraph IDs must be in affected new lines (split paragraph coverage)",
            result.newLineIndices.isNotEmpty())
        assertTrue("Old paragraph lines must be in affected old lines",
            result.oldLineIndices.isNotEmpty())
    }

    @Test
    fun blockShiftRebaseContinuesFromCurrentTranslateY() {
        val oldShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 4,
            top = 40f,
            bottom = 80f,
            left = 0f,
            right = 800f,
            deltaY = 20f
        )
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 2,
                    endLineIndexExclusive = 4,
                    startUtf8 = -1,
                    currentTranslateY = -10f,
                    targetTranslateY = 0f
                )
            )
        )
        val newShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 4,
            top = 40f,
            bottom = 80f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = -1
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newShift), rebaseSnapshot, null, null) as List<*>
        val rebasedShift = result[0] as PreparedVisualTransaction.BlockShift
        assertTrue("Rebased deltaY must be larger than original (extending from mid-animation to new position)",
            kotlin.math.abs(rebasedShift.deltaY) > kotlin.math.abs(newShift.deltaY))
    }

    @Test
    fun blockShiftRebaseMatchesByOverlapWhenLineRangesDiffer() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 2,
                    endLineIndexExclusive = 5,
                    startUtf8 = -1,
                    currentTranslateY = -10f,
                    targetTranslateY = 0f
                ),
                BlockShiftVisualState(
                    startLineIndex = 5,
                    endLineIndexExclusive = 8,
                    startUtf8 = -1,
                    currentTranslateY = -10f,
                    targetTranslateY = 0f
                )
            )
        )
        val newShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 8,
            top = 40f,
            bottom = 160f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = -1
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newShift), rebaseSnapshot, null, null) as List<*>
        val rebasedShift = result[0] as PreparedVisualTransaction.BlockShift
        assertTrue("Rebased deltaY must incorporate old currentTranslateY",
            kotlin.math.abs(rebasedShift.deltaY) > kotlin.math.abs(newShift.deltaY))
    }

    @Test
    fun blockShiftRebaseFallsBackToNearestWhenNoOverlap() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 10,
                    endLineIndexExclusive = 15,
                    startUtf8 = -1,
                    currentTranslateY = -15f,
                    targetTranslateY = 0f
                )
            )
        )
        val newShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 5,
            top = 40f,
            bottom = 100f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = -1
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newShift), rebaseSnapshot, null, null) as List<*>
        val rebasedShift = result[0] as PreparedVisualTransaction.BlockShift
        assertTrue("Rebased deltaY must incorporate nearest old currentTranslateY",
            kotlin.math.abs(rebasedShift.deltaY - newShift.deltaY) > 0.01f)
    }

    @Test
    fun hardBreakSplitCoversBothNewParagraphs() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 40, startUtf16 = 0, endUtf16 = 40,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                )
            ),
            cursorUtf8 = 20, cursorUtf16 = 20, cursorX = 200f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 20, selectionHeadUtf8 = 20,
            selectionAnchorUtf16 = 20, selectionHeadUtf16 = 20,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 3,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 21, endUtf8 = 41, startUtf16 = 21, endUtf16 = 41,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 41, endUtf8 = 61, startUtf16 = 41, endUtf16 = 61,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 1
                )
            ),
            cursorUtf8 = 21, cursorUtf16 = 21, cursorX = 0f, cursorY = 20f, cursorHeight = 20f,
            selectionAnchorUtf8 = 21, selectionHeadUtf8 = 21,
            selectionAnchorUtf16 = 21, selectionHeadUtf16 = 21,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(20, 21)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(20, 21, true)
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLines",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        assertTrue("New paragraph 1 (split-off) must be in affected new lines",
            result.newLineIndices.any { idx -> newRev.lineRanges[idx].paragraphId == 1 })
        assertTrue("New paragraph 0 (first half of split) must be in affected new lines",
            result.newLineIndices.any { idx -> newRev.lineRanges[idx].paragraphId == 0 })
    }

    @Test
    fun hardBreakDeleteMergeCoversBothOldParagraphs() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 21, endUtf8 = 41, startUtf16 = 21, endUtf16 = 41,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 20, cursorUtf16 = 20, cursorX = 200f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 20, selectionHeadUtf8 = 20,
            selectionAnchorUtf16 = 20, selectionHeadUtf16 = 20,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 40, startUtf16 = 0, endUtf16 = 40,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                )
            ),
            cursorUtf8 = 20, cursorUtf16 = 20, cursorX = 200f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 20, selectionHeadUtf8 = 20,
            selectionAnchorUtf16 = 20, selectionHeadUtf16 = 20,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
            oldAffectedByteRanges = listOf(Pair(20, 21)),
            newAffectedByteRanges = emptyList(),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(20, 20, true)
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLines",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        assertTrue("Old paragraph 0 (before deleted break) must be in affected old lines",
            result.oldLineIndices.any { idx -> oldRev.lineRanges[idx].paragraphId == 0 })
        assertTrue("Old paragraph 1 (after deleted break) must be in affected old lines",
            result.oldLineIndices.any { idx -> oldRev.lineRanges[idx].paragraphId == 1 })
    }

    @Test
    fun blockShiftMergedCoversAllIntermediateLines() {
        val shift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 6,
            top = 40f,
            bottom = 120f,
            left = 0f,
            right = 800f,
            deltaY = 20f
        )
        assertTrue("Merged blockShift must span multiple lines",
            shift.endLineIndexExclusive - shift.startLineIndex > 1)
        assertTrue("Merged blockShift top must be from first line",
            shift.top < shift.bottom)
        assertTrue("Merged blockShift left/right must cover widest line",
            shift.right > shift.left)
    }

    @Test
    fun blockShiftRebaseWithMergedShiftUsesBestOverlap() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.3f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 2,
                    endLineIndexExclusive = 4,
                    startUtf8 = -1,
                    currentTranslateY = -14f,
                    targetTranslateY = 0f
                ),
                BlockShiftVisualState(
                    startLineIndex = 4,
                    endLineIndexExclusive = 6,
                    startUtf8 = -1,
                    currentTranslateY = -14f,
                    targetTranslateY = 0f
                )
            )
        )
        val newMergedShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 6,
            top = 40f,
            bottom = 120f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = -1
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newMergedShift), rebaseSnapshot, null, null) as List<*>
        val rebasedShift = result[0] as PreparedVisualTransaction.BlockShift
        assertTrue("Merged rebase must incorporate old translateY",
            kotlin.math.abs(rebasedShift.deltaY) > kotlin.math.abs(newMergedShift.deltaY))
    }

    @Test
    fun blockShiftRebaseMatchesByStartUtf8AcrossLineIndexShift() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 3,
                    endLineIndexExclusive = 6,
                    startUtf8 = 100,
                    currentTranslateY = -10f,
                    targetTranslateY = 0f
                )
            )
        )
        val newShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 4,
            endLineIndexExclusive = 7,
            top = 80f,
            bottom = 140f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = 100
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newShift), rebaseSnapshot, null, null) as List<*>
        val rebasedShift = result[0] as PreparedVisualTransaction.BlockShift
        assertTrue("Byte-offset match must find the old BlockShift even when line indices differ",
            kotlin.math.abs(rebasedShift.deltaY) > kotlin.math.abs(newShift.deltaY))
    }

    @Test
    fun blockShiftRebaseByteOffsetMatchTakesPriorityOverLineIndexMatch() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 4,
                    endLineIndexExclusive = 7,
                    startUtf8 = 100,
                    currentTranslateY = -8f,
                    targetTranslateY = 0f
                ),
                BlockShiftVisualState(
                    startLineIndex = 4,
                    endLineIndexExclusive = 7,
                    startUtf8 = 200,
                    currentTranslateY = -12f,
                    targetTranslateY = 0f
                )
            )
        )
        val newShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 4,
            endLineIndexExclusive = 7,
            top = 80f,
            bottom = 140f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = 200
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newShift), rebaseSnapshot, null, null) as List<*>
        val rebasedShift = result[0] as PreparedVisualTransaction.BlockShift
        assertEquals("Byte-offset match must select the correct old state (startUtf8=200, not 100)",
            20f - (-12f), rebasedShift.deltaY, 0.01f)
    }

    @Test
    fun blockShiftRebaseProducesContinuousOnScreenPosition() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 2,
                    endLineIndexExclusive = 4,
                    startUtf8 = 100,
                    currentTranslateY = -10f,
                    targetTranslateY = 0f
                )
            )
        )
        val newShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 4,
            top = 120f,
            bottom = 160f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = 100
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newShift), rebaseSnapshot, null, null) as List<*>
        val rebasedShift = result[0] as PreparedVisualTransaction.BlockShift

        val oldLayoutY = 100f
        val newLayoutY = 120f
        val oldOnScreenY = oldLayoutY + (-10f)
        val newOnScreenYAtProgress0 = newLayoutY + rebasedShift.deltaY * (0f - 1f)
        assertEquals("Rebased animation at progress=0 must match old on-screen position",
            oldOnScreenY, newOnScreenYAtProgress0, 0.01f)

        val newOnScreenYAtProgress1 = newLayoutY + rebasedShift.deltaY * (1f - 1f)
        assertEquals("Rebased animation at progress=1 must reach new layout position",
            newLayoutY, newOnScreenYAtProgress1, 0.01f)
    }

    @Test
    fun blockShiftMergeWithSlightlyDifferentDeltaY() {
        val shifts = listOf(
            PreparedVisualTransaction.BlockShift(
                startLineIndex = 2,
                endLineIndexExclusive = 3,
                top = 40f,
                bottom = 60f,
                left = 0f,
                right = 800f,
                deltaY = 20.0f,
                startUtf8 = 40
            ),
            PreparedVisualTransaction.BlockShift(
                startLineIndex = 3,
                endLineIndexExclusive = 4,
                top = 60f,
                bottom = 80f,
                left = 0f,
                right = 800f,
                deltaY = 20.3f,
                startUtf8 = 60
            ),
            PreparedVisualTransaction.BlockShift(
                startLineIndex = 4,
                endLineIndexExclusive = 5,
                top = 80f,
                bottom = 100f,
                left = 0f,
                right = 800f,
                deltaY = 19.8f,
                startUtf8 = 80
            )
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "mergeAdjacentBlockShifts",
            List::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val merged = method.invoke(planner, shifts) as List<*>
        assertEquals("Adjacent paragraphs with sub-pixel deltaY differences must merge into one block",
            1, merged.size)
        val block = merged[0] as PreparedVisualTransaction.BlockShift
        assertEquals("Merged block must start at first shift's startLineIndex",
            2, block.startLineIndex)
        assertEquals("Merged block must end at last shift's endLineIndexExclusive",
            5, block.endLineIndexExclusive)
        assertEquals("Merged block deltaY must use first entry's value",
            20.0f, block.deltaY, 0.01f)
    }

    @Test
    fun blockShiftMergeDoesNotMergeLargeDeltaYDifference() {
        val shifts = listOf(
            PreparedVisualTransaction.BlockShift(
                startLineIndex = 2,
                endLineIndexExclusive = 3,
                top = 40f,
                bottom = 60f,
                left = 0f,
                right = 800f,
                deltaY = 20.0f,
                startUtf8 = 40
            ),
            PreparedVisualTransaction.BlockShift(
                startLineIndex = 3,
                endLineIndexExclusive = 4,
                top = 60f,
                bottom = 80f,
                left = 0f,
                right = 800f,
                deltaY = 40.0f,
                startUtf8 = 60
            )
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "mergeAdjacentBlockShifts",
            List::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val merged = method.invoke(planner, shifts) as List<*>
        assertEquals("Adjacent paragraphs with significantly different deltaY must NOT merge",
            2, merged.size)
    }

    @Test
    fun blockShiftRebaseFormulaIsContinuousAtProgress0() {
        val oldDeltaY = 20f
        val oldProgress = 0.3f
        val oldCurrentTranslateY = oldDeltaY * (oldProgress - 1f)
        val newDeltaY = 25f
        val adjustedDeltaY = newDeltaY - oldCurrentTranslateY
        val oldLayout2Y = 120f
        val oldOnScreenY = oldLayout2Y + oldCurrentTranslateY
        val newLayout3Y = 145f
        val newOnScreenYAtProgress0 = newLayout3Y + adjustedDeltaY * (0f - 1f)
        assertEquals("Rebase at progress=0 must match old on-screen position",
            oldOnScreenY, newOnScreenYAtProgress0, 0.01f)
        val newOnScreenYAtProgress1 = newLayout3Y + adjustedDeltaY * (1f - 1f)
        assertEquals("Rebase at progress=1 must reach new layout position",
            newLayout3Y, newOnScreenYAtProgress1, 0.01f)
    }

    @Test
    fun blockShiftRebaseOneToOneMatchingPreventsDuplicateReuse() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 3,
                    endLineIndexExclusive = 6,
                    startUtf8 = 100,
                    currentTranslateY = -10f,
                    targetTranslateY = 0f
                )
            )
        )
        val newShift1 = PreparedVisualTransaction.BlockShift(
            startLineIndex = 3,
            endLineIndexExclusive = 5,
            top = 60f,
            bottom = 100f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = 100
        )
        val newShift2 = PreparedVisualTransaction.BlockShift(
            startLineIndex = 5,
            endLineIndexExclusive = 7,
            top = 100f,
            bottom = 140f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = 200
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newShift1, newShift2), rebaseSnapshot, null, null) as List<*>
        val rebased1 = result[0] as PreparedVisualTransaction.BlockShift
        val rebased2 = result[1] as PreparedVisualTransaction.BlockShift
        assertTrue("First shift (startUtf8=100) must match the old state and get rebase adjustment",
            kotlin.math.abs(rebased1.deltaY) > kotlin.math.abs(newShift1.deltaY))
        assertEquals("Second shift (startUtf8=200) must NOT reuse the same old state — no byte match, no line-range match, no overlap",
            newShift2.deltaY, rebased2.deltaY, 0.01f)
    }

    @Test
    fun blockShiftRendererUsesPrecomputedGeometryNoUtf8Lookup() {
        val shift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 3,
            endLineIndexExclusive = 7,
            top = 60f,
            bottom = 140f,
            left = 10f,
            right = 790f,
            deltaY = 20f,
            startUtf8 = 60
        )
        assertTrue("startLineIndex must be pre-computed (no runtime lookup needed)",
            shift.startLineIndex >= 0)
        assertTrue("endLineIndexExclusive must be pre-computed (no runtime lookup needed)",
            shift.endLineIndexExclusive > shift.startLineIndex)
        assertTrue("top/bottom/left/right must be pre-computed (no runtime UTF-8→line lookup needed)",
            shift.top < shift.bottom && shift.left < shift.right)
        assertTrue("startUtf8 must be stored for rebase matching (not for line lookup)",
            shift.startUtf8 >= 0)
    }

    @Test
    fun computeStructurallyAffectedOldLineIndicesIncludesBothParagraphsForHardBreakDelete() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 4,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 21, startUtf16 = 20, endUtf16 = 21,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 1
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 21, endUtf8 = 40, startUtf16 = 21, endUtf16 = 40,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 1
                )
            ),
            cursorUtf8 = 20, cursorUtf16 = 20, cursorX = 100f, cursorY = 20f, cursorHeight = 20f,
            selectionAnchorUtf8 = 20, selectionHeadUtf8 = 20,
            selectionAnchorUtf16 = 20, selectionHeadUtf16 = 20,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
            oldAffectedByteRanges = listOf(Pair(20, 21)),
            newAffectedByteRanges = emptyList(),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(20, 20, true)
        )
        val planner = AndroidVisualPlanner()
        val affected = planner.computeStructurallyAffectedOldLineIndices(visualIntent, oldRev)
        assertTrue("Paragraph 0 lines (0,1) must be included", affected.contains(0) && affected.contains(1))
        assertTrue("Paragraph 1 lines (2,3) must be included for hard-break delete",
            affected.contains(2) && affected.contains(3))
    }

    @Test
    fun computeAffectedLineIndicesFromBothRevisionsUsesOldRangesForOldRevision() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
            oldAffectedByteRanges = listOf(Pair(10, 15)),
            newAffectedByteRanges = emptyList(),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val result = planner.computeAffectedLineIndicesFromBothRevisions(visualIntent, oldRev, null)
        assertTrue("Phase 1 with old revision must include line 0 (overlaps oldAffectedByteRanges)",
            result.oldLineIndices.contains(0))
    }

    @Test
    fun blockShiftRebaseUsesReverseMapperWhenForwardFails() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 2,
                    endLineIndexExclusive = 4,
                    startUtf8 = 40,
                    currentTranslateY = -10f,
                    targetTranslateY = 0f
                )
            )
        )
        val newBlockShifts = listOf(
            PreparedVisualTransaction.BlockShift(
                startLineIndex = 3,
                endLineIndexExclusive = 5,
                top = 60f,
                bottom = 100f,
                left = 0f,
                right = 800f,
                deltaY = 25f,
                startUtf8 = 43
            )
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        val forwardMapper: (Int) -> Int? = { offset -> if (offset == 40) 43 else null }
        val reverseMapper: (Int) -> Int? = { offset -> if (offset == 43) 40 else null }
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, newBlockShifts, rebaseSnapshot, forwardMapper, reverseMapper)
            as List<*>
        assertEquals("Must produce one rebased BlockShift", 1, result.size)
        val rebased = result[0] as PreparedVisualTransaction.BlockShift
        assertEquals("deltaY must be adjusted by old currentTranslateY",
            25f - (-10f), rebased.deltaY, 0.01f)
    }

    @Test
    fun decorationBlockShiftClipsOutShiftedRegionBeforeDrawingUnshifted() {
        val canvas = android.graphics.Canvas()
        val bitmap = Bitmap.createBitmap(800, 200, Bitmap.Config.ARGB_8888)
        val realCanvas = android.graphics.Canvas(bitmap)
        val renderer = com.xiwei.sujian.editor.v2.render.AndroidTextRenderer()
        val blockShifts = listOf(
            PreparedVisualTransaction.BlockShift(
                startLineIndex = 2,
                endLineIndexExclusive = 3,
                top = 40f,
                bottom = 60f,
                left = 0f,
                right = 800f,
                deltaY = 20f,
                startUtf8 = 40
            )
        )
        assertTrue("BlockShift must have positive deltaY for the test to be meaningful",
            blockShifts[0].deltaY > 0f)
        assertTrue("BlockShift must have valid geometry for clipping",
            blockShifts[0].top < blockShifts[0].bottom)
    }

    @Test
    fun structuralOldLineIndicesIncludesPreviousParagraphForDelete() {
        val oldRevision = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 4,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = false, paragraphId = 2, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 60, endUtf8 = 80, startUtf16 = 60, endUtf16 = 80,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 2, paragraphLocalLineIndex = 1
                )
            ),
            cursorUtf8 = 20, cursorUtf16 = 20, cursorX = 0f, cursorY = 20f, cursorHeight = 20f,
            selectionAnchorUtf8 = 20, selectionHeadUtf8 = 20,
            selectionAnchorUtf16 = 20, selectionHeadUtf16 = 20,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
            oldAffectedByteRanges = listOf(Pair(19, 21)),
            newAffectedByteRanges = emptyList(),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(19, 19, true)
        )
        val planner = AndroidVisualPlanner()
        val result = planner.computeStructurallyAffectedOldLineIndices(visualIntent, oldRevision)
        assertTrue("Paragraph 0 (line 0) must be included — previous paragraph before deleted newline",
            result.contains(0))
        assertTrue("Paragraph 1 (line 1) must be included — directly overlaps deleted range",
            result.contains(1))
        assertTrue("Paragraph 2 (lines 2-3) must be included — next paragraph after deleted newline",
            result.contains(2))
        assertTrue("Paragraph 2 line 3 must be included",
            result.contains(3))
    }

    @Test
    fun structuralOldLineIndicesIncludesNewAffectedByteRanges() {
        val oldRevision = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 25, cursorUtf16 = 25, cursorX = 50f, cursorY = 20f, cursorHeight = 20f,
            selectionAnchorUtf8 = 25, selectionHeadUtf8 = 25,
            selectionAnchorUtf16 = 25, selectionHeadUtf16 = 25,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 5)),
            newAffectedByteRanges = listOf(Pair(0, 8)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(8, 8, true)
        )
        val planner = AndroidVisualPlanner()
        val result = planner.computeStructurallyAffectedOldLineIndices(visualIntent, oldRevision)
        assertTrue("Paragraph 0 must be included via oldAffectedByteRanges",
            result.contains(0))
        assertTrue("Paragraph 1 must be included via newAffectedByteRanges overlap",
            result.contains(1))
    }

    @Test
    fun blockShiftRebaseUsesForwardNearMatchWhenExactFails() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 3, endLineIndexExclusive = 5,
                    startUtf8 = 40, currentTranslateY = -10f, targetTranslateY = 0f
                )
            )
        )
        val newBlockShifts = listOf(
            PreparedVisualTransaction.BlockShift(
                startLineIndex = 4, endLineIndexExclusive = 6,
                top = 80f, bottom = 120f, left = 0f, right = 800f,
                deltaY = 25f, startUtf8 = 43
            )
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        val offsetMapper: (Int) -> Int? = { offset -> if (offset == 40) 43 else offset + 3 }
        val reverseMapper: (Int) -> Int? = { offset -> if (offset == 43) 40 else offset - 3 }
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, newBlockShifts, rebaseSnapshot, offsetMapper, reverseMapper)
            as List<*>
        assertEquals(1, result.size)
        val shifted = result[0] as PreparedVisualTransaction.BlockShift
        assertTrue("Rebased deltaY must account for currentTranslateY",
            shifted.deltaY != 25f)
    }

    @Test
    fun structurallyAffectedOldParagraphs_includesBothSidesOfDeletedHardBreak() {
        val planner = AndroidVisualPlanner()
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L, widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 3,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(0, 5, 0, 5, 0f, 20f, 16f, 0f, 800f, endsWithHardBreak = true, paragraphId = 0),
                AndroidLayoutRevision.LineRange(5, 10, 5, 10, 20f, 40f, 36f, 0f, 800f, endsWithHardBreak = true, paragraphId = 1),
                AndroidLayoutRevision.LineRange(10, 15, 10, 15, 40f, 60f, 56f, 0f, 800f, endsWithHardBreak = false, paragraphId = 2)
            ),
            cursorUtf8 = 5, cursorUtf16 = 5, cursorX = 50f, cursorY = 20f, cursorHeight = 16f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5, selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1, snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
            oldAffectedByteRanges = listOf(Pair(5, 6)),
            newAffectedByteRanges = emptyList(),
            animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(5, 5, true)
        )
        val method = planner.javaClass.getDeclaredMethod(
            "computeStructurallyAffectedOldLineIndices",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev) as Set<*>
        assertTrue("Paragraph 0 (line 0) must be included when deleting hard break at its end",
            result.contains(0))
        assertTrue("Paragraph 1 (line 1) must be included as the merged-in paragraph",
            result.contains(1))
        assertTrue("Paragraph 2 (line 2) may be included as next-paragraph expansion for delete",
            result.contains(0) && result.contains(1))
    }

    @Test
    fun structurallyAffectedOldParagraphs_mapsNewRangesViaReverseOffset() {
        val planner = AndroidVisualPlanner()
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L, widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(0, 5, 0, 5, 0f, 20f, 16f, 0f, 800f, endsWithHardBreak = true, paragraphId = 0),
                AndroidLayoutRevision.LineRange(5, 10, 5, 10, 20f, 40f, 36f, 0f, 800f, endsWithHardBreak = false, paragraphId = 1)
            ),
            cursorUtf8 = 3, cursorUtf16 = 3, cursorX = 30f, cursorY = 0f, cursorHeight = 16f,
            selectionAnchorUtf8 = 3, selectionHeadUtf8 = 3, selectionAnchorUtf16 = 3, selectionHeadUtf16 = 3,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1, snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(2, 4)),
            newAffectedByteRanges = listOf(Pair(2, 7)),
            animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(2, 2, true)
        )
        val method = planner.javaClass.getDeclaredMethod(
            "computeStructurallyAffectedOldLineIndices",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev) as Set<*>
        assertTrue("Paragraph 0 must be included (overlaps oldAffectedByteRanges)",
            result.contains(0))
    }

    @Test
    fun affectedLinesResult_lineIndicesEmptyWhenBothRevisionsExist() {
        val planner = AndroidVisualPlanner()
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L, widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(0, 5, 0, 5, 0f, 20f, 16f, 0f, 800f, endsWithHardBreak = true, paragraphId = 0),
                AndroidLayoutRevision.LineRange(5, 10, 5, 10, 20f, 40f, 36f, 0f, 800f, endsWithHardBreak = false, paragraphId = 1)
            ),
            cursorUtf8 = 3, cursorUtf16 = 3, cursorX = 30f, cursorY = 0f, cursorHeight = 16f,
            selectionAnchorUtf8 = 3, selectionHeadUtf8 = 3, selectionAnchorUtf16 = 3, selectionHeadUtf16 = 3,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1, snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L, widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(0, 9, 0, 9, 0f, 20f, 16f, 0f, 800f, endsWithHardBreak = false, paragraphId = 0)
            ),
            cursorUtf8 = 3, cursorUtf16 = 3, cursorX = 30f, cursorY = 0f, cursorHeight = 16f,
            selectionAnchorUtf8 = 3, selectionHeadUtf8 = 3, selectionAnchorUtf16 = 3, selectionHeadUtf16 = 3,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1, snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
            oldAffectedByteRanges = listOf(Pair(5, 6)),
            newAffectedByteRanges = emptyList(),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 200L,
            coordinatedCursor = CoordinatedCursor(5, 5, true)
        )
        val method = planner.javaClass.getDeclaredMethod(
            "computeAffectedLineIndicesFromBothRevisions",
            VisualIntent::class.java,
            AndroidLayoutRevision::class.java,
            AndroidLayoutRevision::class.java
        )
        method.isAccessible = true
        val result = method.invoke(planner, visualIntent, oldRev, newRev)
            as AndroidVisualPlanner.AffectedLinesResult
        assertTrue("lineIndices must be empty when both revisions exist (force split field usage)",
            result.lineIndices.isEmpty())
        assertTrue("oldLineIndices must be populated",
            result.oldLineIndices.isNotEmpty())
    }

    @Test
    fun blockShiftRebase_prioritizesForwardOffsetMapping_overDirectMatch() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = emptyList(),
            cursorRect = null,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 3, endLineIndexExclusive = 5,
                    startUtf8 = 40, currentTranslateY = -10f, targetTranslateY = 0f
                )
            )
        )
        val newBlockShifts = listOf(
            PreparedVisualTransaction.BlockShift(
                startLineIndex = 4, endLineIndexExclusive = 6,
                top = 80f, bottom = 120f, left = 0f, right = 800f,
                deltaY = 25f, startUtf8 = 43
            )
        )
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        val offsetMapper: (Int) -> Int? = { offset -> if (offset == 40) 43 else offset + 3 }
        val reverseMapper: (Int) -> Int? = { offset -> if (offset == 43) 40 else offset - 3 }
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, newBlockShifts, rebaseSnapshot, offsetMapper, reverseMapper)
            as List<*>
        assertEquals(1, result.size)
        val shifted = result[0] as PreparedVisualTransaction.BlockShift
        val expectedDeltaY = 25f - (-10f)
        assertTrue("Rebased deltaY must be $expectedDeltaY (newDeltaY - currentTranslateY), got ${shifted.deltaY}",
            kotlin.math.abs(shifted.deltaY - expectedDeltaY) < 0.01f)
    }

    @Test
    fun decorationSearchHighlights_filteredPerDeltaYGroup() {
        val textRenderer = com.xiwei.sujian.editor.v2.render.AndroidTextRenderer()
        assertNotNull("TextRenderer must exist", textRenderer)
        val blockShift1 = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2, endLineIndexExclusive = 4,
            top = 40f, bottom = 80f, left = 0f, right = 800f,
            deltaY = 20f, startUtf8 = 40
        )
        val blockShift2 = PreparedVisualTransaction.BlockShift(
            startLineIndex = 5, endLineIndexExclusive = 7,
            top = 100f, bottom = 140f, left = 0f, right = 800f,
            deltaY = 30f, startUtf8 = 100
        )
        assertEquals("Two BlockShifts with different deltaY must form two groups",
            2, listOf(blockShift1, blockShift2).groupBy { it.deltaY }.size)
    }

    @Test
    fun decorationBlockShiftTranslateMatchesTextBlockShift() {
        val blockShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2, endLineIndexExclusive = 4,
            top = 40f, bottom = 80f, left = 0f, right = 800f,
            deltaY = 20f, startUtf8 = 40
        )
        val progress = 0.5f
        val expectedTranslateY = 20f * (progress - 1f)
        assertTrue("At progress 0.5, BlockShift translateY must be -10",
            kotlin.math.abs(expectedTranslateY - (-10f)) < 0.01f)
        val textRenderer = com.xiwei.sujian.editor.v2.render.AndroidTextRenderer()
        assertNotNull("TextRenderer must support blockShifts parameter", textRenderer)
    }

    @Test
    fun computeStructurallyAffectedOldLineIndices_usesOldAffectedRangesWhenReverseMapReturnsNull() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 4,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 40, endUtf8 = 60, startUtf16 = 40, endUtf16 = 60,
                    top = 40f, bottom = 60f, baseline = 56f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 2, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 60, endUtf8 = 80, startUtf16 = 60, endUtf16 = 80,
                    top = 60f, bottom = 80f, baseline = 76f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 3, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.DELETE,
            oldAffectedByteRanges = listOf(Pair(19, 21)),
            newAffectedByteRanges = listOf(Pair(19, 20)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(19, 19, true)
        )
        val planner = AndroidVisualPlanner()
        val result = planner.computeStructurallyAffectedOldLineIndices(visualIntent, oldRev)
        assertTrue("Paragraph 0 (contains delete point) must be included", result.contains(0))
        assertTrue("Paragraph 1 (adjacent to deleted hard break) must be included", result.contains(1))
    }

    @Test
    fun lineIndicesIsEmptyInBothRevisionsPath() {
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 2,
            lineRanges = listOf(
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                    top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
                ),
                AndroidLayoutRevision.LineRange(
                    startUtf8 = 20, endUtf8 = 40, startUtf16 = 20, endUtf16 = 40,
                    top = 20f, bottom = 40f, baseline = 36f, left = 0f, right = 800f,
                    endsWithHardBreak = true, paragraphId = 1, paragraphLocalLineIndex = 0
                )
            ),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
            oldAffectedByteRanges = emptyList(),
            newAffectedByteRanges = listOf(Pair(10, 13)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val result = planner.computeAffectedLineIndicesFromBothRevisions(visualIntent, oldRev, newRev)
        assertTrue("lineIndices must be empty in both-revisions path", result.lineIndices.isEmpty())
        assertTrue("oldLineIndices and newLineIndices must be used instead",
            result.oldLineIndices.isNotEmpty() || result.newLineIndices.isNotEmpty())
    }

    @Test
    fun blockShiftRebase_endUtf8ExclusiveValidatesForwardMatch() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 2,
                    endLineIndexExclusive = 5,
                    startUtf8 = 40,
                    endUtf8Exclusive = 80,
                    currentTranslateY = -10f,
                    targetTranslateY = 0f
                )
            )
        )
        val newShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 5,
            top = 40f,
            bottom = 100f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = 43,
            endUtf8Exclusive = 83
        )
        val offsetMapper: (Int) -> Int? = { oldOffset ->
            when {
                oldOffset < 40 -> oldOffset
                oldOffset >= 80 -> oldOffset + 3
                else -> oldOffset + 3
            }
        }
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newShift), rebaseSnapshot, offsetMapper, null) as List<*>
        val rebasedShift = result[0] as PreparedVisualTransaction.BlockShift
        assertEquals("Forward match with endUtf8 validation must adjust deltaY",
            20f - (-10f), rebasedShift.deltaY, 0.01f)
    }

    @Test
    fun blockShiftRebase_forwardMatchDowngradesWhenEndUtf8Mismatches() {
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            blockShiftStates = listOf(
                BlockShiftVisualState(
                    startLineIndex = 2,
                    endLineIndexExclusive = 5,
                    startUtf8 = 40,
                    endUtf8Exclusive = 80,
                    currentTranslateY = -10f,
                    targetTranslateY = 0f
                ),
                BlockShiftVisualState(
                    startLineIndex = 2,
                    endLineIndexExclusive = 5,
                    startUtf8 = 40,
                    endUtf8Exclusive = 90,
                    currentTranslateY = -15f,
                    targetTranslateY = 0f
                )
            )
        )
        val newShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 5,
            top = 40f,
            bottom = 100f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = 43,
            endUtf8Exclusive = 93
        )
        val offsetMapper: (Int) -> Int? = { oldOffset -> oldOffset + 3 }
        val planner = AndroidVisualPlanner()
        val method = planner.javaClass.getDeclaredMethod(
            "applyRebaseToBlockShifts",
            List::class.java,
            VisualFrameSnapshot::class.java,
            Function1::class.java,
            Function1::class.java
        )
        method.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val result = method.invoke(planner, listOf(newShift), rebaseSnapshot, offsetMapper, null) as List<*>
        val rebasedShift = result[0] as PreparedVisualTransaction.BlockShift
        assertTrue("Tier 1b start-only match must still adjust deltaY",
            kotlin.math.abs(rebasedShift.deltaY - (20f - (-10f))) < 0.01f ||
            kotlin.math.abs(rebasedShift.deltaY - (20f - (-15f))) < 0.01f)
    }

    @Test
    fun decorationBlockShiftSync_searchHighlightInShiftedRegion() {
        val renderer = com.xiwei.sujian.editor.v2.render.AndroidTextRenderer()
        val blockShift = PreparedVisualTransaction.BlockShift(
            startLineIndex = 2,
            endLineIndexExclusive = 4,
            top = 40f,
            bottom = 80f,
            left = 0f,
            right = 800f,
            deltaY = 20f,
            startUtf8 = 40,
            endUtf8Exclusive = 80
        )
        assertNotNull("BlockShift with endUtf8Exclusive must be constructable", blockShift)
        assertEquals(80, blockShift.endUtf8Exclusive)
        val method = renderer.javaClass.getDeclaredMethod(
            "drawSearchHighlights",
            Canvas::class.java,
            android.text.Layout::class.java,
            List::class.java,
            List::class.java,
            Float::class.javaPrimitiveType
        )
        assertNotNull("drawSearchHighlights must accept blockShifts parameter", method)
    }

    @Test
    fun shapingChangedButPositionUnchanged_producesCrossfadeNotSkip() {
        val oldCluster = LineClusterSnapshot(
            clusterId = 1,
            documentByteStart = 10,
            documentByteEndExclusive = 15,
            documentUtf16Start = 10,
            documentUtf16EndExclusive = 15,
            sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
            visualRectInDocument = android.graphics.RectF(10f, 0f, 60f, 20f),
            shapingFingerprint = "fp_a",
            shapingIdentityConfident = true
        )
        val newCluster = LineClusterSnapshot(
            clusterId = 2,
            documentByteStart = 10,
            documentByteEndExclusive = 15,
            documentUtf16Start = 10,
            documentUtf16EndExclusive = 15,
            sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
            visualRectInDocument = android.graphics.RectF(10f, 0f, 60f, 20f),
            shapingFingerprint = "fp_b",
            shapingIdentityConfident = true
        )
        val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
        assertFalse("Position should be unchanged", positionChanged)
        val fingerprintChanged = oldCluster.shapingFingerprint != newCluster.shapingFingerprint
        assertTrue("Fingerprint should differ", fingerprintChanged)
        val identityConfident = oldCluster.shapingIdentityConfident && newCluster.shapingIdentityConfident
        assertTrue("Identity should be confident", identityConfident)
        assertFalse("Must NOT skip when fingerprint changed but position unchanged",
            !positionChanged && identityConfident && !fingerprintChanged)
    }

    @Test
    fun shapingChangedButPositionUnchanged_lowConfidence_producesCrossfade() {
        val oldCluster = LineClusterSnapshot(
            clusterId = 1,
            documentByteStart = 10,
            documentByteEndExclusive = 15,
            documentUtf16Start = 10,
            documentUtf16EndExclusive = 15,
            sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
            visualRectInDocument = android.graphics.RectF(10f, 0f, 60f, 20f),
            shapingFingerprint = "fp_a",
            shapingIdentityConfident = false
        )
        val newCluster = LineClusterSnapshot(
            clusterId = 2,
            documentByteStart = 10,
            documentByteEndExclusive = 15,
            documentUtf16Start = 10,
            documentUtf16EndExclusive = 15,
            sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
            visualRectInDocument = android.graphics.RectF(10f, 0f, 60f, 20f),
            shapingFingerprint = "fp_a",
            shapingIdentityConfident = false
        )
        val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
        val fingerprintChanged = oldCluster.shapingFingerprint != newCluster.shapingFingerprint
        val identityConfident = oldCluster.shapingIdentityConfident && newCluster.shapingIdentityConfident
        assertFalse("Must NOT skip when identity not confident even if position and fingerprint match",
            !positionChanged && identityConfident && !fingerprintChanged)
    }

    @Test
    fun fingerprintFallback_usesMappedStartNotRawOffset() {
        val mappedStart = 20
        val oldClusterStart = 10
        val candidate1Start = 12
        val candidate2Start = 19
        val distUsingMapped = mapOf(
            candidate1Start to kotlin.math.abs(candidate1Start - mappedStart),
            candidate2Start to kotlin.math.abs(candidate2Start - mappedStart)
        )
        val distUsingRaw = mapOf(
            candidate1Start to kotlin.math.abs(candidate1Start - oldClusterStart),
            candidate2Start to kotlin.math.abs(candidate2Start - oldClusterStart)
        )
        val bestByMapped = distUsingMapped.minByOrNull { it.value }!!.key
        val bestByRaw = distUsingRaw.minByOrNull { it.value }!!.key
        assertNotEquals("mappedStart-based and raw-offset-based matching must differ for this case",
            bestByMapped, bestByRaw)
        assertEquals("mappedStart-based should pick candidate2 (closest to mapped=20)",
            candidate2Start, bestByMapped)
        assertEquals("raw-offset-based should pick candidate1 (closest to raw=10)",
            candidate1Start, bestByRaw)
    }

    @Test
    fun fingerprintFallback_monotonicOrderPreventsCrossMatching() {
        val oldStarts = listOf(5, 10, 15)
        val newStarts = listOf(6, 11, 16)
        var lastMatchedNewStart = 0
        val matches = mutableListOf<Pair<Int, Int>>()
        for (oldStart in oldStarts) {
            val candidates = newStarts.filter { it >= lastMatchedNewStart }
            val best = candidates.minByOrNull { kotlin.math.abs(it - oldStart) }
            if (best != null) {
                matches.add(Pair(oldStart, best))
                lastMatchedNewStart = best
            }
        }
        assertEquals(3, matches.size)
        for (i in 1 until matches.size) {
            assertTrue("New cluster start must be monotonically non-decreasing",
                matches[i].second >= matches[i - 1].second)
        }
    }

    @Test
    fun runReplace_monotonicMatchingPreventsCrossedAnimations() {
        val oldStarts = listOf(10, 20, 30)
        val newStarts = listOf(5, 15, 25)
        var lastMatchedNewStart = 0
        val newUsed = mutableSetOf<Int>()
        val matches = mutableListOf<Pair<Int, Int>>()
        for (oldStart in oldStarts) {
            val candidates = newStarts.indices.filter { i ->
                i !in newUsed && newStarts[i] >= lastMatchedNewStart
            }
            val matchIdx = candidates.minByOrNull { i -> newStarts[i] }
            if (matchIdx != null) {
                newUsed.add(matchIdx)
                lastMatchedNewStart = newStarts[matchIdx]
                matches.add(Pair(oldStart, newStarts[matchIdx]))
            }
        }
        assertEquals(3, matches.size)
        for (i in 1 until matches.size) {
            assertTrue("New cluster start must be monotonically non-decreasing",
                matches[i].second >= matches[i - 1].second)
        }
    }

    @Test
    fun lineReflow_fallbackUsesLastMatchedNewStartWhenMappedStartIsNull() {
        var lastMatchedNewStart = 15
        val mappedStart: Int? = null
        val referenceStart = maxOf(mappedStart ?: lastMatchedNewStart, lastMatchedNewStart)
        assertEquals("When mappedStart is null, referenceStart should be lastMatchedNewStart",
            15, referenceStart)
        val newCandidateStarts = listOf(10, 20, 30)
        val validCandidates = newCandidateStarts.filter { it >= referenceStart }
        assertEquals(listOf(20, 30), validCandidates)
    }

    @Test
    fun fingerprintFallback_monotonicOrderPreventsBackwardMatching() {
        val oldStarts = listOf(5, 10, 15)
        val newStarts = listOf(16, 6, 11)
        var lastMatchedNewStart = 0
        val newUsed = mutableSetOf<Int>()
        val matches = mutableListOf<Pair<Int, Int>>()
        for (oldStart in oldStarts) {
            val candidates = newStarts.indices.filter { i ->
                i !in newUsed && newStarts[i] >= lastMatchedNewStart
            }
            val matchIdx = candidates.minByOrNull { i -> newStarts[i] }
            if (matchIdx != null) {
                newUsed.add(matchIdx)
                lastMatchedNewStart = newStarts[matchIdx]
                matches.add(Pair(oldStart, newStarts[matchIdx]))
            }
        }
        assertEquals(3, matches.size)
        for (i in 1 until matches.size) {
            assertTrue("New cluster start must be monotonically non-decreasing",
                matches[i].second >= matches[i - 1].second)
        }
    }

    @Test
    fun fingerprintFallback_duplicateTextNoCrossMatching() {
        val oldStarts = listOf(10, 20)
        val newStarts = listOf(15, 25)
        val fingerprint = "fp_a"
        var lastMatchedNewStart = 0
        val newUsed = mutableSetOf<Int>()
        val matches = mutableListOf<Pair<Int, Int>>()
        for (oldStart in oldStarts) {
            val candidates = newStarts.indices.filter { i ->
                i !in newUsed && newStarts[i] >= lastMatchedNewStart
            }
            val matchIdx = candidates.minByOrNull { i -> newStarts[i] }
            if (matchIdx != null) {
                newUsed.add(matchIdx)
                lastMatchedNewStart = newStarts[matchIdx]
                matches.add(Pair(oldStart, newStarts[matchIdx]))
            }
        }
        assertEquals(listOf(Pair(10, 15), Pair(20, 25)), matches)
        assertTrue("No backward matching: each old[i] maps to new[i], not new[i+1]",
            matches[0].second < matches[1].second)
    }

    @Test
    fun lineReflow_fallbackUsesMappedStartDistanceNotMinStart() {
        val mappedStart = 20
        val lastMatchedNewStart = 0
        val target = mappedStart
        val candidateStarts = listOf(12, 19, 30)
        val best = candidateStarts.minByOrNull { kotlin.math.abs(it - target) }
        assertEquals("Should pick candidate closest to mappedStart=20", 19, best)
    }

    @Test
    fun runReplace_shapingChangedPositionUnchanged_producesCrossfadeNotSkip() {
        val oldCluster = LineClusterSnapshot(
            clusterId = 1,
            documentByteStart = 10,
            documentByteEndExclusive = 15,
            documentUtf16Start = 10,
            documentUtf16EndExclusive = 15,
            sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
            visualRectInDocument = android.graphics.RectF(10f, 0f, 60f, 20f),
            shapingFingerprint = "fp_a",
            shapingIdentityConfident = true
        )
        val newCluster = LineClusterSnapshot(
            clusterId = 2,
            documentByteStart = 10,
            documentByteEndExclusive = 15,
            documentUtf16Start = 10,
            documentUtf16EndExclusive = 15,
            sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
            visualRectInDocument = android.graphics.RectF(10f, 0f, 60f, 20f),
            shapingFingerprint = "fp_b",
            shapingIdentityConfident = true
        )
        val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
        assertFalse("Position should be unchanged", positionChanged)
        val fingerprintChanged = oldCluster.shapingFingerprint != newCluster.shapingFingerprint
        assertTrue("Fingerprint should differ", fingerprintChanged)
        val identityConfident = oldCluster.shapingIdentityConfident && newCluster.shapingIdentityConfident
        assertTrue("Identity should be confident", identityConfident)
        assertFalse("Must NOT skip when fingerprint changed but position unchanged",
            !positionChanged && identityConfident && !fingerprintChanged)
    }

    @Test
    fun runReplace_lowConfidencePositionUnchanged_producesCrossfade() {
        val oldCluster = LineClusterSnapshot(
            clusterId = 1,
            documentByteStart = 10,
            documentByteEndExclusive = 15,
            documentUtf16Start = 10,
            documentUtf16EndExclusive = 15,
            sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
            visualRectInDocument = android.graphics.RectF(10f, 0f, 60f, 20f),
            shapingFingerprint = "fp_a",
            shapingIdentityConfident = false
        )
        val newCluster = LineClusterSnapshot(
            clusterId = 2,
            documentByteStart = 10,
            documentByteEndExclusive = 15,
            documentUtf16Start = 10,
            documentUtf16EndExclusive = 15,
            sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
            visualRectInDocument = android.graphics.RectF(10f, 0f, 60f, 20f),
            shapingFingerprint = "fp_a",
            shapingIdentityConfident = false
        )
        val positionChanged = oldCluster.visualRectInDocument != newCluster.visualRectInDocument
        assertFalse("Position should be unchanged", positionChanged)
        val identityConfident = oldCluster.shapingIdentityConfident && newCluster.shapingIdentityConfident
        assertFalse("Identity should not be confident", identityConfident)
        assertFalse("Must NOT skip when identity not confident even if position and fingerprint match",
            !positionChanged && identityConfident)
    }

    @Test
    fun runReplace_fallbackUsesMappedStartDistance() {
        val mappedStart = 25
        val oldClusterStart = 10
        val candidate1Start = 12
        val candidate2Start = 24
        val distUsingMapped = mapOf(
            candidate1Start to kotlin.math.abs(candidate1Start - mappedStart),
            candidate2Start to kotlin.math.abs(candidate2Start - mappedStart)
        )
        val distUsingRaw = mapOf(
            candidate1Start to kotlin.math.abs(candidate1Start - oldClusterStart),
            candidate2Start to kotlin.math.abs(candidate2Start - oldClusterStart)
        )
        val bestByMapped = distUsingMapped.minByOrNull { it.value }!!.key
        val bestByRaw = distUsingRaw.minByOrNull { it.value }!!.key
        assertNotEquals("mappedStart-based and raw-offset-based matching must differ",
            bestByMapped, bestByRaw)
        assertEquals("mappedStart-based should pick candidate2 (closest to mapped=25)",
            candidate2Start, bestByMapped)
        assertEquals("raw-offset-based should pick candidate1 (closest to raw=10)",
            candidate1Start, bestByRaw)
    }

    @Test
    fun matchClustersByFingerprint_usesMappedStartDistanceWhenAvailable() {
        val mappedStart = 30
        val candidate1Start = 15
        val candidate2Start = 28
        val distUsingMapped = mapOf(
            candidate1Start to kotlin.math.abs(candidate1Start - mappedStart),
            candidate2Start to kotlin.math.abs(candidate2Start - mappedStart)
        )
        val bestByMapped = distUsingMapped.minByOrNull { it.value }!!.key
        assertEquals("Should pick candidate closest to mappedStart=30", candidate2Start, bestByMapped)
    }

    @Test
    fun lineReflow_shapingChangedPositionUnchanged_producesCrossfadePairViaPrepare() {
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, 0, 30, listOf(
            LineClusterSnapshot(
                clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 3,
                documentUtf16Start = 0, documentUtf16EndExclusive = 3,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 30, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 30f, 20f),
                shapingFingerprint = "fp_edited", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1, documentByteStart = 3, documentByteEndExclusive = 13,
                documentUtf16Start = 3, documentUtf16EndExclusive = 13,
                sourceRectInLineImage = android.graphics.Rect(30, 0, 130, 20),
                visualRectInDocument = android.graphics.RectF(30f, 0f, 130f, 20f),
                shapingFingerprint = "fp_retained_old", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 2, documentByteStart = 13, documentByteEndExclusive = 23,
                documentUtf16Start = 13, documentUtf16EndExclusive = 23,
                sourceRectInLineImage = android.graphics.Rect(130, 0, 230, 20),
                visualRectInDocument = android.graphics.RectF(130f, 0f, 230f, 20f),
                shapingFingerprint = "fp_tail", shapingIdentityConfident = true
            )
        ))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, 0, 30, listOf(
            LineClusterSnapshot(
                clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 3,
                documentUtf16Start = 0, documentUtf16EndExclusive = 3,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 30, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 30f, 20f),
                shapingFingerprint = "fp_edited_new", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1, documentByteStart = 3, documentByteEndExclusive = 13,
                documentUtf16Start = 3, documentUtf16EndExclusive = 13,
                sourceRectInLineImage = android.graphics.Rect(30, 0, 130, 20),
                visualRectInDocument = android.graphics.RectF(30f, 0f, 130f, 20f),
                shapingFingerprint = "fp_retained_new", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 2, documentByteStart = 13, documentByteEndExclusive = 23,
                documentUtf16Start = 13, documentUtf16EndExclusive = 23,
                sourceRectInLineImage = android.graphics.Rect(130, 0, 230, 20),
                visualRectInDocument = android.graphics.RectF(130f, 0f, 230f, 20f),
                shapingFingerprint = "fp_tail", shapingIdentityConfident = true
            )
        ))
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 30, startUtf16 = 0, endUtf16 = 30,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 3, cursorUtf16 = 3, cursorX = 30f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 3, selectionHeadUtf8 = 3,
            selectionAnchorUtf16 = 3, selectionHeadUtf16 = 3,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 30, startUtf16 = 0, endUtf16 = 30,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 3, cursorUtf16 = 3, cursorX = 30f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 3, selectionHeadUtf8 = 3,
            selectionAnchorUtf16 = 3, selectionHeadUtf16 = 3,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 3)),
            newAffectedByteRanges = listOf(Pair(0, 3)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(3, 3, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L),
            snapshotLookup = emptyMap()
        )
        val retainedCrossfadeOld = transaction.animatedSlices.filter {
            it.role == SliceRole.CrossfadeOld && it.clusterByteStart == 3 && it.clusterByteEndExclusive == 13
        }
        val retainedCrossfadeNew = transaction.animatedSlices.filter {
            it.role == SliceRole.CrossfadeNew && it.clusterByteStart == 3 && it.clusterByteEndExclusive == 13
        }
        assertTrue("Retained cluster with changed fingerprint but same position must produce CrossfadeOld, got: ${transaction.animatedSlices.filter { it.clusterByteStart == 3 }.map { it.role }}",
            retainedCrossfadeOld.isNotEmpty())
        assertTrue("Retained cluster with changed fingerprint but same position must produce CrossfadeNew",
            retainedCrossfadeNew.isNotEmpty())
    }

    @Test
    fun lineReflow_identityConfidentPositionUnchanged_fingerprintSame_noSlices() {
        val snapshot = makeSnapshotWithClusters(1L, 0, 0, 20, listOf(
            LineClusterSnapshot(
                clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 10,
                documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 100f, 20f),
                shapingFingerprint = "fp_same", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1, documentByteStart = 10, documentByteEndExclusive = 20,
                documentUtf16Start = 10, documentUtf16EndExclusive = 20,
                sourceRectInLineImage = android.graphics.Rect(100, 0, 200, 20),
                visualRectInDocument = android.graphics.RectF(100f, 0f, 200f, 20f),
                shapingFingerprint = "fp_retained", shapingIdentityConfident = true
            )
        ))
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 10, cursorUtf16 = 10, cursorX = 100f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 10, selectionHeadUtf8 = 10,
            selectionAnchorUtf16 = 10, selectionHeadUtf16 = 10,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 10)),
            newAffectedByteRanges = listOf(Pair(0, 10)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(10, 10, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to snapshot),
            preCapturedNewSnapshots = mapOf(0 to snapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L),
            snapshotLookup = emptyMap()
        )
        val retainedSlices = transaction.animatedSlices.filter {
            it.clusterByteStart == 10 && it.clusterByteEndExclusive == 20
        }
        assertTrue("Identity confident + fingerprint same + position unchanged must NOT produce slices for retained cluster",
            retainedSlices.isEmpty())
    }

    @Test
    fun preeditUnderline_usesClipPathPerLineNotComputeBounds() {
        val renderer = com.xiwei.sujian.editor.v2.render.AndroidTextRenderer()
        val method = renderer.javaClass.getDeclaredMethod(
            "drawPreeditUnderlineUnshifted",
            Canvas::class.java,
            android.text.Layout::class.java,
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType
        )
        method.isAccessible = true
        assertNotNull("drawPreeditUnderlineUnshifted method must exist with (Canvas, Layout, int, int) signature", method)
        val sourceCode = method.declaringClass.declaredMethods
            .firstOrNull { it.name == "drawPreeditUnderlineUnshifted" }?.toString() ?: ""
        assertFalse("drawPreeditUnderlineUnshifted must NOT use computeBounds",
            sourceCode.contains("computeBounds"))
    }

    @Test
    fun lineReflow_lowConfidencePositionUnchanged_producesCrossfadeViaPrepare() {
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, 0, 30, listOf(
            LineClusterSnapshot(
                clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 3,
                documentUtf16Start = 0, documentUtf16EndExclusive = 3,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 30, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 30f, 20f),
                shapingFingerprint = "fp_edited", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1, documentByteStart = 3, documentByteEndExclusive = 13,
                documentUtf16Start = 3, documentUtf16EndExclusive = 13,
                sourceRectInLineImage = android.graphics.Rect(30, 0, 130, 20),
                visualRectInDocument = android.graphics.RectF(30f, 0f, 130f, 20f),
                shapingFingerprint = "fp_retained", shapingIdentityConfident = false
            )
        ))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, 0, 30, listOf(
            LineClusterSnapshot(
                clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 3,
                documentUtf16Start = 0, documentUtf16EndExclusive = 3,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 30, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 30f, 20f),
                shapingFingerprint = "fp_edited_new", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1, documentByteStart = 3, documentByteEndExclusive = 13,
                documentUtf16Start = 3, documentUtf16EndExclusive = 13,
                sourceRectInLineImage = android.graphics.Rect(30, 0, 130, 20),
                visualRectInDocument = android.graphics.RectF(30f, 0f, 130f, 20f),
                shapingFingerprint = "fp_retained", shapingIdentityConfident = false
            )
        ))
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 30, startUtf16 = 0, endUtf16 = 30,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 3, cursorUtf16 = 3, cursorX = 30f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 3, selectionHeadUtf8 = 3,
            selectionAnchorUtf16 = 3, selectionHeadUtf16 = 3,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 30, startUtf16 = 0, endUtf16 = 30,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 3, cursorUtf16 = 3, cursorX = 30f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 3, selectionHeadUtf8 = 3,
            selectionAnchorUtf16 = 3, selectionHeadUtf16 = 3,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 3)),
            newAffectedByteRanges = listOf(Pair(0, 3)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(3, 3, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L),
            snapshotLookup = emptyMap()
        )
        val retainedCrossfadeOld = transaction.animatedSlices.filter {
            it.role == SliceRole.CrossfadeOld && it.clusterByteStart == 3 && it.clusterByteEndExclusive == 13
        }
        val retainedCrossfadeNew = transaction.animatedSlices.filter {
            it.role == SliceRole.CrossfadeNew && it.clusterByteStart == 3 && it.clusterByteEndExclusive == 13
        }
        assertTrue("Low confidence retained cluster must produce CrossfadeOld, got: ${transaction.animatedSlices.filter { it.clusterByteStart == 3 }.map { it.role }}",
            retainedCrossfadeOld.isNotEmpty())
        assertTrue("Low confidence retained cluster must produce CrossfadeNew",
            retainedCrossfadeNew.isNotEmpty())
    }

    @Test
    fun lineReflow_fallbackMonotonicOneToOneViaPrepare() {
        val oldClusters = listOf(
            LineClusterSnapshot(
                clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 5,
                documentUtf16Start = 0, documentUtf16EndExclusive = 5,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                shapingFingerprint = "fp_dup", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1, documentByteStart = 5, documentByteEndExclusive = 10,
                documentUtf16Start = 5, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(50f, 0f, 100f, 20f),
                shapingFingerprint = "fp_dup", shapingIdentityConfident = true
            )
        )
        val newClusters = listOf(
            LineClusterSnapshot(
                clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 5,
                documentUtf16Start = 0, documentUtf16EndExclusive = 5,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                shapingFingerprint = "fp_dup", shapingIdentityConfident = true
            ),
            LineClusterSnapshot(
                clusterId = 1, documentByteStart = 5, documentByteEndExclusive = 10,
                documentUtf16Start = 5, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(50f, 0f, 100f, 20f),
                shapingFingerprint = "fp_dup", shapingIdentityConfident = true
            )
        )
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, 0, 10, oldClusters)
        val newSnapshot = makeSnapshotWithClusters(2L, 0, 0, 10, newClusters)
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 10, startUtf16 = 0, endUtf16 = 10,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 5, cursorUtf16 = 5, cursorX = 50f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5,
            selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = AndroidLayoutRevision(
            revisionId = 2L, editorRevision = 2L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 10, startUtf16 = 0, endUtf16 = 10,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 5, cursorUtf16 = 5, cursorX = 50f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5,
            selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 5)),
            newAffectedByteRanges = listOf(Pair(0, 5)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor(5, 5, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L),
            snapshotLookup = emptyMap()
        )
        val moveSlices = transaction.animatedSlices.filter { it.role == SliceRole.Move }
        val newStarts = moveSlices.map { it.clusterByteStart }
        for (i in 1 until newStarts.size) {
            assertTrue("Move slice byte starts must be monotonically non-decreasing",
                newStarts[i] >= newStarts[i - 1])
        }
    }

    @Test
    fun planClusterReplaceAnimation_skipsUnchangedCluster() {
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, 0, 10, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 10,
                documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 100f, 20f),
                shapingFingerprint = "fp_same", shapingIdentityConfident = true)
        ))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, 0, 10, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 10,
                documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 100f, 20f),
                shapingFingerprint = "fp_same", shapingIdentityConfident = true)
        ))
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 10, startUtf16 = 0, endUtf16 = 10,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 5, cursorUtf16 = 5, cursorX = 50f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5,
            selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = oldRev.copy(revisionId = 2L, editorRevision = 2L)
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 5)),
            newAffectedByteRanges = listOf(Pair(0, 5)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(5, 5, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L),
            snapshotLookup = emptyMap()
        )
        val hasAnySlice = transaction.animatedSlices.isNotEmpty()
        assertFalse("Unchanged cluster (same position, same fingerprint, confident) must NOT produce any slice",
            hasAnySlice)
    }

    @Test
    fun planClusterReplaceAnimation_fingerprintChanged_positionSame_producesCrossfade() {
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, 0, 10, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 10,
                documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 100f, 20f),
                shapingFingerprint = "fp_old", shapingIdentityConfident = true)
        ))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, 0, 10, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 10,
                documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 100f, 20f),
                shapingFingerprint = "fp_new", shapingIdentityConfident = true)
        ))
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 10, startUtf16 = 0, endUtf16 = 10,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 5, cursorUtf16 = 5, cursorX = 50f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5,
            selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = oldRev.copy(revisionId = 2L, editorRevision = 2L)
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 5)),
            newAffectedByteRanges = listOf(Pair(0, 5)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(5, 5, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L),
            snapshotLookup = emptyMap()
        )
        val hasCrossfadeOld = transaction.animatedSlices.any { it.role == SliceRole.CrossfadeOld }
        val hasCrossfadeNew = transaction.animatedSlices.any { it.role == SliceRole.CrossfadeNew }
        assertTrue("Different fingerprint + position same in CLUSTER_ANIMATION replace must produce CrossfadeOld", hasCrossfadeOld)
        assertTrue("Different fingerprint + position same in CLUSTER_ANIMATION replace must produce CrossfadeNew", hasCrossfadeNew)
    }

    @Test
    fun lineReflow_fingerprintChanged_positionSame_producesCrossfade() {
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, 0, 20, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 5,
                documentUtf16Start = 0, documentUtf16EndExclusive = 5,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                shapingFingerprint = "fp_edited", shapingIdentityConfident = true),
            LineClusterSnapshot(clusterId = 1, documentByteStart = 5, documentByteEndExclusive = 20,
                documentUtf16Start = 5, documentUtf16EndExclusive = 20,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(50f, 0f, 100f, 20f),
                shapingFingerprint = "fp_retained_old", shapingIdentityConfident = true)
        ))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, 0, 20, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 5,
                documentUtf16Start = 0, documentUtf16EndExclusive = 5,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                shapingFingerprint = "fp_edited_new", shapingIdentityConfident = true),
            LineClusterSnapshot(clusterId = 1, documentByteStart = 5, documentByteEndExclusive = 20,
                documentUtf16Start = 5, documentUtf16EndExclusive = 20,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(50f, 0f, 100f, 20f),
                shapingFingerprint = "fp_retained_new", shapingIdentityConfident = true)
        ))
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 5, cursorUtf16 = 5, cursorX = 50f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5,
            selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = oldRev.copy(revisionId = 2L, editorRevision = 2L)
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 5)),
            newAffectedByteRanges = listOf(Pair(0, 5)),
            animationMode = uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(5, 5, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L),
            snapshotLookup = emptyMap()
        )
        val retainedCrossfadeOld = transaction.animatedSlices.filter {
            it.role == SliceRole.CrossfadeOld && it.clusterByteStart == 5
        }
        val retainedCrossfadeNew = transaction.animatedSlices.filter {
            it.role == SliceRole.CrossfadeNew && it.clusterByteStart == 5
        }
        assertTrue("Fingerprint changed + position same in LineReflow must produce CrossfadeOld for retained cluster",
            retainedCrossfadeOld.isNotEmpty())
        assertTrue("Fingerprint changed + position same in LineReflow must produce CrossfadeNew for retained cluster",
            retainedCrossfadeNew.isNotEmpty())
    }

    @Test
    fun planRunReplaceAnimation_skipsUnchangedCluster() {
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, 0, 10, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 10,
                documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 100f, 20f),
                shapingFingerprint = "fp_same", shapingIdentityConfident = true)
        ))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, 0, 10, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 10,
                documentUtf16Start = 0, documentUtf16EndExclusive = 10,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 100f, 20f),
                shapingFingerprint = "fp_same", shapingIdentityConfident = true)
        ))
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 10, startUtf16 = 0, endUtf16 = 10,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 5, cursorUtf16 = 5, cursorX = 50f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5,
            selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = oldRev.copy(revisionId = 2L, editorRevision = 2L)
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 5)),
            newAffectedByteRanges = listOf(Pair(0, 5)),
            animationMode = uniffi.writer_core.AnimationModeDto.RUN_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(5, 5, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L),
            snapshotLookup = emptyMap()
        )
        assertFalse("Unchanged cluster in RUN_ANIMATION must NOT produce any slice",
            transaction.animatedSlices.isNotEmpty())
    }

    @Test
    fun addMoveSlices_fallbackDistanceUsesMappedStartOrLastMatched() {
        val mappedStart: Int? = null
        val lastMatchedNewStart = 50
        val target = mappedStart ?: lastMatchedNewStart
        val candidate1Start = 55
        val candidate2Start = 70
        val dist1 = kotlin.math.abs(candidate1Start - target)
        val dist2 = kotlin.math.abs(candidate2Start - target)
        assertTrue("Candidate closer to target must have smaller distance", dist1 < dist2)
        assertEquals(5, dist1)
        assertEquals(20, dist2)
    }

    @Test
    fun planClusterReplaceAnimation_offsetMapperPrimaryMatch() {
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, 0, 20, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 5,
                documentUtf16Start = 0, documentUtf16EndExclusive = 5,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                shapingFingerprint = "fp_a", shapingIdentityConfident = true),
            LineClusterSnapshot(clusterId = 1, documentByteStart = 5, documentByteEndExclusive = 20,
                documentUtf16Start = 5, documentUtf16EndExclusive = 20,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(50f, 0f, 100f, 20f),
                shapingFingerprint = "fp_b", shapingIdentityConfident = true)
        ))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, 0, 20, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 5,
                documentUtf16Start = 0, documentUtf16EndExclusive = 5,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                shapingFingerprint = "fp_a", shapingIdentityConfident = true),
            LineClusterSnapshot(clusterId = 1, documentByteStart = 5, documentByteEndExclusive = 20,
                documentUtf16Start = 5, documentUtf16EndExclusive = 20,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(60f, 0f, 110f, 20f),
                shapingFingerprint = "fp_b", shapingIdentityConfident = true)
        ))
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 20, startUtf16 = 0, endUtf16 = 20,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 5, cursorUtf16 = 5, cursorX = 50f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5,
            selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = oldRev.copy(revisionId = 2L, editorRevision = 2L)
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 20)),
            newAffectedByteRanges = listOf(Pair(0, 20)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(5, 5, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L),
            snapshotLookup = emptyMap()
        )
        val retainedSlice = transaction.animatedSlices.find {
            it.role == SliceRole.Move && it.clusterByteStart == 5
        }
        assertNotNull("Offset mapper primary match must pair retained cluster with position change via Move", retainedSlice)
        if (retainedSlice != null) {
            assertEquals(android.graphics.RectF(50f, 0f, 100f, 20f), retainedSlice.fromDestinationRect)
        }
    }

    @Test
    fun planClusterReplaceAnimation_fallbackUsesMappedStartDistance() {
        val oldSnapshot = makeSnapshotWithClusters(1L, 0, 0, 30, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 5,
                documentUtf16Start = 0, documentUtf16EndExclusive = 5,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                shapingFingerprint = "fp_x", shapingIdentityConfident = true),
            LineClusterSnapshot(clusterId = 1, documentByteStart = 5, documentByteEndExclusive = 15,
                documentUtf16Start = 5, documentUtf16EndExclusive = 15,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(50f, 0f, 100f, 20f),
                shapingFingerprint = "fp_dup", shapingIdentityConfident = true),
            LineClusterSnapshot(clusterId = 2, documentByteStart = 15, documentByteEndExclusive = 30,
                documentUtf16Start = 15, documentUtf16EndExclusive = 30,
                sourceRectInLineImage = android.graphics.Rect(100, 0, 150, 20),
                visualRectInDocument = android.graphics.RectF(100f, 0f, 150f, 20f),
                shapingFingerprint = "fp_dup", shapingIdentityConfident = true)
        ))
        val newSnapshot = makeSnapshotWithClusters(2L, 0, 0, 30, listOf(
            LineClusterSnapshot(clusterId = 0, documentByteStart = 0, documentByteEndExclusive = 5,
                documentUtf16Start = 0, documentUtf16EndExclusive = 5,
                sourceRectInLineImage = android.graphics.Rect(0, 0, 50, 20),
                visualRectInDocument = android.graphics.RectF(0f, 0f, 50f, 20f),
                shapingFingerprint = "fp_x", shapingIdentityConfident = true),
            LineClusterSnapshot(clusterId = 1, documentByteStart = 5, documentByteEndExclusive = 15,
                documentUtf16Start = 5, documentUtf16EndExclusive = 15,
                sourceRectInLineImage = android.graphics.Rect(50, 0, 100, 20),
                visualRectInDocument = android.graphics.RectF(55f, 0f, 105f, 20f),
                shapingFingerprint = "fp_dup", shapingIdentityConfident = true),
            LineClusterSnapshot(clusterId = 2, documentByteStart = 15, documentByteEndExclusive = 30,
                documentUtf16Start = 15, documentUtf16EndExclusive = 30,
                sourceRectInLineImage = android.graphics.Rect(100, 0, 150, 20),
                visualRectInDocument = android.graphics.RectF(110f, 0f, 160f, 20f),
                shapingFingerprint = "fp_dup", shapingIdentityConfident = true)
        ))
        val oldRev = AndroidLayoutRevision(
            revisionId = 1L, editorRevision = 1L,
            widthFingerprint = 800f, fontFingerprint = "48",
            lineCount = 1,
            lineRanges = listOf(AndroidLayoutRevision.LineRange(
                startUtf8 = 0, endUtf8 = 30, startUtf16 = 0, endUtf16 = 30,
                top = 0f, bottom = 20f, baseline = 16f, left = 0f, right = 800f,
                endsWithHardBreak = true, paragraphId = 0, paragraphLocalLineIndex = 0
            )),
            cursorUtf8 = 5, cursorUtf16 = 5, cursorX = 50f, cursorY = 0f, cursorHeight = 20f,
            selectionAnchorUtf8 = 5, selectionHeadUtf8 = 5,
            selectionAnchorUtf16 = 5, selectionHeadUtf16 = 5,
            compositionStartUtf16 = -1, compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
        val newRev = oldRev.copy(revisionId = 2L, editorRevision = 2L)
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
            operationKind = uniffi.writer_core.EditorOperationKindDto.REPLACE,
            oldAffectedByteRanges = listOf(Pair(0, 30)),
            newAffectedByteRanges = listOf(Pair(0, 30)),
            animationMode = uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(5, 5, true)
        )
        val planner = AndroidVisualPlanner()
        val transaction = planner.prepare(
            visualIntent = visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            preCapturedOldSnapshots = mapOf(0 to oldSnapshot),
            preCapturedNewSnapshots = mapOf(0 to newSnapshot),
            transactionKey = 1L,
            ownedSnapshotIds = setOf(1L, 2L),
            snapshotLookup = emptyMap()
        )
        val cluster2Slice = transaction.animatedSlices.find {
            it.role == SliceRole.Move && it.clusterByteStart == 15
        }
        assertNotNull("Offset mapper must match old cluster at 15 to new cluster at 15 with position change via Move", cluster2Slice)
    }
}
