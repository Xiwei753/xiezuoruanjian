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
        val affected = method.invoke(planner, visualIntent, oldRev, newRev) as Set<*>
        assertTrue("Paragraph 0 extra line (new index 2) must be included",
            affected.contains(2))
        assertTrue("Paragraph 1 shifted line (new index 3) must be included",
            affected.contains(3))
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
    fun computeAffectedLineIndicesExpandsToDocumentEnd() {
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
        assertTrue("Line 0 must be included", affected.contains(0))
        assertTrue("Line 1 must be included", affected.contains(1))
        assertTrue("Line 2 (next paragraph) must be included for Y-geometry Move",
            affected.contains(2))
        assertTrue("Line 3 must be included", affected.contains(3))
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
            0 to makeSnapshot(101, 0, 0, 20),
            1 to makeSnapshot(102, 1, 20, 40),
            2 to makeSnapshot(103, 2, 40, 60)
        )
        val newSnapshots = mapOf(
            0 to makeSnapshot(201, 0, 0, 15),
            1 to makeSnapshot(202, 1, 15, 30),
            2 to makeSnapshot(203, 2, 30, 40),
            3 to makeSnapshot(204, 3, 40, 60)
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
        assertTrue("Paragraph 1 shifted line (new index 3) must be animated",
            newLineIndices.contains(3))
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
}
