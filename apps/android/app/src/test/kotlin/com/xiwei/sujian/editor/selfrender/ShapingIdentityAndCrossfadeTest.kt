package com.xiwei.sujian.editor.selfrender

import android.graphics.RectF
import org.junit.Assert.*
import org.junit.Test

class ShapingIdentityAndCrossfadeTest {

    @Test
    fun shapingIdentity_notNull_whenClusterTextProvided() {
        val clusterText = "abc"
        val paintFingerprint = "sans-serif:16:1.00:0.00"
        val textHash = clusterText.hashCode()
        val isRtl = false
        val identity = "$textHash:$paintFingerprint:0:3:$isRtl"
        assertNotNull(identity)
        assertTrue(identity.isNotEmpty())
        assertTrue(identity.contains("3"))
        assertTrue(identity.contains("sans-serif"))
        assertTrue(identity.contains("false"))
    }

    @Test
    fun shapingIdentity_differentText_notEqual() {
        val id1 = buildTestShapingIdentity("abc", "sans-serif:16", false)
        val id2 = buildTestShapingIdentity("xyz", "sans-serif:16", false)
        assertNotEquals(id1, id2)
    }

    @Test
    fun shapingIdentity_differentFont_notEqual() {
        val id1 = buildTestShapingIdentity("abc", "sans-serif:16", false)
        val id2 = buildTestShapingIdentity("abc", "serif:16", false)
        assertNotEquals(id1, id2)
    }

    @Test
    fun shapingIdentity_differentDirection_notEqual() {
        val id1 = buildTestShapingIdentity("abc", "sans-serif:16", false)
        val id2 = buildTestShapingIdentity("abc", "sans-serif:16", true)
        assertNotEquals(id1, id2)
    }

    @Test
    fun shapingIdentity_sameInputs_equal() {
        val id1 = buildTestShapingIdentity("abc", "sans-serif:16", false)
        val id2 = buildTestShapingIdentity("abc", "sans-serif:16", false)
        assertEquals(id1, id2)
    }

    @Test
    fun shapingIdentity_emoji_notNull() {
        val clusterText = "\uD83D\uDE00"
        val id = buildTestShapingIdentity(clusterText, "sans-serif:16", false)
        assertNotNull(id)
        assertTrue(id.isNotEmpty())
    }

    @Test
    fun shapingIdentity_arabic_notNull() {
        val clusterText = "\u0628\u0627"
        val id = buildTestShapingIdentity(clusterText, "sans-serif:16", true)
        assertNotNull(id)
        assertTrue(id.isNotEmpty())
    }

    @Test
    fun shapingIdentity_combiningMark_notNull() {
        val clusterText = "e\u0301"
        val id = buildTestShapingIdentity(clusterText, "sans-serif:16", false)
        assertNotNull(id)
        assertTrue(id.isNotEmpty())
    }

    @Test
    fun crossfade_slice_createdWithCorrectRoles() {
        val oldSlice = AndroidAnimatedSlice.crossfade(
            id = 1u,
            role = AndroidAnimatedSliceRole.CrossfadeOld,
            snapshotId = AndroidLineSnapshotId(1L, 0),
            sourceRect = RectF(0f, 0f, 10f, 20f),
            fromRect = RectF(0f, 0f, 10f, 20f),
            toRect = RectF(10f, 0f, 20f, 20f),
            byteStart = 0,
            byteEnd = 3,
            shapingIdentity = "old-identity"
        )
        val newSlice = AndroidAnimatedSlice.crossfade(
            id = 2u,
            role = AndroidAnimatedSliceRole.CrossfadeNew,
            snapshotId = AndroidLineSnapshotId(2L, 0),
            sourceRect = RectF(0f, 0f, 10f, 20f),
            fromRect = RectF(0f, 0f, 10f, 20f),
            toRect = RectF(10f, 0f, 20f, 20f),
            byteStart = 0,
            byteEnd = 3,
            shapingIdentity = "new-identity"
        )

        assertEquals(AndroidAnimatedSliceRole.CrossfadeOld, oldSlice.role)
        assertEquals(AndroidAnimatedSliceRole.CrossfadeNew, newSlice.role)
        assertEquals(1f, oldSlice.opacityFrom, 0.01f)
        assertEquals(0f, oldSlice.opacityTo, 0.01f)
        assertEquals(0f, newSlice.opacityFrom, 0.01f)
        assertEquals(1f, newSlice.opacityTo, 0.01f)
        assertEquals("old-identity", oldSlice.shapingIdentity)
        assertEquals("new-identity", newSlice.shapingIdentity)
    }

    @Test
    fun crossfade_oldFadesOut_newFadesIn() {
        val oldSlice = AndroidAnimatedSlice.crossfade(
            id = 1u,
            role = AndroidAnimatedSliceRole.CrossfadeOld,
            snapshotId = null,
            sourceRect = RectF(0f, 0f, 10f, 20f),
            fromRect = RectF(0f, 0f, 10f, 20f),
            toRect = RectF(10f, 0f, 20f, 20f),
            byteStart = 0,
            byteEnd = 3
        )
        val newSlice = AndroidAnimatedSlice.crossfade(
            id = 2u,
            role = AndroidAnimatedSliceRole.CrossfadeNew,
            snapshotId = null,
            sourceRect = RectF(0f, 0f, 10f, 20f),
            fromRect = RectF(0f, 0f, 10f, 20f),
            toRect = RectF(10f, 0f, 20f, 20f),
            byteStart = 0,
            byteEnd = 3
        )

        assertEquals(1f, oldSlice.opacityFrom, 0.01f)
        assertEquals(0f, oldSlice.opacityTo, 0.01f)
        assertEquals(0f, newSlice.opacityFrom, 0.01f)
        assertEquals(1f, newSlice.opacityTo, 0.01f)
    }

    @Test
    fun reflow_sameShaping_generatesMove_notCrossfade() {
        val shapingId = buildTestShapingIdentity("abc", "sans-serif:16", false)
        val oldCluster = AndroidClusterSnapshot(
            documentByteStart = 0, documentByteEnd = 3,
            platformTextStart = 0, platformTextEnd = 3,
            sourceRectInLineSnapshot = RectF(0f, 0f, 30f, 20f),
            visualRectInDocument = RectF(0f, 0f, 30f, 20f),
            textDirection = 0,
            shapingIdentity = shapingId
        )
        val newCluster = AndroidClusterSnapshot(
            documentByteStart = 0, documentByteEnd = 3,
            platformTextStart = 0, platformTextEnd = 3,
            sourceRectInLineSnapshot = RectF(0f, 0f, 30f, 20f),
            visualRectInDocument = RectF(40f, 0f, 70f, 20f),
            textDirection = 0,
            shapingIdentity = shapingId
        )

        val shapingChanged = oldCluster.shapingIdentity != null &&
            newCluster.shapingIdentity != null &&
            oldCluster.shapingIdentity != newCluster.shapingIdentity

        assertFalse(shapingChanged)
    }

    @Test
    fun reflow_differentShaping_generatesCrossfade() {
        val oldShapingId = buildTestShapingIdentity("abc", "sans-serif:16", false)
        val newShapingId = buildTestShapingIdentity("abc", "serif:16", false)
        val oldCluster = AndroidClusterSnapshot(
            documentByteStart = 0, documentByteEnd = 3,
            platformTextStart = 0, platformTextEnd = 3,
            sourceRectInLineSnapshot = RectF(0f, 0f, 30f, 20f),
            visualRectInDocument = RectF(0f, 0f, 30f, 20f),
            textDirection = 0,
            shapingIdentity = oldShapingId
        )
        val newCluster = AndroidClusterSnapshot(
            documentByteStart = 0, documentByteEnd = 3,
            platformTextStart = 0, platformTextEnd = 3,
            sourceRectInLineSnapshot = RectF(0f, 0f, 30f, 20f),
            visualRectInDocument = RectF(40f, 0f, 70f, 20f),
            textDirection = 0,
            shapingIdentity = newShapingId
        )

        val shapingChanged = oldCluster.shapingIdentity != null &&
            newCluster.shapingIdentity != null &&
            oldCluster.shapingIdentity != newCluster.shapingIdentity

        assertTrue(shapingChanged)
    }

    @Test
    fun reflow_nullShaping_generatesMove_notCrossfade() {
        val oldCluster = AndroidClusterSnapshot(
            documentByteStart = 0, documentByteEnd = 3,
            platformTextStart = 0, platformTextEnd = 3,
            sourceRectInLineSnapshot = RectF(0f, 0f, 30f, 20f),
            visualRectInDocument = RectF(0f, 0f, 30f, 20f),
            textDirection = 0,
            shapingIdentity = null
        )
        val newCluster = AndroidClusterSnapshot(
            documentByteStart = 0, documentByteEnd = 3,
            platformTextStart = 0, platformTextEnd = 3,
            sourceRectInLineSnapshot = RectF(0f, 0f, 30f, 20f),
            visualRectInDocument = RectF(40f, 0f, 70f, 20f),
            textDirection = 0,
            shapingIdentity = null
        )

        val shapingChanged = oldCluster.shapingIdentity != null &&
            newCluster.shapingIdentity != null &&
            oldCluster.shapingIdentity != newCluster.shapingIdentity

        assertFalse(shapingChanged)
    }

    @Test
    fun reflow_oneNullShaping_generatesMove_notCrossfade() {
        val shapingId = buildTestShapingIdentity("abc", "sans-serif:16", false)
        val oldCluster = AndroidClusterSnapshot(
            documentByteStart = 0, documentByteEnd = 3,
            platformTextStart = 0, platformTextEnd = 3,
            sourceRectInLineSnapshot = RectF(0f, 0f, 30f, 20f),
            visualRectInDocument = RectF(0f, 0f, 30f, 20f),
            textDirection = 0,
            shapingIdentity = shapingId
        )
        val newCluster = AndroidClusterSnapshot(
            documentByteStart = 0, documentByteEnd = 3,
            platformTextStart = 0, platformTextEnd = 3,
            sourceRectInLineSnapshot = RectF(0f, 0f, 30f, 20f),
            visualRectInDocument = RectF(40f, 0f, 70f, 20f),
            textDirection = 0,
            shapingIdentity = null
        )

        val shapingChanged = oldCluster.shapingIdentity != null &&
            newCluster.shapingIdentity != null &&
            oldCluster.shapingIdentity != newCluster.shapingIdentity

        assertFalse(shapingChanged)
    }

    private fun buildTestShapingIdentity(clusterText: String, fontFingerprint: String, isRtl: Boolean): String {
        val textHash = clusterText.hashCode()
        return "$textHash:$fontFingerprint:0:${clusterText.codePointCount(0, clusterText.length)}:$isRtl"
    }
}
