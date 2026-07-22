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
    fun pipelineDelegatesToRenderRuntime() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline")
        val renderRuntimeField = clazz.getDeclaredField("renderRuntime")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidRenderRuntime"), renderRuntimeField.type)
    }

    @Test
    fun renderRuntimeOwnsTextRenderer() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidRenderRuntime")
        val field = clazz.getDeclaredField("textRenderer")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.render.AndroidTextRenderer"), field.type)
    }

    @Test
    fun renderRuntimeOwnsAnimationRenderer() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidRenderRuntime")
        val field = clazz.getDeclaredField("animationRenderer")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.render.AndroidTextAnimationRenderer"), field.type)
    }

    @Test
    fun renderRuntimeOwnsFrameComposer() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidRenderRuntime")
        val field = clazz.getDeclaredField("frameComposer")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.render.EditorFrameComposer"), field.type)
    }

    @Test
    fun renderRuntimeOwnsDrawFrame() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidRenderRuntime")
        val methods = clazz.declaredMethods.filter { it.name == "drawFrame" }
        assertTrue("AndroidRenderRuntime should have drawFrame method", methods.isNotEmpty())
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
    fun visualRuntimeHasTick() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime")
        val methods = clazz.declaredMethods.filter { it.name == "tick" }
        assertTrue("AndroidVisualRuntime should have tick method", methods.isNotEmpty())
    }

    @Test
    fun inputAdapterDependsOnInputCommandPortInterface() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.input.AndroidInputAdapter")
        val constructor = clazz.getDeclaredConstructor(
            Class.forName("com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror"),
            Class.forName("com.xiwei.sujian.editor.v2.pipeline.InputCommandPort")
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

    @Test
    fun layoutRuntimeOwnsSecretProjectionState() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidLayoutRuntime")
        val secretField = clazz.getDeclaredField("secretDisplayMode")
        assertEquals(Boolean::class.javaPrimitiveType, secretField.type)
        val setMethod = clazz.getDeclaredMethod("setSecretDisplayMode", Boolean::class.javaPrimitiveType)
        assertNotNull(setMethod)
        val applyMethod = clazz.getDeclaredMethod("applySecretDisplayIfActive")
        assertNotNull(applyMethod)
    }

    @Test
    fun pipelineDoesNotOwnSecretProjectionState() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline")
        val secretFields = clazz.declaredFields.filter { it.name == "secretDisplayMode" || it.name == "currentProjection" || it.name == "inputAdapter" }
        assertTrue("Pipeline should not own secretDisplayMode, currentProjection or inputAdapter — they belong to LayoutRuntime/View", secretFields.isEmpty())
    }

    @Test
    fun pipelineDoesNotOwnRenderersDirectly() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline")
        val rendererFields = clazz.declaredFields.filter { it.name == "textRenderer" || it.name == "animationRenderer" || it.name == "frameComposer" }
        assertTrue("Pipeline should not own textRenderer/animationRenderer/frameComposer directly — they belong to RenderRuntime", rendererFields.isEmpty())
    }

    @Test
    fun visualRuntimeDefaultConstructorSharesPlannerAndResourceStore() {
        val runtime = AndroidVisualRuntime()
        val plannerField = AndroidVisualRuntime::class.java.getDeclaredField("visualPlanner")
        plannerField.isAccessible = true
        val engineField = AndroidVisualRuntime::class.java.getDeclaredField("animationEngine")
        engineField.isAccessible = true
        val storeField = AndroidVisualRuntime::class.java.getDeclaredField("resourceStore")
        storeField.isAccessible = true

        val outerPlanner = plannerField.get(runtime)
        val engine = engineField.get(runtime)
        val outerStore = storeField.get(runtime)

        val enginePlannerField = engine.javaClass.getDeclaredField("visualPlanner")
        enginePlannerField.isAccessible = true
        val engineStoreField = engine.javaClass.getDeclaredField("resourceStore")
        engineStoreField.isAccessible = true

        val enginePlanner = enginePlannerField.get(engine)
        val engineStore = engineStoreField.get(engine)

        assertTrue("visualPlanner must be same instance as animationEngine's planner", outerPlanner === enginePlanner)
        assertTrue("resourceStore must be same instance as animationEngine's store", outerStore === engineStore)
    }

    @Test
    fun visualRuntimeSecondaryConstructorSharesPlannerAndResourceStore() {
        val planner = com.xiwei.sujian.editor.v2.visual.AndroidVisualPlanner()
        val store = com.xiwei.sujian.editor.v2.visual.VisualResourceStore()
        val runtime = AndroidVisualRuntime(planner, store)

        val plannerField = AndroidVisualRuntime::class.java.getDeclaredField("visualPlanner")
        plannerField.isAccessible = true
        val engineField = AndroidVisualRuntime::class.java.getDeclaredField("animationEngine")
        engineField.isAccessible = true
        val storeField = AndroidVisualRuntime::class.java.getDeclaredField("resourceStore")
        storeField.isAccessible = true

        val outerPlanner = plannerField.get(runtime)
        val engine = engineField.get(runtime)
        val outerStore = storeField.get(runtime)

        val enginePlannerField = engine.javaClass.getDeclaredField("visualPlanner")
        enginePlannerField.isAccessible = true
        val engineStoreField = engine.javaClass.getDeclaredField("resourceStore")
        engineStoreField.isAccessible = true

        val enginePlanner = enginePlannerField.get(engine)
        val engineStore = engineStoreField.get(engine)

        assertTrue("visualPlanner must be same instance as passed planner", outerPlanner === planner)
        assertTrue("resourceStore must be same instance as passed store", outerStore === store)
        assertTrue("engine's planner must be same instance as passed planner", enginePlanner === planner)
        assertTrue("engine's store must be same instance as passed store", engineStore === store)
    }

    @Test
    fun renderRuntimeDrawFrameDoesNotDependOnLayoutRuntimeOrVisualRuntime() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidRenderRuntime")
        val drawFrameMethods = clazz.declaredMethods.filter { it.name == "drawFrame" }
        assertTrue("AndroidRenderRuntime should have drawFrame method", drawFrameMethods.isNotEmpty())
        val method = drawFrameMethods.first()
        val paramTypes = method.parameterTypes.map { it.name }
        assertFalse("drawFrame should not take AndroidLayoutRuntime as parameter",
            paramTypes.any { it.contains("AndroidLayoutRuntime") })
        assertFalse("drawFrame should not take AndroidVisualRuntime as parameter",
            paramTypes.any { it.contains("AndroidVisualRuntime") })
    }

    @Test
    fun renderRuntimeHasDrawFromFrameState() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidRenderRuntime")
        val method = clazz.getDeclaredMethod("drawFromFrameState",
            android.graphics.Canvas::class.java,
            Class.forName("com.xiwei.sujian.editor.v2.pipeline.FrameState")
        )
        assertNotNull(method)
    }

    @Test
    fun pipelineDrawFrameConvergesRuntimeAccess() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline")
        val method = clazz.getDeclaredMethod("drawFrame",
            android.graphics.Canvas::class.java,
            List::class.java,
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
            Float::class.javaPrimitiveType,
            Float::class.javaPrimitiveType
        )
        assertNotNull(method)
    }

    @Test
    fun frameStateContainsRenderInput() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.FrameState")
        val field = clazz.getDeclaredField("renderInput")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.pipeline.FrameRenderInput"), field.type)
    }

    @Test
    fun targetDisplayRuntimeHasClearDecorations() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        val method = clazz.getDeclaredMethod("clearDecorations")
        assertNotNull(method)
    }

    @Test
    fun targetDisplayRuntimeHasDrawFrame() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        val method = clazz.getDeclaredMethod("drawFrame", android.graphics.Canvas::class.java)
        assertNotNull(method)
    }

    @Test
    fun targetDisplayRuntimeHasVisualRuntime() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        val method = clazz.getDeclaredMethod("getVisualRuntime")
        assertEquals(Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime"), method.returnType)
    }

    @Test
    fun targetDisplayRuntimeHasScrollPosition() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        val setMethod = clazz.getDeclaredMethod("setScrollPosition", Float::class.javaPrimitiveType, Float::class.javaPrimitiveType)
        assertNotNull(setMethod)
        val getXMethod = clazz.getDeclaredMethod("getScrollX")
        assertNotNull(getXMethod)
        val getYMethod = clazz.getDeclaredMethod("getScrollY")
        assertNotNull(getYMethod)
    }
}
