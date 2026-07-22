package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ViewModelSyncAfterReplaceTest {

    @Test
    fun coordinatorHasUpdateTargetTextMethod() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val method = clazz.getDeclaredMethod("updateTargetText", String::class.java, String::class.java)
        assertTrue(method != null)
    }

    @Test
    fun coordinatorHasOnTargetContentChangedCallback() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val field = clazz.getDeclaredField("onTargetContentChanged")
        assertTrue(field != null)
    }

    @Test
    fun targetCommandResultSuccessCarriesSnapshot() {
        val snap = TargetSnapshot("replaced", 8, 2L, 0, 8)
        val result = TargetCommandResult.Success(snap)
        assertEquals("replaced", result.snapshot.text)
        assertEquals(8, result.snapshot.cursorUtf8)
        assertEquals(2L, result.snapshot.revision)
    }

    @Test
    fun targetCommandResultFailedCarriesReason() {
        val result = TargetCommandResult.Failed(TargetCommandError.KERNEL_REJECTED)
        assertEquals(TargetCommandError.KERNEL_REJECTED, result.reason)
    }

    @Test
    fun sessionResetSourceHasExternalValue() {
        assertTrue(SessionResetSource.values().contains(SessionResetSource.EXTERNAL))
    }

    @Test
    fun sessionResetSourceHasChapterSwitchValue() {
        assertTrue(SessionResetSource.values().contains(SessionResetSource.CHAPTER_SWITCH))
    }

    @Test
    fun sessionResetSourceHasLocalContentChangedValue() {
        assertTrue(SessionResetSource.values().contains(SessionResetSource.LOCAL_CONTENT_CHANGED))
    }
}
