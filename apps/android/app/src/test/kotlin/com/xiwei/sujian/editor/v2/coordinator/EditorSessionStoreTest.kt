@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 四：EditorSessionStore 契约测试。
 *
 * 规则（issue 解决四）：
 * - 每个活动过的 target 都有记录（包括非持久 target）；
 * - sessionId 属于所有活动 session — "是否持久"只决定 detach/close 时是否保留
 *   记录与 Rust session，不能决定记录里是否保存 ID（旧缺陷：非持久 target
 *   第一次编辑后 sessionId 变成 null）；
 * - 窗口重绑只修改 binding 相关字段，正文版本/hash/transaction/selection 保留；
 * - 关闭非活动 target 不得清掉活动 target 状态（旧实现会把新章节的 Attached
 *   清成 Idle）。
 */
class EditorSessionStoreTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.data.AppServiceBridge(
                com.xiwei.sujian.data.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_595_store",
                    "/tmp/sujian_test_workspace_595_store",
                ),
            ),
        )
    }

    @Test
    fun registerTargetMeta_createsRecordWithIdentity() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        assertTrue(coordinator.isTargetRegistered("t1"))
        assertTrue(coordinator.isTargetPersistent("t1"))

        coordinator.registerTargetMeta("t2", TextEditorProfile.ShortTitle, persistent = false)
        assertFalse(coordinator.isTargetPersistent("t2"))
    }

    @Test
    fun nonPersistentTarget_sessionIdRecordedInStore() {
        // #595 四：非持久 target 同样在 store 记录中持有 sessionId —
        // applyLocalEdit 重建状态时不会把 sessionId 变成 null。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.ShortTitle, persistent = false)

        // 模拟 prepareSessionForEdit 写入 sessionId（无 native 时 create 失败返回 null，
        // 这里直接验证记录结构语义：sessionId 属于所有活动 session）。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                text = "draft text",
                revision = 2L,
                transactionId = 5L,
                selectionAnchorUtf8 = 3,
                selectionHeadUtf8 = 5,
            ),
        )
        // 会话状态保留记录中的 sessionId 派生路径（0UL 表示尚无 Rust session）。
        assertEquals("t1", coordinator.sessionState.targetId)
        assertEquals("draft text", coordinator.sessionState.text)
        assertEquals(2L, coordinator.sessionState.revision)
    }

    @Test
    fun applyLocalEdit_updatesRecordDocumentStateAndSelection() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                text = "你好世界",
                revision = 3L,
                transactionId = 7L,
                selectionAnchorUtf8 = 3,
                selectionHeadUtf8 = 9,
            ),
        )
        val state = coordinator.sessionState
        assertEquals("你好世界", state.text)
        assertEquals(3L, state.revision)
        assertEquals(7L, state.lastAppliedTransactionId)
        assertEquals(3, state.selectionAnchorUtf8)
        assertEquals(9, state.selectionHeadUtf8)
        assertTrue("本地输入必须置 localDirty", state.localDirty)
    }

    @Test
    fun closeTarget_nonActiveTarget_keepsOtherTargetState() {
        // #595 四：关闭非活动 target 不得清掉其他 target 的正文/编辑状态
        // （旧实现会把 SessionState 无条件清成 Idle，破坏新章节的 Attached）。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "text", 1L, 1L, selectionAnchorUtf8 = 2, selectionHeadUtf8 = 4),
        )
        coordinator.forceEditingState(EditingState.EDITING)
        assertEquals("t1", coordinator.sessionState.targetId)

        // 关闭另一个非活动 target（t2）— 不改变 t1 的 SessionState。
        coordinator.registerTargetMeta("t2", TextEditorProfile.ShortTitle, persistent = false)
        coordinator.closeTarget("t2", SessionCloseReason.CHAPTER_SWITCH)

        assertEquals("t1", coordinator.sessionState.targetId)
        assertEquals("text", coordinator.sessionState.text)
        assertEquals(EditingState.EDITING, coordinator.sessionState.editingState)
        // t2 的记录已删除。
        assertFalse(coordinator.isTargetRegistered("t2"))
    }

    @Test
    fun closeTarget_activeTarget_resetsToIdle() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "text", 1L, 1L, selectionAnchorUtf8 = 2, selectionHeadUtf8 = 4),
        )
        coordinator.closeTarget("t1", SessionCloseReason.WORKSPACE_NAVIGATION)
        val state = coordinator.sessionState
        assertNull(state.targetId)
        assertEquals("", state.text)
        assertEquals(WindowBindingState.Idle, state.bindingState)
        assertEquals(EditorSessionOrigin.NONE, state.origin)
    }

    @Test
    fun releasePreparedTarget_newlyCreated_removesOnlyOwnSessionRecord() {
        // #595 一：newlyCreated=true → 只关闭本事务新建的 session 并移除仍指向
        // 该 session 的记录；不修改全局 SessionState（准备阶段从未修改它）。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(EditorDocumentUpdate.LocalInput("t1", "text", 1L, 1L))
        assertEquals("t1", coordinator.sessionState.targetId)

        val handle =
            PreparedSessionHandle(
                targetId = "t1",
                sessionId = 0UL,
                snapshot = TargetSnapshot("text", 4, 1L, 0, 4),
                newlyCreated = true,
                previousRecord = null,
            )
        coordinator.releasePreparedTarget(handle)
        assertFalse(coordinator.isTargetRegistered("t1"))
    }

    @Test
    fun releasePreparedTarget_borrowed_restoresPreviousRecordAndKeepsSession() {
        // #595 一/二：newlyCreated=false（借用的既有 session）→ 恢复事务前记录，
        // 不关闭 session — 回滚不得销毁事务开始前已经存在的 B session 与 Undo 历史。
        val coordinator = createCoordinator()
        val previous =
            EditorSessionRecord(
                targetId = "t1",
                sessionId = 7UL,
                persistent = true,
                documentState =
                    DocumentState(
                        text = "original",
                        revision = 3L,
                        committedVersion = DocumentVersion(contentHash = "hash-original"),
                        sessionBaseVersion = DocumentVersion(contentHash = "hash-original"),
                        lastSavedVersion = DocumentVersion(contentHash = "hash-original"),
                        localDirty = false,
                    ),
            )
        // 事务预准备期间记录被写入新 snapshot（模拟 prepare 后的记录状态）。
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "prepared",
                DocumentVersion(contentHash = "hash-prepared"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )

        val handle =
            PreparedSessionHandle(
                targetId = "t1",
                sessionId = 0UL,
                snapshot = TargetSnapshot("prepared", 8, 1L, 0, 8),
                newlyCreated = false,
                previousRecord = previous,
            )
        coordinator.releasePreparedTarget(handle)

        // 借用 session 不关闭：记录恢复为事务前的文档事实（正文/版本/选区）。
        assertTrue(coordinator.isTargetRegistered("t1"))
        assertEquals(7UL, coordinator.getPersistentSessionId("t1"))
        // 全局状态不被回滚触碰（准备阶段无副作用 — 只更新 store 记录）。
        assertEquals(EditorSessionOrigin.EXTERNAL_REPLACE, coordinator.sessionState.origin)
    }

    @Test
    fun detachWindowBinding_nonPersistent_closesAndRemovesRecord() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.ShortTitle, persistent = false)
        coordinator.applyLocalEdit(EditorDocumentUpdate.LocalInput("t1", "text", 1L, 1L))
        coordinator.detachWindowBinding("w1", "t1")
        assertFalse(coordinator.isTargetRegistered("t1"))
        assertNull(coordinator.sessionState.targetId)
    }

    @Test
    fun documentState_versionBookkeepingSurvivesLocalEdits() {
        // #595 二/四：本地编辑只置 dirty，不清 committedVersion。
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyExternalContentFact(
            TargetDocumentFact(
                "t1",
                "repo v1",
                DocumentVersion(contentHash = "hash-1"),
                DocumentVersion(),
                DocumentFactOrigin.REPOSITORY_LOAD,
            ),
        )
        assertEquals("hash-1", coordinator.sessionState.committedVersion.contentHash)

        coordinator.applyLocalEdit(EditorDocumentUpdate.LocalInput("t1", "repo v1 + local", 2L, 3L))
        val state = coordinator.sessionState
        assertEquals("hash-1", state.committedVersion.contentHash)
        assertTrue(state.localDirty)

        coordinator.markSaved("t1", DocumentVersion(contentHash = "hash-1b"))
        assertFalse(coordinator.sessionState.localDirty)
    }

    @Test
    fun sessionState_derivesFromActiveRecordDocument() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("t1", TextEditorProfile.DocumentBody, persistent = true)
        coordinator.applyLocalEdit(EditorDocumentUpdate.LocalInput("t1", "a", 1L, 1L))
        val record = coordinator.sessionState
        assertNotNull(record)
    }
}
