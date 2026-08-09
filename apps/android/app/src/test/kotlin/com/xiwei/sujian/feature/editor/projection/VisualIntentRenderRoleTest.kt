package com.xiwei.sujian.feature.editor.projection

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto

/**
 * #606: Verifies that [VisualIntent] render-role helpers consume Core's [operationKind]
 * correctly — the planner dispatches based on Core's classification, not a local
 * re-classification from byte ranges.
 */
class VisualIntentRenderRoleTest {
    private fun intent(kind: EditorOperationKindDto): VisualIntent =
        VisualIntent.fromDto(
            EditorVisualIntentDto(
                cause = EditorTransactionCauseDto.TYPING,
                operationKind = kind,
                oldAffectedByteRanges = emptyList(),
                newAffectedByteRanges = emptyList(),
                animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0uL,
                coordinatedCursor =
                    CoordinatedCursorDto(
                        oldByteOffset = 0u,
                        newByteOffset = 0u,
                        shouldAnimate = false,
                    ),
                offsetMap = null,
            ),
        )

    @Test
    fun insertRenderRole_true_forInsert() {
        assertTrue(intent(EditorOperationKindDto.INSERT).isInsertRenderRole())
    }

    @Test
    fun insertRenderRole_false_forDelete() {
        assertFalse(intent(EditorOperationKindDto.DELETE).isInsertRenderRole())
    }

    @Test
    fun deleteRenderRole_true_forDelete() {
        assertTrue(intent(EditorOperationKindDto.DELETE).isDeleteRenderRole())
    }

    @Test
    fun deleteRenderRole_true_forCompositionCancel() {
        assertTrue(intent(EditorOperationKindDto.COMPOSITION_CANCEL).isDeleteRenderRole())
    }

    @Test
    fun deleteRenderRole_false_forInsert() {
        assertFalse(intent(EditorOperationKindDto.INSERT).isDeleteRenderRole())
    }

    @Test
    fun replaceRenderRole_true_forReplace() {
        assertTrue(intent(EditorOperationKindDto.REPLACE).isReplaceRenderRole())
    }

    @Test
    fun replaceRenderRole_true_forCompositionCommit() {
        assertTrue(intent(EditorOperationKindDto.COMPOSITION_COMMIT).isReplaceRenderRole())
    }

    @Test
    fun replaceRenderRole_true_forCompositionUpdate() {
        assertTrue(intent(EditorOperationKindDto.COMPOSITION_UPDATE).isReplaceRenderRole())
    }

    @Test
    fun replaceRenderRole_false_forInsert() {
        assertFalse(intent(EditorOperationKindDto.INSERT).isReplaceRenderRole())
    }

    @Test
    fun replaceRenderRole_false_forCompositionCancel() {
        assertFalse(intent(EditorOperationKindDto.COMPOSITION_CANCEL).isReplaceRenderRole())
    }

    @Test
    fun deleteOrReplaceRenderRole_true_forDelete() {
        assertTrue(intent(EditorOperationKindDto.DELETE).isDeleteOrReplaceRenderRole())
    }

    @Test
    fun deleteOrReplaceRenderRole_true_forReplace() {
        assertTrue(intent(EditorOperationKindDto.REPLACE).isDeleteOrReplaceRenderRole())
    }

    @Test
    fun deleteOrReplaceRenderRole_true_forCompositionCancel() {
        assertTrue(intent(EditorOperationKindDto.COMPOSITION_CANCEL).isDeleteOrReplaceRenderRole())
    }

    @Test
    fun deleteOrReplaceRenderRole_true_forCompositionCommit() {
        assertTrue(intent(EditorOperationKindDto.COMPOSITION_COMMIT).isDeleteOrReplaceRenderRole())
    }

    @Test
    fun deleteOrReplaceRenderRole_true_forCompositionUpdate() {
        assertTrue(intent(EditorOperationKindDto.COMPOSITION_UPDATE).isDeleteOrReplaceRenderRole())
    }

    @Test
    fun deleteOrReplaceRenderRole_false_forInsert() {
        assertFalse(intent(EditorOperationKindDto.INSERT).isDeleteOrReplaceRenderRole())
    }

    @Test
    fun deleteOrReplaceRenderRole_false_forCursorOnly() {
        assertFalse(intent(EditorOperationKindDto.CURSOR_ONLY).isDeleteOrReplaceRenderRole())
    }
}
