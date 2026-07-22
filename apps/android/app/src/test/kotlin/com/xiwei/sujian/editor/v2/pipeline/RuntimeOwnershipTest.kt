package com.xiwei.sujian.editor.v2.pipeline

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RuntimeOwnershipTest {

    @Test
    fun pipelineDelegatesToLayoutRuntime() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline")
        val layoutRuntimeField = clazz.getDeclaredField("layoutRuntime")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidLayoutRuntime"), layoutRuntimeField.type)
    }

    @Test
    fun pipelineDelegatesToVisualRuntime() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline")
        val visualRuntimeField = clazz.getDeclaredField("visualRuntime")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime"), visualRuntimeField.type)
    }

    @Test
    fun layoutRuntimeOwnsLayoutEngine() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidLayoutRuntime")
        val layoutEngineField = clazz.getDeclaredField("layoutEngine")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine"), layoutEngineField.type)
    }

    @Test
    fun visualRuntimeOwnsAnimationEngine() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime")
        val animationEngineField = clazz.getDeclaredField("animationEngine")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.visual.AndroidTextAnimationEngine"), animationEngineField.type)
    }

    @Test
    fun layoutRuntimeHasApplyProjection() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidLayoutRuntime")
        val method = clazz.getDeclaredMethod("applyProjection", Class.forName("com.xiwei.sujian.editor.v2.projection.DisplayTextProjection"))
        assertNotNull(method)
    }

    @Test
    fun visualRuntimeHasPrepareAndSubmit() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime")
        val methods = clazz.declaredMethods.filter { it.name == "prepareAndSubmit" }
        assertTrue(methods.isNotEmpty())
    }

    @Test
    fun inputAdapterDependsOnEditorCommandPortInterface() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.input.AndroidInputAdapter")
        val constructor = clazz.getDeclaredConstructor(
            Class.forName("com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror"),
            Class.forName("com.xiwei.sujian.editor.v2.pipeline.EditorCommandPort")
        )
        assertNotNull(constructor)
    }

    @Test
    fun pipelineImplementsEditorCommandPort() {
        assertTrue(EditorCommandPort::class.java.isAssignableFrom(AndroidEditorPipeline::class.java))
    }

    @Test
    fun coordinatorImplementsSessionCommandPort() {
        assertTrue(Class.forName("com.xiwei.sujian.editor.v2.coordinator.SessionCommandPort")
            .isAssignableFrom(Class.forName("com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator")))
    }
}
