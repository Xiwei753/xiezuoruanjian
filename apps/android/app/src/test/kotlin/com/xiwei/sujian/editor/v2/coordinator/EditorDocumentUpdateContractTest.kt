package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 一：EditorDocumentUpdate 类型化正文更新协议契约测试。
 *
 * 验证 LocalInput 和 ExternalReplace 的来源区分、revision 携带和
 * 替代 generation/hashCode 启发式判断的正确性。
 */
class EditorDocumentUpdateContractTest {

    @Test
    fun localInputCarriesTransactionIdAndRevision() {
        val update = EditorDocumentUpdate.LocalInput(
            targetId = "chapter-body:p:v:c",
            text = "hello",
            revision = 5L,
            transactionId = 42L,
        )
        assertEquals("chapter-body:p:v:c", update.targetId)
        assertEquals("hello", update.text)
        assertEquals(5L, update.revision)
        assertEquals(42L, update.transactionId)
        assertEquals(EditorOperationKind.INSERT, update.operationKind)
    }

    @Test
    fun externalReplaceCarriesSource() {
        val update = EditorDocumentUpdate.ExternalReplace(
            targetId = "chapter-body:p:v:c",
            text = "replaced",
            revision = 10L,
            source = ExternalSource.SYNC_MERGE,
        )
        assertEquals("chapter-body:p:v:c", update.targetId)
        assertEquals("replaced", update.text)
        assertEquals(10L, update.revision)
        assertEquals(ExternalSource.SYNC_MERGE, update.source)
    }

    @Test
    fun localInputAndExternalReplaceAreDistinctTypes() {
        val local = EditorDocumentUpdate.LocalInput("t", "text", 1L, 100L)
        val external = EditorDocumentUpdate.ExternalReplace("t", "text", 1L, ExternalSource.REPOSITORY_LOAD)
        assertNotEquals("LocalInput and ExternalReplace must be distinct", local, external)
    }

    @Test
    fun externalSourceDistinguishesAllOrigins() {
        val sources = ExternalSource.values()
        assertEquals(4, sources.size)
        assertTrue(sources.contains(ExternalSource.REPOSITORY_LOAD))
        assertTrue(sources.contains(ExternalSource.SYNC_MERGE))
        assertTrue(sources.contains(ExternalSource.UNDO_RESTORE))
        assertTrue(sources.contains(ExternalSource.PROGRAMMATIC_REPLACE))
    }

    @Test
    fun editorSessionStateDefaultsToNone() {
        val state = EditorSessionState()
        assertEquals(null, state.targetId)
        assertEquals("", state.text)
        assertEquals(0L, state.revision)
        assertEquals(EditorSessionOrigin.NONE, state.origin)
        assertEquals(WindowBindingState.Idle, state.bindingState)
    }

    @Test
    fun localInputPreservesRevisionAcrossUiRoundTrip() {
        // 模拟本地输入：IME → Rust EditResult(rev=5) → applyLocalEdit → SessionState(rev=5)
        // WritingPane 收集 sessionStateFlow 发现 rev=5 已应用，不触发 reset
        val update = EditorDocumentUpdate.LocalInput("t", "new text", 5L, 42L)
        val state = EditorSessionState(
            targetId = update.targetId,
            text = update.text,
            revision = update.revision,
            lastAppliedTransactionId = update.transactionId,
            origin = EditorSessionOrigin.LOCAL_INPUT,
        )
        assertEquals(5L, state.revision)
        assertEquals(42L, state.lastAppliedTransactionId)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, state.origin)
    }
}
