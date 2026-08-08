package com.xiwei.sujian.feature.editor.input

import android.view.inputmethod.EditorInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.EditorTransactionCauseDto

/**
 * InputConnection lifecycle contract tests (Issue #589).
 *
 * Composition validity is governed by the InputConnection lifecycle, the kernel
 * composition session (session id / base revision / generation) and the adapter state
 * machine — NOT by InputConnection creation. `onCreateInputConnection` is invoked not
 * only when the IME binding is replaced but also spuriously by unrelated callers
 * (Espresso view descriptions, direct connection probing, soft restarts), so cancelling
 * the composition there would destroy live compositions (the regression this file
 * guards against).
 *
 * An orphaned kernel session (IME switch mid-composition, soft reset) is healed lazily:
 * the kernel rejects the stale session and the adapter replays the edit as a plain
 * operation — no text loss, no connection-creation coupling.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], shadows = [RecordingInputMethodManagerShadow::class])
class AndroidInputAdapterOnCreateInputConnectionTest {
    @Test
    fun newInputConnection_doesNotDisturbLiveComposition() {
        // A spurious connection creation (Espresso description, probing) must not cancel
        // a live composition: the IME binding is still active and the kernel session is
        // still valid.
        val h = InputConnectionTestHarness("ABXY", 4)
        assertTrue(h.connection.setComposingRegion(0, 2))
        assertTrue(h.adapter.isComposing())
        assertTrue(h.mirror.hasComposition())
        assertTrue(h.commandPort.hasActiveSession())

        val newConnection = h.adapter.onCreateInputConnection(EditorInfo())

        assertNotNull("A fresh connection must still be created", newConnection)
        assertTrue("The live composition must survive connection recreation", h.adapter.isComposing())
        assertTrue("The mirror overlay must survive", h.mirror.hasComposition())
        assertTrue("The kernel session must survive", h.commandPort.hasActiveSession())
        assertEquals("No kernel cancel may be issued", 0, h.commandPort.cancelCompositionCount)

        // The composition still works through a fresh connection: replace the preedit.
        assertTrue(newConnection!!.setComposingText("Z", 1))
        assertEquals("Z", h.adapter.getCompositionText())
        assertEquals("ZXY", h.mirror.getText())
    }

    @Test
    fun newInputConnection_withoutComposition_isUnaffected() {
        val h = InputConnectionTestHarness("ABXY", 4)

        val newConnection = h.adapter.onCreateInputConnection(EditorInfo())

        assertNotNull(newConnection)
        assertEquals(0, h.commandPort.cancelCompositionCount)
        assertEquals(0, h.commandPort.beginCompositionCount)
    }

    @Test
    fun orphanedKernelSession_commitReplaysAsPlainReplaceWithoutLoss() {
        // IME switch mid-composition: the kernel session is gone (the new binding reset
        // it) but the adapter still believes it is composing with a stale session id.
        // The next commitText must land at the composition range as a plain replace —
        // no text loss, no kernel reload.
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11)
        h.connection.setComposingRegion(6, 11)
        h.connection.setComposingText("wrld", 1)
        assertEquals("hello wrld", h.mirror.getText())
        assertTrue(h.adapter.isComposing())

        // Simulate the orphan: the kernel session is cancelled/closed out-of-band while
        // the adapter still holds the stale session id.
        val (sessionId, _, generation) = h.adapter.compositionSessionInfo()
        h.commandPort.cancelComposition(sessionId, generation.toLong())
        assertFalse("Kernel session must be gone", h.commandPort.hasActiveSession())

        // Commit through a fresh connection (the new IME binding).
        val newConnection = h.adapter.onCreateInputConnection(EditorInfo())
        assertNotNull(newConnection)
        assertTrue(newConnection!!.commitText("WORLD", 1))

        assertFalse("The composition must be finished by the commit", h.adapter.isComposing())
        assertEquals("The replacement must land at the composition range", "hello WORLD", h.mirror.getCommittedText())
        assertEquals("Kernel text must match the mirror", "hello WORLD", h.commandPort.getKernelText())
        assertEquals(
            "The replay must be a plain replace, not a composition commit",
            uniffi.writer_core.EditorOperationKindDto.REPLACE,
            h.commandPort.commitCalls.last().operationKind,
        )
        assertEquals(
            "The replay must use the TYPING cause",
            EditorTransactionCauseDto.TYPING,
            h.commandPort.commitCalls.last().cause,
        )
        assertEquals("No kernel reload may be needed", 0, h.commandPort.reloadCount)
    }

    @Test
    fun orphanedKernelSession_onEmptyRange_replaysAsPlainInsert() {
        // Orphan created by a preedit on an empty range (composition started at the
        // cursor, no selection): the replay must be a plain insert at the same spot.
        val h = InputConnectionTestHarness("AB", 2)
        h.connection.setComposingText("X", 1)
        assertTrue(h.adapter.isComposing())
        assertEquals("ABX", h.mirror.getText())

        val (sessionId, _, generation) = h.adapter.compositionSessionInfo()
        h.commandPort.cancelComposition(sessionId, generation.toLong())
        assertFalse(h.commandPort.hasActiveSession())

        val newConnection = h.adapter.onCreateInputConnection(EditorInfo())
        assertNotNull(newConnection)
        assertTrue(newConnection!!.commitText("Y", 1))

        assertEquals("ABY", h.mirror.getCommittedText())
        assertEquals("ABY", h.commandPort.getKernelText())
        assertEquals(0, h.commandPort.reloadCount)
        assertEquals(
            "The replay must be a plain insert",
            uniffi.writer_core.EditorOperationKindDto.INSERT,
            h.commandPort.commitCalls.last().operationKind,
        )
    }
}
