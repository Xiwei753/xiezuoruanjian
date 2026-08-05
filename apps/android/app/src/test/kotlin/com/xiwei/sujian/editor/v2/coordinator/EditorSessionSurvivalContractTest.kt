package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertTrue
import org.junit.Test

class EditorSessionSurvivalContractTest {

    @Test
    fun editorSessionViewModel_existsAndCanBeCreated() {
        val vm = EditorSessionViewModel()
        assertTrue(vm.coordinator == null)
    }

    @Test
    fun releaseWindowOnly_existsOnCoordinator() {
        val method = AnimatedTextEditorCoordinator::class.java.getMethod("releaseWindowOnly")
        assertTrue(method != null)
    }

    @Test
    fun releaseHost_existsOnCoordinator() {
        val method = AnimatedTextEditorCoordinator::class.java.getMethod("releaseHost")
        assertTrue(method != null)
    }

    @Test
    fun editingState_hasIdleAndReleased() {
        assertTrue(EditingState.IDLE.name == "IDLE")
        assertTrue(EditingState.RELEASED.name == "RELEASED")
    }
}
