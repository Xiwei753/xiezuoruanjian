package com.xiwei.sujian.editor.v2

import com.xiwei.sujian.editor.v2.mirror.DisplayPatch
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor
import org.junit.Assert.*
import org.junit.Test

class EditResultTest {

    @Test
    fun displayPatchFromDtoRoundTrip() {
        val patch = DisplayPatch(
            baseRevision = 0,
            newRevision = 1,
            replaceByteStart = 2,
            replaceByteEndExclusive = 2,
            insertedText = "c",
            resultingSelectionStart = 3,
            resultingSelectionEnd = 3
        )
        assertEquals(0, patch.baseRevision)
        assertEquals(1, patch.newRevision)
        assertEquals(2, patch.replaceByteStart)
        assertEquals(2, patch.replaceByteEndExclusive)
        assertEquals("c", patch.insertedText)
    }

    @Test
    fun visualIntentHoldsCorrectFields() {
        val intent = VisualIntent(
            cause = "typing",
            operationKind = "insert",
            oldAffectedByteRanges = listOf(Pair(2, 2)),
            newAffectedByteRanges = listOf(Pair(2, 3)),
            animationMode = "ClusterAnimation",
            durationMs = 160,
            coordinatedCursor = CoordinatedCursor(2, 3, true)
        )
        assertEquals("typing", intent.cause)
        assertEquals("insert", intent.operationKind)
        assertEquals(1, intent.oldAffectedByteRanges.size)
        assertEquals(1, intent.newAffectedByteRanges.size)
        assertEquals(160, intent.durationMs)
        assertTrue(intent.coordinatedCursor.shouldAnimate)
    }

    @Test
    fun editResultHoldsCorrectFields() {
        val result = EditResult(
            transactionId = 1,
            baseRevision = 0,
            newRevision = 1,
            displayPatches = listOf(
                DisplayPatch(0, 1, 2, 2, "c", 3, 3)
            ),
            oldSelectionStart = 2,
            oldSelectionEnd = 2,
            newSelectionStart = 3,
            newSelectionEnd = 3,
            visualIntent = VisualIntent(
                cause = "typing",
                operationKind = "insert",
                oldAffectedByteRanges = listOf(Pair(2, 2)),
                newAffectedByteRanges = listOf(Pair(2, 3)),
                animationMode = "ClusterAnimation",
                durationMs = 160,
                coordinatedCursor = CoordinatedCursor(2, 3, true)
            )
        )
        assertEquals(1, result.transactionId)
        assertEquals(1, result.displayPatches.size)
        assertEquals(3, result.newSelectionEnd)
    }

    @Test
    fun coordinatedCursorDefaults() {
        val cursor = CoordinatedCursor(0, 5, false)
        assertEquals(0, cursor.oldByteOffset)
        assertEquals(5, cursor.newByteOffset)
        assertFalse(cursor.shouldAnimate)
    }
}
