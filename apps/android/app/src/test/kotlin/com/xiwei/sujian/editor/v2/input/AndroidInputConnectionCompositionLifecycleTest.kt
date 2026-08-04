package com.xiwei.sujian.editor.v2.input

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * Contract tests for composition lifecycle operations driven through
 * [AndroidInputConnection] (Issue #589): rapid consecutive commits, preedit replacement,
 * finishComposingText materialization and the cancel path (text restoration).
 *
 * All assertions run against the production [AndroidInputAdapter] / [AndroidInputConnection]
 * stack with a deterministic in-memory [FakeInputCommandPort] standing in for the Rust
 * kernel — no system IME is involved.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], shadows = [RecordingInputMethodManagerShadow::class])
class AndroidInputConnectionCompositionLifecycleTest {

    @Test
    fun rapidConsecutiveCommits_noLossNoDuplicates() {
        val h = InputConnectionTestHarness("", 0)

        h.connection.commitText("你", 1)
        h.connection.commitText("好", 1)
        h.connection.commitText("!", 1)

        assertEquals("No character may be lost or duplicated", "你好!", h.mirror.getCommittedText())
        assertEquals("Kernel text must match the mirror", "你好!", h.commandPort.getKernelText())
        assertEquals("Each commit must reach the kernel exactly once", 3, h.commandPort.commitCalls.size)
        assertFalse("Plain commits must not leave composing state", h.adapter.isComposing())
        assertEquals(
            "Every plain commit must use the TYPING cause",
            listOf(EditorTransactionCauseDto.TYPING, EditorTransactionCauseDto.TYPING, EditorTransactionCauseDto.TYPING),
            h.commandPort.commitCalls.map { it.cause }
        )
        assertEquals(
            "Plain multi-byte inserts must be INSERT operations, never composition commits",
            listOf(EditorOperationKindDto.INSERT, EditorOperationKindDto.INSERT, EditorOperationKindDto.INSERT),
            h.commandPort.commitCalls.map { it.operationKind }
        )
        assertEquals("The IME must be notified once per commit", 3, h.imm.updateSelectionCount)
    }

    @Test
    fun plainCommitText_reportsInsertOperationKind() {
        val h = InputConnectionTestHarness("AB", 2)

        h.connection.commitText("C", 1)

        assertEquals("ABC", h.mirror.getCommittedText())
        assertEquals(
            "Plain commit must use the TYPING cause",
            EditorTransactionCauseDto.TYPING, h.commandPort.commitCalls.single().cause
        )
        assertEquals(
            "Plain commit without composition must report INSERT",
            EditorOperationKindDto.INSERT, h.commandPort.commitCalls.single().operationKind
        )
    }

    @Test
    fun rapidAlternatingCompositionAndCommit_noTextLoss() {
        val h = InputConnectionTestHarness("", 0)

        h.connection.setComposingText("你", 1)
        h.connection.commitText("你", 1)
        h.connection.setComposingText("好", 1)
        h.connection.commitText("好", 1)

        assertEquals("你好", h.mirror.getCommittedText())
        assertEquals("你好", h.commandPort.getKernelText())
        assertFalse(h.adapter.isComposing())
        assertEquals(
            "Committing a composition must report COMPOSITION_COMMIT",
            EditorOperationKindDto.COMPOSITION_COMMIT, h.commandPort.commitCalls.last().operationKind
        )
        assertEquals(
            "Committing a composition must use the TYPING_COMMIT cause",
            EditorTransactionCauseDto.TYPING_COMMIT, h.commandPort.commitCalls.last().cause
        )
    }

    @Test
    fun setComposingTextWhileComposing_replacesOldPreedit() {
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11)
        h.connection.setComposingRegion(6, 11)
        assertEquals("world", h.adapter.getCompositionText())

        h.connection.setComposingText("wrld", 1)

        assertTrue(h.adapter.isComposing())
        assertEquals("Old preedit must be replaced", "wrld", h.adapter.getCompositionText())
        assertEquals("Overlay must show the new preedit", "hello wrld", h.mirror.getText())
        assertEquals("Committed text must stay unchanged", "hello world", h.mirror.getCommittedText())
        assertEquals("The kernel session must survive preedit replacement", 1, h.commandPort.beginCompositionCount)
        assertEquals("One preedit update per setComposingText", 1, h.commandPort.updateCompositionCount)
    }

    @Test
    fun commitWhileComposing_commitsFinalText() {
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11)
        h.connection.setComposingRegion(6, 11)
        h.connection.setComposingText("wrld", 1)

        h.connection.commitText("world", 1)

        assertFalse("Composition must be finished by the commit", h.adapter.isComposing())
        assertEquals("Final text must be committed", "hello world", h.mirror.getCommittedText())
        assertEquals("Kernel text must match the mirror", "hello world", h.commandPort.getKernelText())
        assertEquals(
            EditorOperationKindDto.COMPOSITION_COMMIT, h.commandPort.commitCalls.last().operationKind
        )
        assertEquals(
            EditorTransactionCauseDto.TYPING_COMMIT, h.commandPort.commitCalls.last().cause
        )
    }

    @Test
    fun finishComposingText_materializesPreedit() {
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11)
        h.connection.setComposingRegion(6, 11)
        h.connection.setComposingText("wrld", 1)
        assertEquals("hello wrld", h.mirror.getText())

        h.connection.finishComposingText()

        assertFalse("finishComposingText must end composing mode", h.adapter.isComposing())
        assertEquals("Preedit must be materialized into the text", "hello wrld", h.mirror.getCommittedText())
        assertEquals("Kernel text must match the mirror", "hello wrld", h.commandPort.getKernelText())
        assertFalse("No overlay may remain", h.mirror.hasComposition())
        // Cursor lands at the end of the materialized preedit: UTF-8 offset 6 + 4 = 10.
        assertEquals(10, h.mirror.getCursorUtf8())
        assertEquals(1, h.commandPort.finishCompositionCount)
        assertEquals("finish must not go through commitText", 0, h.commandPort.commitCalls.size)
    }

    @Test
    fun finishComposingText_withoutComposition_isNoOp() {
        val h = InputConnectionTestHarness("hello", 5)

        h.connection.finishComposingText()

        assertFalse(h.adapter.isComposing())
        assertEquals("hello", h.mirror.getCommittedText())
    }

    @Test
    fun newInputConnection_clearsOrphanCompositionState() {
        // Composition state belongs to one InputConnection instance. When the system
        // recreates the connection (IME switch / restartInput / focus change), the
        // previous connection's composition must be cancelled: kernel session closed,
        // overlay removed, committed text restored — otherwise the new connection's
        // first plain commit would be misrouted into the orphaned composition range.
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11)
        h.connection.setComposingRegion(6, 11)
        h.connection.setComposingText("wrld", 1)
        assertTrue(h.adapter.isComposing())
        assertTrue(h.commandPort.hasActiveSession())

        val newConnection = h.adapter.onCreateInputConnection(android.view.inputmethod.EditorInfo())

        assertNotNull("A new connection must be produced", newConnection)
        assertFalse("Orphan composition must be cancelled on connection recreation", h.adapter.isComposing())
        assertEquals("Kernel session must be cancelled", 1, h.commandPort.cancelCompositionCount)
        assertFalse("No kernel session may survive", h.commandPort.hasActiveSession())
        assertFalse("Overlay must be cleared", h.mirror.hasComposition())
        assertEquals("Committed text must be restored", "hello world", h.mirror.getCommittedText())
        assertFalse("A second recreation must be a no-op (no double cancel)", h.adapter.isComposing())
        h.adapter.onCreateInputConnection(android.view.inputmethod.EditorInfo())
        assertEquals("Idle recreation must not cancel again", 1, h.commandPort.cancelCompositionCount)
    }

    @Test
    fun finishComposingText_withRegionOnly_leavesCommittedTextUnchanged() {
        // Recorrection path: setComposingRegion alone (no setComposingText) followed by
        // finish must land without text loss — the committed text already is the preedit.
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setComposingRegion(6, 11)
        assertTrue(h.adapter.isComposing())

        h.connection.finishComposingText()

        assertFalse(h.adapter.isComposing())
        assertEquals("hello world", h.mirror.getCommittedText())
        assertEquals(1, h.commandPort.finishCompositionCount)
        assertEquals("finish must not go through commitText", 0, h.commandPort.commitCalls.size)
    }

    @Test
    fun rapidComposingAndPlainCommits_keepKindsAndCausesInOrder() {
        // A burst mixing plain commits with a composing region (begin → preedit → commit)
        // must route every call with the correct cause/operationKind and no text loss.
        val h = InputConnectionTestHarness("", 0)
        h.connection.commitText("hello ", 1)
        h.connection.setComposingRegion(6, 6)
        h.connection.setComposingText("world", 1)
        h.connection.commitText("WORLD", 1)
        h.connection.commitText("!", 1)

        assertEquals("hello WORLD!", h.mirror.getCommittedText())
        assertEquals("hello WORLD!", h.commandPort.getKernelText())
        assertEquals(
            listOf(
                EditorTransactionCauseDto.TYPING,
                EditorTransactionCauseDto.TYPING_COMMIT,
                EditorTransactionCauseDto.TYPING
            ),
            h.commandPort.commitCalls.map { it.cause }
        )
        assertEquals(
            listOf(
                EditorOperationKindDto.INSERT,
                EditorOperationKindDto.COMPOSITION_COMMIT,
                EditorOperationKindDto.INSERT
            ),
            h.commandPort.commitCalls.map { it.operationKind }
        )
    }

    @Test
    fun cancelPath_restoresOriginalText() {
        // Deleting the entire preedit (deleteSurroundingText) triggers the cancel path:
        // the kernel session is cancelled and the overlay is removed, restoring the
        // committed text that existed before composition.
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11)
        h.connection.setComposingRegion(6, 11)
        h.connection.setComposingText("wrld", 1)
        assertEquals("hello wrld", h.mirror.getText())

        h.connection.deleteSurroundingText(4, 0)

        assertFalse("Cancel must end composing mode", h.adapter.isComposing())
        assertEquals("Original committed text must be restored", "hello world", h.mirror.getCommittedText())
        assertEquals("Overlay must be gone", "hello world", h.mirror.getText())
        assertEquals("Kernel text must match", "hello world", h.commandPort.getKernelText())

        // Cancel does not disturb the selection (same as the kernel): it still spans the
        // cancelled region. A real IME moves the cursor before committing again.
        assertEquals(Pair(6, 11), Pair(h.mirror.getSelectionStartUtf8(), h.mirror.getSelectionEndUtf8()))
        h.connection.setSelection(11, 11)

        // The cancelled session must not poison subsequent edits: the commit must be a
        // plain insert, not a stale composition commit.
        h.connection.commitText("!", 1)
        assertEquals("hello world!", h.mirror.getCommittedText())
        assertEquals("No kernel reload may be needed", 0, h.commandPort.reloadCount)
    }

    @Test
    fun commitAfterCancel_usesPlainInsertKind() {
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11)
        h.connection.setComposingRegion(6, 11)
        h.connection.setComposingText("wrld", 1)
        h.connection.deleteSurroundingText(4, 0)

        // Real IME sequence after a cancel: move the cursor, then commit.
        h.connection.setSelection(11, 11)
        h.connection.commitText("!", 1)

        assertEquals(
            "Commit after cancel must be a plain insert",
            EditorOperationKindDto.INSERT, h.commandPort.commitCalls.last().operationKind
        )
        assertEquals(
            EditorTransactionCauseDto.TYPING, h.commandPort.commitCalls.last().cause
        )
        assertEquals("hello world!", h.mirror.getCommittedText())
        assertEquals("No kernel reload may be needed", 0, h.commandPort.reloadCount)
    }

    @Test
    fun commitText_deletesReplacedSelection() {
        // commitText over a selection must replace the selected range, not append.
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11)

        h.connection.commitText("there", 1)

        assertEquals("hello there", h.mirror.getCommittedText())
        assertEquals("hello there", h.commandPort.getKernelText())
        assertEquals(
            "Selection replacement without composition must report REPLACE",
            EditorOperationKindDto.REPLACE, h.commandPort.commitCalls.last().operationKind
        )
    }
}
