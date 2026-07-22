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
    fun editorCommandPortDeclaresReplaceAll() {
        val method = EditorCommandPort::class.java.getDeclaredMethod("replaceAll", String::class.java, String::class.java)
        assertNotNull(method)
        assertEquals(PipelineOutput::class.java, method.returnType)
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
    fun inputAdapterIsPrivateSet() {
        val field = AndroidEditorPipeline::class.java.getDeclaredField("inputAdapter")
        val setter = AndroidEditorPipeline::class.java.declaredMethods.filter {
            it.name == "setInputAdapter" && it.parameterCount == 1
        }
        assertTrue(setter.isEmpty() || setter.all { !java.lang.reflect.Modifier.isPublic(it.modifiers) })
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
}
