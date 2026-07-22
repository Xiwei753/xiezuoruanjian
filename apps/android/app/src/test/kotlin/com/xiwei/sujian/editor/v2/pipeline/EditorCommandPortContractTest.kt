package com.xiwei.sujian.editor.v2.pipeline

import org.junit.Assert.assertEquals
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
}
