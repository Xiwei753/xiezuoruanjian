package com.xiwei.sujian.editor.v2.pipeline

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class EditorCommandPortContractTest {

    @Test
    fun pipelineOutputNeedReloadIsSingleton() {
        assertEquals(PipelineOutput.NeedReload, PipelineOutput.NeedReload)
    }

    @Test
    fun pipelineOutputStaleOrInvalidIsSingleton() {
        assertEquals(PipelineOutput.StaleOrInvalid, PipelineOutput.StaleOrInvalid)
    }

    @Test
    fun androidEditorPipelineImplementsEditorCommandPort() {
        assertTrue(EditorCommandPort::class.java.isAssignableFrom(AndroidEditorPipeline::class.java))
    }

    @Test
    fun pipelineOutputTypesAreDistinct() {
        val types = setOf(
            PipelineOutput.Edited::class,
            PipelineOutput.NeedReload::class,
            PipelineOutput.StaleOrInvalid::class
        )
        assertEquals(3, types.size)
    }

    @Test
    fun editorCommandPortDoesNotDeclareReplaceAll() {
        val methods = EditorCommandPort::class.java.declaredMethods.filter { it.name == "replaceAll" }
        assertTrue("replaceAll should not be on EditorCommandPort — use SessionCommandPort.applyTargetCommand(TargetCommand.ReplaceAll) instead", methods.isEmpty())
    }

    @Test
    fun editorCommandPortDeclaresGetCursorUtf8() {
        val method = EditorCommandPort::class.java.getDeclaredMethod("getCursorUtf8")
        assertNotNull(method)
        assertEquals(Int::class.javaPrimitiveType, method.returnType)
    }

    @Test
    fun editorCommandPortDeclaresGetRevision() {
        val method = EditorCommandPort::class.java.getDeclaredMethod("getRevision")
        assertNotNull(method)
        assertEquals(Long::class.javaPrimitiveType, method.returnType)
    }

    @Test
    fun editorCommandPortDeclaresGetText() {
        val method = EditorCommandPort::class.java.getDeclaredMethod("getText")
        assertNotNull(method)
        assertEquals(String::class.java, method.returnType)
    }

    @Test
    fun pipelineGetPipelineReturnsEditorCommandPort() {
        val method = Class.forName("com.xiwei.sujian.editor.v2.host.SujianEditorView")
            .getDeclaredMethod("getPipeline")
        assertEquals(EditorCommandPort::class.java, method.returnType)
    }

    @Test
    fun inputAdapterIsNotOnPipeline() {
        val fields = AndroidEditorPipeline::class.java.declaredFields.filter { it.name == "inputAdapter" }
        assertTrue("Pipeline should not own inputAdapter — it belongs to SujianEditorView", fields.isEmpty())
    }

    @Test
    fun layoutRuntimeIsInternal() {
        val field = AndroidEditorPipeline::class.java.getDeclaredField("layoutRuntime")
        assertTrue(!java.lang.reflect.Modifier.isPublic(field.modifiers))
    }

    @Test
    fun visualRuntimeIsInternal() {
        val field = AndroidEditorPipeline::class.java.getDeclaredField("visualRuntime")
        assertTrue(!java.lang.reflect.Modifier.isPublic(field.modifiers))
    }

    @Test
    fun renderRuntimeIsInternal() {
        val field = AndroidEditorPipeline::class.java.getDeclaredField("renderRuntime")
        assertTrue(!java.lang.reflect.Modifier.isPublic(field.modifiers))
    }
}
