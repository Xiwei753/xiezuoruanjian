package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.feature.editor.window.EditableTextTarget
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #624 评论11 第1项：会话层 dirty 规则 — dirty 不能由「这次没改正文」清掉。
 *
 * reloadFromKernel 改发 contentChanged=false 的 LOAD 事件（REPLACE operationKind）
 * 后，旧 `localInputDirty` 的 SELECTION 特判会让 REPLACE+contentChanged=false
 * 把旧 localDirty=true 清成 false。修正后 dirty 只由保存成功/外部事实提交清掉：
 * `previous.localDirty || contentChanged`。
 */
class EditorSessionReloadDirtyTest {
    private fun createCoordinator(): EditorSessionCoordinator {
        return EditorSessionCoordinator(
            AppServiceBridge(
                WriterAppServiceHolder(
                    "/tmp/sujian_test_workspace_624_reload_dirty",
                    "/tmp/sujian_test_workspace_624_reload_dirty",
                ),
            ),
        )
    }

    private fun typingUpdate(
        targetId: String,
        lease: EditorInputLease,
        revision: Long,
        transactionId: Long,
    ): EditorDocumentUpdate.LocalInput =
        EditorDocumentUpdate.LocalInput(
            targetId = targetId,
            revision = revision,
            transactionId = transactionId,
            operationKind = EditorOperationKind.INSERT,
            contentChanged = true,
            contentDelta = EditorContentDelta(insertedChars = 1),
            lease = lease,
        )

    /** reload 事件：REPLACE + contentChanged=false（mirror 重新对齐，正文语义未变）。 */
    private fun reloadUpdate(
        targetId: String,
        lease: EditorInputLease,
        revision: Long,
    ): EditorDocumentUpdate.LocalInput =
        EditorDocumentUpdate.LocalInput(
            targetId = targetId,
            revision = revision,
            transactionId = 20L,
            operationKind = EditorOperationKind.REPLACE,
            contentChanged = false,
            contentDelta = EditorContentDelta(),
            lease = lease,
        )

    /**
     * 用户输入（dirty=true）后触发 reload：reload 只是 mirror 重新对齐，
     * 未保存正文仍在 session 里 — localDirty 必须保留，绝不能清成 false
     * （否则同步/保存屏障会误判磁盘与屏幕一致，丢弃未落盘正文）。
     */
    @Test
    fun reloadReplaceWithoutContentChange_keepsPreviousDirty() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val lease = EditorInputLease("t1", 0UL, 0L)

        coordinator.applyLocalEdit(typingUpdate("t1", lease, revision = 5L, transactionId = 11L))
        assertTrue("前置：输入必须置 dirty", coordinator.sessionState.localDirty)

        // reload 事件（REPLACE + contentChanged=false）不得清掉未保存 dirty。
        coordinator.applyLocalEdit(reloadUpdate("t1", lease, revision = 6L))

        assertTrue(
            "#624 评论11 第1项：reload 不得把 localDirty 清成 false — " +
                "dirty 只能由保存成功/外部事实提交清掉",
            coordinator.sessionState.localDirty,
        )
        assertEquals("reload 仍推进 revision", 6L, coordinator.sessionState.revision)
    }

    /** 从未编辑的干净会话收到 reload 事件 — 保持干净（不伪造 dirty）。 */
    @Test
    fun reloadReplaceWithoutContentChange_fromCleanStaysClean() {
        val coordinator = createCoordinator()
        coordinator.registerTarget(EditableTextTarget("t1", isPersistent = true))
        val lease = EditorInputLease("t1", 0UL, 0L)

        coordinator.applyLocalEdit(reloadUpdate("t1", lease, revision = 3L))

        assertFalse("干净会话 reload 后必须仍干净", coordinator.sessionState.localDirty)
        assertEquals(3L, coordinator.sessionState.revision)
    }
}
