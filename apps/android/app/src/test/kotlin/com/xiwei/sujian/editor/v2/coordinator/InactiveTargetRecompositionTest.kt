package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class InactiveTargetRecompositionTest {

    @Test
    fun coordinatorHasTargetDecorationsVersionState() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val method = clazz.getDeclaredMethod("getTargetDecorationsVersion")
        assertEquals(Long::class.javaPrimitiveType, method.returnType)
    }

    @Test
    fun coordinatorHasOnTargetContentChangedCallback() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val field = clazz.getDeclaredField("onTargetContentChanged")
        assertNotNull(field)
    }

    @Test
    fun coordinatorImplementsSessionCommandPort() {
        assertTrue(Class.forName("com.xiwei.sujian.editor.v2.coordinator.SessionCommandPort")
            .isAssignableFrom(Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")))
    }

    @Test
    fun sessionCommandPortHasQueryTargetSnapshot() {
        val iface = Class.forName("com.xiwei.sujian.editor.v2.coordinator.SessionCommandPort")
        val method = iface.getDeclaredMethod("queryTargetSnapshot", String::class.java)
        assertNotNull(method)
    }

    @Test
    fun sessionCommandPortHasApplyTargetCommand() {
        val iface = Class.forName("com.xiwei.sujian.editor.v2.coordinator.SessionCommandPort")
        val method = iface.getDeclaredMethod("applyTargetCommand", String::class.java, Class.forName("com.xiwei.sujian.editor.v2.coordinator.TargetCommand"))
        assertNotNull(method)
    }

    @Test
    fun sessionCommandPortHasSetTargetDecorations() {
        val iface = Class.forName("com.xiwei.sujian.editor.v2.coordinator.SessionCommandPort")
        val method = iface.getDeclaredMethod("setTargetDecorations", String::class.java, Class.forName("com.xiwei.sujian.editor.v2.coordinator.TargetDecorations"))
        assertNotNull(method)
    }

    @Test
    fun coordinatorHasGetTargetTextMethod() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val method = clazz.getDeclaredMethod("getTargetText", String::class.java)
        assertNotNull(method)
    }

    @Test
    fun coordinatorHasGetTargetProjectionMethod() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")
        val method = clazz.getDeclaredMethod("getTargetProjection", String::class.java)
        assertNotNull(method)
    }

    @Test
    fun targetCommandReplaceUsesRealRevision() {
        val cmd = TargetCommand.Replace(0, 5, "world", "hello")
        assertEquals(0, cmd.byteStart)
        assertEquals(5, cmd.byteEndExclusive)
        assertEquals("world", cmd.replacementText)
        assertEquals("hello", cmd.originalText)
    }

    @Test
    fun targetCommandReplaceAllUsesRealRevision() {
        val cmd = TargetCommand.ReplaceAll("old", "new")
        assertEquals("old", cmd.searchText)
        assertEquals("new", cmd.replacementText)
    }

    private fun assertNotNull(obj: Any?) {
        assertTrue("Expected non-null value", obj != null)
    }
}
