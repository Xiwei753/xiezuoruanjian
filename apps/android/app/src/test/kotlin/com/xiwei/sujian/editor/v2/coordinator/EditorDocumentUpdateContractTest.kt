package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 一/二：EditorDocumentUpdate 类型化正文更新协议契约测试。
 *
 * 验证 LocalInput（本地输入）与 TargetDocumentFact（Repository 加载/同步合并
 * 文档事实）的来源区分、revision/版本锚点携带。
 * #595 二：ExternalReplace/ExternalSource 已删除；contentVersion（进程内事件
 * 序号）已删除 — 新旧判断由 DocumentVersion（contentHash + manifest 锚点）完成，
 * 事件总线保存每个 target 的完整文档事实而非"最后一个事件对象"。
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
    fun documentFactCarriesRealVersionAnchors() {
        val fact = TargetDocumentFact(
            targetId = "chapter-body:p:v:c",
            text = "loaded",
            sourceVersion = DocumentVersion(contentHash = "sha256:abc123"),
            baseVersion = DocumentVersion(),
            origin = DocumentFactOrigin.REPOSITORY_LOAD,
        )
        assertEquals("chapter-body:p:v:c", fact.targetId)
        assertEquals("loaded", fact.text)
        assertEquals("sha256:abc123", fact.sourceVersion.contentHash)
        assertEquals(DocumentFactOrigin.REPOSITORY_LOAD, fact.origin)
    }

    @Test
    fun localInputAndDocumentFactAreDistinctTypes() {
        val local = EditorDocumentUpdate.LocalInput("t", "text", 1L, 100L)
        val fact = TargetDocumentFact(
            "t", "text",
            DocumentVersion(contentHash = "hash-1"),
            DocumentVersion(),
            DocumentFactOrigin.REPOSITORY_LOAD,
        )
        assertNotEquals("LocalInput and TargetDocumentFact must be distinct", local, fact)
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
    fun uiCounterContentVersionIsRemoved() {
        // #595 二：contentVersion（进程内事件序号）必须已删除 —
        // 版本锚点是 DocumentVersion。
        val updateClass = EditorDocumentUpdate.LocalInput::class.java
        assertTrue(
            "LocalInput must not carry contentVersion (#595 二)",
            updateClass.declaredFields.none { it.name == "contentVersion" },
        )
        assertTrue(
            "EditorSessionState must not carry lastAppliedContentVersion (#595 二)",
            EditorSessionState::class.java.declaredFields.none { it.name == "lastAppliedContentVersion" },
        )
        assertTrue(
            "EditorSessionState must not carry lastRepositoryHash (#595 二)",
            EditorSessionState::class.java.declaredFields.none { it.name == "lastRepositoryHash" },
        )
        assertTrue(
            "EditorSessionCoordinator must not expose nextContentVersion (#595 二)",
            EditorSessionCoordinator::class.java.methods.none { it.name == "nextContentVersion" },
        )
    }

    @Test
    fun editorSessionStateDefaultsToNone() {
        val state = EditorSessionState()
        assertEquals(null, state.targetId)
        assertEquals("", state.text)
        assertEquals(0L, state.revision)
        assertEquals(EditorSessionOrigin.NONE, state.origin)
        assertEquals(WindowBindingState.Idle, state.bindingState)
        assertEquals(DocumentVersion(), state.committedVersion)
        assertEquals(DocumentVersion(), state.sessionBaseVersion)
        assertTrue(!state.localDirty)
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

    @Test
    fun documentVersionEmptyWhenNoAnchors() {
        assertTrue(DocumentVersion().isEmpty)
        assertTrue(!DocumentVersion(contentHash = "h").isEmpty)
        assertTrue(!DocumentVersion(repositoryRevision = 3L).isEmpty)
        assertTrue(!DocumentVersion(syncManifestRevision = 5L).isEmpty)
    }
}
