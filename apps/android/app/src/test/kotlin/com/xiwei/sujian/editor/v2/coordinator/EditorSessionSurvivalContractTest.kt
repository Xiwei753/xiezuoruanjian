package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertTrue
import org.junit.Test

class EditorSessionSurvivalContractTest {

    @Test
    fun editorSessionViewModel_existsAndCanBeCreated() {
        val vm = EditorSessionViewModel()
        assertTrue(vm.sessionCoordinator == null)
    }

    @Test
    fun releaseWindow_existsOnWindowHost() {
        val method = EditorWindowHost::class.java.getMethod("releaseWindow")
        assertTrue(method != null)
    }

    @Test
    fun releaseHost_existsOnWindowHost() {
        val method = EditorWindowHost::class.java.getMethod("releaseHost")
        assertTrue(method != null)
    }

    @Test
    fun releaseHost_existsOnSessionCoordinator() {
        val method = EditorSessionCoordinator::class.java.getMethod("releaseHost")
        assertTrue(method != null)
    }

    @Test
    fun editingState_hasIdleAndReleased() {
        assertTrue(EditingState.IDLE.name == "IDLE")
        assertTrue(EditingState.RELEASED.name == "RELEASED")
    }
}
