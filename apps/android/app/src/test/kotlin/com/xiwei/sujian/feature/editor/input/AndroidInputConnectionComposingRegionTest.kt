package com.xiwei.sujian.feature.editor.input

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Contract tests for [AndroidInputConnection.setComposingRegion] (Issue #589).
 *
 * The pre-#589 behavior gated setComposingRegion on `InputMethodManager.enabledInputMethodList`
 * and ignored the call when no IME was enabled; it also rejected reversed ranges and ended
 * with an extra InputMethodManager.updateSelection. Per the Android InputConnection contract
 * the method must instead:
 * - reject negative offsets (return false);
 * - normalize reversed ranges (start > end) to [min, max) before entering composing mode;
 * - never trigger updateSelection (the committed text and selection do not change);
 * - accept calls regardless of which (if any) IME is enabled — composition validity is
 *   managed by the InputConnection lifecycle, the kernel composition session and the
 *   adapter state machine.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], shadows = [RecordingInputMethodManagerShadow::class])
class AndroidInputConnectionComposingRegionTest {
    @Test
    fun reversedRange_isNormalizedAndEntersComposing() {
        val h = InputConnectionTestHarness("ABXY", 4)

        val result = h.connection.setComposingRegion(2, 0)

        assertTrue("Reversed range must be accepted and normalized", result)
        assertTrue("The call must enter composing mode", h.adapter.isComposing())
        assertEquals("Kernel composition must be started once", 1, h.commandPort.beginCompositionCount)
        assertEquals(
            "Range must be normalized to [0,2) in UTF-8 bytes",
            Pair(0, 2),
            h.adapter.getCompositionRangeUtf8(),
        )
        assertEquals("Preedit must be the normalized region text", "AB", h.adapter.getCompositionText())
        assertEquals("Committed text must be untouched by the overlay", "ABXY", h.mirror.getCommittedText())
        assertTrue("Mirror must show the composing overlay", h.mirror.hasComposition())
    }

    @Test
    fun negativeRange_isRejected() {
        val h = InputConnectionTestHarness("ABXY", 4)

        assertFalse("Negative start must be rejected", h.connection.setComposingRegion(-1, 2))
        assertFalse("Negative end must be rejected", h.connection.setComposingRegion(0, -2))
        assertFalse("Negative offsets must not enter composing mode", h.adapter.isComposing())
        assertEquals("No kernel composition may be started", 0, h.commandPort.beginCompositionCount)
    }

    @Test
    fun noEnabledIme_doesNotGateComposing() {
        // Robolectric's default device has no enabled IME. The deleted behavior ignored
        // the call in this state; the new contract accepts it — validity is managed by the
        // InputConnection lifecycle, the kernel session and the adapter state machine.
        val h = InputConnectionTestHarness("ABXY", 4)

        val result = h.connection.setComposingRegion(0, 2)

        assertTrue("The call must be accepted with no enabled IME", result)
        assertTrue("The call must enter composing mode", h.adapter.isComposing())
        assertEquals("Kernel composition must be started", 1, h.commandPort.beginCompositionCount)
    }

    @Test
    fun setComposingRegion_doesNotTriggerUpdateSelection() {
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11) // select "world" (notifies the IME once)

        val before = h.imm.updateSelectionCount
        val result = h.connection.setComposingRegion(6, 11)
        assertTrue(result)
        assertTrue(h.adapter.isComposing())

        assertEquals(
            "setComposingRegion must not produce an extra selection callback",
            before,
            h.imm.updateSelectionCount,
        )
    }

    @Test
    fun commitText_stillTriggersSingleUpdateSelection() {
        // Contrast: commitText changes committed text and selection, so it must keep
        // notifying the IME exactly once per call.
        val h = InputConnectionTestHarness("hello world", 11)

        val before = h.imm.updateSelectionCount
        h.connection.commitText("!", 1)

        assertEquals("commitText must notify the IME exactly once", before + 1, h.imm.updateSelectionCount)
    }

    @Test
    fun setSelection_stillTriggersUpdateSelection() {
        val h = InputConnectionTestHarness("hello world", 11)

        val before = h.imm.updateSelectionCount
        h.connection.setSelection(6, 11)

        assertEquals("setSelection must notify the IME exactly once", before + 1, h.imm.updateSelectionCount)
    }

    @Test
    fun reCorrection_beginsCompositionOnCommittedText() {
        // LatinIME recorrection: after a selection change the IME calls setComposingRegion
        // over the already committed word to re-enter composition on it.
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setSelection(6, 11)

        val result = h.connection.setComposingRegion(6, 11)

        assertTrue(result)
        assertTrue("Re-correction must begin a composition", h.adapter.isComposing())
        assertEquals("Preedit must be the selected committed text", "world", h.adapter.getCompositionText())
        assertEquals("Composition range must cover the selected word", Pair(6, 11), h.adapter.getCompositionRangeUtf8())
        assertEquals("Kernel composition must be started", 1, h.commandPort.beginCompositionCount)
        assertEquals("Committed text must be unchanged", "hello world", h.mirror.getCommittedText())
        assertTrue(h.mirror.hasComposition())
    }

    @Test
    fun setComposingRegionWhileComposing_replacesPreviousComposition() {
        val h = InputConnectionTestHarness("hello world", 11)
        h.connection.setComposingRegion(6, 11)
        assertTrue(h.adapter.isComposing())
        assertEquals("world", h.adapter.getCompositionText())

        val result = h.connection.setComposingRegion(0, 5)

        assertTrue(result)
        assertTrue("Composing mode must continue", h.adapter.isComposing())
        assertEquals("Old preedit must be replaced by the new region", "hello", h.adapter.getCompositionText())
        assertEquals(Pair(0, 5), h.adapter.getCompositionRangeUtf8())
        assertEquals("A fresh kernel session must be begun", 2, h.commandPort.beginCompositionCount)
        assertEquals("Committed text must survive the replacement", "hello world", h.mirror.getCommittedText())
    }

    @Test
    fun multiByteUtf16Offsets_convertToUtf8Bytes() {
        // "你好世界": each char is 1 UTF-16 unit but 3 UTF-8 bytes. UTF-16 [0,2) is
        // UTF-8 byte range [0,6).
        val h = InputConnectionTestHarness("你好世界", 12)

        val result = h.connection.setComposingRegion(0, 2)

        assertTrue(result)
        assertTrue(h.adapter.isComposing())
        assertEquals("你好", h.adapter.getCompositionText())
        assertEquals(Pair(0, 6), h.adapter.getCompositionRangeUtf8())
        assertEquals("你好世界", h.mirror.getCommittedText())
    }

    @Test
    fun multiByteReversedRange_normalizesInUtf16Space() {
        // Reversed UTF-16 range [4,2) normalizes to [2,4) = "世界" → UTF-8 bytes [6,12).
        val h = InputConnectionTestHarness("你好世界", 12)

        val result = h.connection.setComposingRegion(4, 2)

        assertTrue(result)
        assertTrue(h.adapter.isComposing())
        assertEquals("世界", h.adapter.getCompositionText())
        assertEquals(Pair(6, 12), h.adapter.getCompositionRangeUtf8())
    }

    @Test
    fun mixedAsciiAndMultiByte_reversedRange_convertsCorrectly() {
        // "a你b": UTF-16 offsets — a=0, 你=[1,2), b=2. UTF-8 bytes — a=[0,1), 你=[1,4), b=[4,5).
        val h = InputConnectionTestHarness("a你b", 5)

        val result = h.connection.setComposingRegion(2, 1)

        assertTrue(result)
        assertTrue(h.adapter.isComposing())
        assertEquals("你", h.adapter.getCompositionText())
        assertEquals(Pair(1, 4), h.adapter.getCompositionRangeUtf8())
    }
}
