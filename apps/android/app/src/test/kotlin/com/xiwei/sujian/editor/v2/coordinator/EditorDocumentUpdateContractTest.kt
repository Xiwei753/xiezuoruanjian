package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 一/二：EditorDocumentUpdate 类型化正文更新协议契约测试。
 *
 * 验证 LocalInput（本地输入）与 RepositoryLoaded（真实来源外部加载）的
 * 来源区分、revision/hash 携带和替代 generation/hashCode 启发式判断的正确性。
 * #595 二：ExternalReplace/ExternalSource 已删除 — UI 不得伪造 revision/source，
 * 外部更新只由真实来源事件驱动。
 */
class EditorDocumentUpdateContractTest {

    @Test
    fun localInputCarriesTransactionIdAndRevision() {
        val update = EditorDocumentUpdate.LocalInput(
            targetId = "chapter-body:p:v:c",
            text = "hello",
            revision = 5L,
            contentVersion = 1L,
            transactionId = 42L,
        )
        assertEquals("chapter-body:p:v:c", update.targetId)
        assertEquals("hello", update.text)
        assertEquals(5L, update.revision)
        assertEquals(42L, update.transactionId)
        assertEquals(EditorOperationKind.INSERT, update.operationKind)
    }

    @Test
    fun repositoryLoadedCarriesRealFileHash() {
        val update = EditorDocumentUpdate.RepositoryLoaded(
            targetId = "chapter-body:p:v:c",
            text = "loaded",
            fileHash = "sha256:abc123",
            revision = 0L,
            contentVersion = 1L,
        )
        assertEquals("chapter-body:p:v:c", update.targetId)
        assertEquals("loaded", update.text)
        assertEquals("sha256:abc123", update.fileHash)
        assertEquals(0L, update.revision)
    }

    @Test
    fun localInputAndRepositoryLoadedAreDistinctTypes() {
        val local = EditorDocumentUpdate.LocalInput("t", "text", 1L, 1L, 100L)
        val external = EditorDocumentUpdate.RepositoryLoaded("t", "text", "hash-1", 0L, contentVersion = 1L)
        assertNotEquals("LocalInput and RepositoryLoaded must be distinct", local, external)
    }

    @Test
    fun uiMustNotFabricateExternalReplace() {
        // #595 二：UI 不再持有 ExternalReplace/ExternalSource 伪造入口 —
        // 任何字符串差异都不能现场构造“更高 revision”覆盖编辑状态。
        val fabricatedReplaceClass = EditorDocumentUpdate::class.java.declaredClasses.firstOrNull {
            it.simpleName == "ExternalReplace"
        }
        assertTrue(
            "EditorDocumentUpdate.ExternalReplace must be removed (#595 二)",
            fabricatedReplaceClass == null,
        )
        val fabricatedSourceClass = try {
            Class.forName("com.xiwei.sujian.editor.v2.coordinator.ExternalSource")
        } catch (_: ClassNotFoundException) {
            null
        }
        assertTrue("ExternalSource enum must be removed (#595 二)", fabricatedSourceClass == null)
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
        val update = EditorDocumentUpdate.LocalInput("t", "new text", 5L, 1L, 42L)
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
