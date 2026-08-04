package com.xiwei.sujian.editor.v2.input

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
 * Root cause: Android does not guarantee `finishComposingText()` before it discards an
 * InputConnection (IME restart, focus regain, soft reset). The adapter can therefore be
 * left holding an orphan composition whose kernel session can never be updated, committed
 * or finished. A later plain `commitText` would then be misrouted into the composition
 * commit path, rejected by the kernel as StaleRevision and dropped (text loss).
 *
 * The fix is a pure lifecycle hook: creating a fresh InputConnection terminates the
 * previous IME binding, so [AndroidInputAdapter.onCreateInputConnection] cancels any
 * orphan composition first. No IME enumeration/switch is involved — validity remains
 * governed by the InputConnection lifecycle, the kernel session and the adapter state
 * machine.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], shadows = [RecordingInputMethodManagerShadow::class])
class AndroidInputAdapterOnCreateInputConnectionTest {

    @Test
    fun newInputConnection_cancelsOrphanComposition() {
        val h = InputConnectionTestHarness("ABXY", 4)
        assertTrue(h.connection.setComposingRegion(0, 2))
        assertTrue(h.adapter.isComposing())

        val newConnection = h.adapter.onCreateInputConnection(EditorInfo())

        assertNotNull("A fresh connection must still be created", newConnection)
        assertFalse("The orphan composition must be cancelled", h.adapter.isComposing())
        assertFalse("The mirror overlay must be removed", h.mirror.hasComposition())
        assertEquals("The kernel session must be cancelled", 1, h.commandPort.cancelCompositionCount)
        assertEquals("Committed text must survive", "ABXY", h.mirror.getText())

        // A subsequent plain commit must be routed as a normal TYPING commit (no orphan
        // session, no text loss).
        assertTrue(h.connection.commitText("z", 1))
        assertEquals("ABXYz", h.mirror.getText())
        assertEquals(EditorTransactionCauseDto.TYPING, h.commandPort.commitCalls.single().cause)
    }

    @Test
    fun newInputConnection_withoutOrphanComposition_isUnaffected() {
        val h = InputConnectionTestHarness("ABXY", 4)

        val newConnection = h.adapter.onCreateInputConnection(EditorInfo())

        assertNotNull(newConnection)
        assertEquals(0, h.commandPort.cancelCompositionCount)
        assertEquals(0, h.commandPort.beginCompositionCount)
    }

    @Test
    fun newInputConnection_cleansOrphanWithPreeditEdit() {
        // Orphan with a non-trivial preedit: setComposingRegion + setComposingText then a
        // connection restart. The overlay text must not leak into the committed text.
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setComposingRegion(6, 11)
        h.connection.setComposingText("foo", 1)
        assertEquals("hello foo", h.mirror.getText())

        h.adapter.onCreateInputConnection(EditorInfo())

        assertFalse(h.adapter.isComposing())
        assertEquals("Preedit must be rolled back, not committed", "hello world", h.mirror.getText())
        assertEquals(1, h.commandPort.cancelCompositionCount)
    }
}
