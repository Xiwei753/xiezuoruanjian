package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #624 评论9：本地输入回灌不触发 session reset 的会话层契约测试（重写）。
 *
 * 上轮机制的 `sessionState.origin == LOCAL_INPUT && sessionState.text == uiState.content`
 * 已随 SessionState.text 镜像删除。新机制：
 * - 本地输入经 [EditorSessionEditOps.applyLocalUpdate] 推进 revision/transactionId，
 *   origin 置 LOCAL_INPUT，WritingPane 不再做全文 String 比较；
 * - 连续输入第二次同样满足（revision 单调推进，不依赖字符串比较）；
 * - 外部内容是否应用由 [shouldApplyExternalContent] 用版本/dirty 判定 +
 *   冷路径 snapshot.text 低频比较决定。
 */
class LocalInputOriginTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_624_local_input_origin",
                    "/tmp/sujian_test_workspace_624_local_input_origin",
                ),
            ),
        )
    }

    @Test
    fun localInputAdvancesRevisionAndKeepsLocalOrigin() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                revision = 5L,
                transactionId = 11L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = 11),
                lease = EditorInputLease("t1", 0UL, 0L),
            ),
        )
        val state = coordinator.sessionState
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, state.origin)
        assertEquals(5L, state.revision)
        assertEquals(11L, state.lastAppliedTransactionId)
        assertTrue("本地输入必须置 localDirty", state.localDirty)
    }

    @Test
    fun consecutiveLocalInputsDoNotFalselyTriggerReset() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val lease = EditorInputLease("t1", 0UL, 0L)
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                revision = 5L,
                transactionId = 11L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = 1),
                lease = lease,
            ),
        )
        assertEquals(5L, coordinator.sessionState.revision)

        // 连续第二次输入：revision 继续推进，不依赖任何字符串比较。
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                revision = 6L,
                transactionId = 12L,
                operationKind = EditorOperationKind.INSERT,
                contentChanged = true,
                contentDelta = EditorContentDelta(insertedChars = 1),
                lease = lease,
            ),
        )
        assertEquals(6L, coordinator.sessionState.revision)
        assertEquals(12L, coordinator.sessionState.lastAppliedTransactionId)
        assertEquals(EditorSessionOrigin.LOCAL_INPUT, coordinator.sessionState.origin)
    }

    @Test
    fun selectionOnlyEditKeepsLocalOriginAndDoesNotMarkDirty() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        coordinator.applyLocalEdit(
            EditorDocumentUpdate.LocalInput(
                targetId = "t1",
                revision = 5L,
                transactionId = 13L,
                operationKind = EditorOperationKind.SELECTION,
                contentChanged = false,
                contentDelta = EditorContentDelta(),
                selectionAnchorUtf8 = 2,
                selectionHeadUtf8 = 4,
                lease = EditorInputLease("t1", 0UL, 0L),
            ),
        )
        val state = coordinator.sessionState
        assertEquals(5L, state.revision)
        assertEquals(2, state.selectionAnchorUtf8)
        assertEquals(4, state.selectionHeadUtf8)
        assertFalse("selection-only 不改变正文 → 不置 dirty", state.localDirty)
    }
}
