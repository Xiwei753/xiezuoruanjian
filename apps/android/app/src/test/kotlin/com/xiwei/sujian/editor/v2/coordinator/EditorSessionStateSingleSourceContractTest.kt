package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 四：EditorSessionState 唯一状态源契约测试。
 *
 * 旧缺陷：会话层同时保留 EditorSessionState / activeTargetIdFlow /
 * editingStateFlow / windowBindingStateFlow / targetTexts / persistentSessionIds，
 * 且状态更新不完整：
 * - detachWindowBinding 不改 EditorSessionState.bindingState；
 * - closeTarget 不清 SessionState；
 * - applyLocalEdit 沿用旧 selection（不携带真实选区）；
 * - commitActiveSession 清除 active target 后 SessionState 仍保留旧 target/binding。
 *
 * 修复：EditorSessionState 是唯一正文/选区/revision/binding 事实源，
 * 所有生命周期事件同步更新它；targetTexts 并行缓存已删除。
 */
class EditorSessionStateSingleSourceContractTest {

    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(com.xiwei.sujian.data.AppServiceBridge(
            com.xiwei.sujian.data.WriterAppServiceHolder("/tmp/sujian_test_workspace_595")
        ))
    }

    @Test
    fun applyLocalEdit_carriesRealSelectionIntoSessionState() {
        val coordinator = createCoordinator()
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                text = "你好世界",
                revision = 3L,
                transactionId = 7L,
                selectionAnchorUtf8 = 3,
                selectionHeadUtf8 = 9,
            )
        )
        val state = coordinator.sessionState
        assertEquals("t1", state.targetId)
        assertEquals("你好世界", state.text)
        assertEquals(3L, state.revision)
        assertEquals(7L, state.lastAppliedTransactionId)
        assertEquals(3, state.selectionAnchorUtf8)
        assertEquals(9, state.selectionHeadUtf8)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, state.origin)
    }

    @Test
    fun detachWindowBinding_syncsSessionStateBindingState() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = false))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "text", 1L, 1L, selectionAnchorUtf8 = 2, selectionHeadUtf8 = 4)
        )
        coordinator.detachWindowBinding("w1", "t1")
        assertEquals(
            "detachWindowBinding must sync EditorSessionState.bindingState",
            WindowBindingState.Idle,
            coordinator.sessionState.bindingState,
        )
        assertNull(coordinator.sessionState.targetId)
    }

    @Test
    fun closeTarget_resetsSessionStateToIdle() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput("t1", "text", 1L, 1L, selectionAnchorUtf8 = 2, selectionHeadUtf8 = 4)
        )
        coordinator.closeTarget("t1", SessionCloseReason.WORKSPACE_NAVIGATION)
        val state = coordinator.sessionState
        assertNull("closeTarget must clear session target", state.targetId)
        assertNull("closeTarget must clear session id", state.sessionId)
        assertEquals("", state.text)
        assertEquals(0L, state.revision)
        assertEquals(WindowBindingState.Idle, state.bindingState)
        assertEquals(EditorSessionOrigin.NONE, state.origin)
    }

    @Test
    fun repositoryLoaded_recordsRealHashAndIsIdempotent() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))

        val first = EditorDocumentUpdate.RepositoryLoaded("t1", "repo text v1", fileHash = "hash-1", revision = 0L)
        assertTrue("New repository version must apply", coordinator.shouldApplyRepositoryLoad(first))
        coordinator.applyRepositoryLoaded(first)

        var state = coordinator.sessionState
        assertEquals("repo text v1", state.text)
        assertEquals("hash-1", state.lastRepositoryHash)

        // 同一 hash + 同一内容重放：幂等，不 reset。
        assertFalse("Same hash+content replay must be idempotent", coordinator.shouldApplyRepositoryLoad(first))

        // 新 hash + 新内容：应用。
        val second = EditorDocumentUpdate.RepositoryLoaded("t1", "repo text v2", fileHash = "hash-2", revision = 0L)
        assertTrue("New repository hash must apply", coordinator.shouldApplyRepositoryLoad(second))
        coordinator.applyRepositoryLoaded(second)
        state = coordinator.sessionState
        assertEquals("repo text v2", state.text)
        assertEquals("hash-2", state.lastRepositoryHash)
        assertEquals(EditorSessionOrigin.EXTERNAL_REPLACE, state.origin)
    }

    @Test
    fun repositoryLoaded_emptyHashIsRejected() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val noHash = EditorDocumentUpdate.RepositoryLoaded("t1", "x", fileHash = "", revision = 0L)
        assertFalse("Empty fileHash must be rejected", coordinator.shouldApplyRepositoryLoad(noHash))
    }

    @Test
    fun sessionState_hasNoParallelTargetTextsCache() {
        // targetTexts 并行正文缓存必须已删除：会话层正文唯一来源是 sessionState。
        val field = EditorSessionCoordinator::class.java.declaredFields.firstOrNull {
            it.name == "targetTexts"
        }
        assertNull(
            "targetTexts parallel text cache must be removed (#595 四)",
            field,
        )
    }
}
