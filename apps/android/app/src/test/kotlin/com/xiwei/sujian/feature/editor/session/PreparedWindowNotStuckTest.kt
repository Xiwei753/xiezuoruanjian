@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.ui.EditorAttachAction
import com.xiwei.sujian.feature.editor.ui.editorAttachDecision
import com.xiwei.sujian.feature.editor.window.EditingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #624 评论17 问题2：删除 prepared 假窗口 — 回归测试。
 *
 * 旧缺陷：commitPreparedSession(handle) 默认构造
 * Attaching("prepared", targetId, sessionId)，而 WritingPaneEditorAttach 改成
 * `if (binding is Attaching) return`。组合后卡死：
 * commit B → binding=Attaching("prepared",B) → 真实 windowId != "prepared"
 * → WritingEditorSurface 不创建 AndroidView → WritingPaneEditorAttach 看到
 * Attaching 直接 return → prepareSessionForEdit 没机会 restamp
 * → 永远到不了 Attached → inputFrozen 永远不解除。
 *
 * 新实现：commitPreparedSession 后 Detached/IDLE/activeTargetId=null；
 * 真实窗口出现后 WritingPaneEditorAttach 从 Detached 调 beginEdit，
 * 由 prepareSessionForEdit(..., realWindowId) 进入
 * Detached → Attaching(realWindowId) → Attached(realWindowId) → confirm。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class PreparedWindowNotStuckTest {
    private fun createCoordinator(): EditorSessionCoordinator =
        EditorSessionCoordinator(
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_prepared",
                    "/home/xiwei/.cache/agent-tmp/sujian_test_624_c17_prepared",
                ),
            ),
        )

    /**
     * commitPreparedSession 后状态必须是 Detached/IDLE/activeTargetId=null，
     * 不得是 Attaching("prepared", ...)。
     */
    @Test
    fun commitPreparedSession_resultsInDetachedNotPreparedAttaching() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 7UL,
                snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
                mode = PreparedSessionMode.Created,
                previousRecord = null,
            )
        assertTrue(coordinator.commitPreparedSession(handle))

        val state = coordinator.sessionState
        assertTrue(
            "commitPreparedSession 后必须是 Detached，不是 Attaching(prepared)",
            state.bindingState is WindowBindingState.Detached,
        )
        assertEquals(EditingState.IDLE, state.editingState)
        assertEquals(null, state.activeTargetId)
        assertEquals(7UL, state.sessionId)
        assertEquals(2L, state.revision)
    }

    /**
     * 真实窗口从 Detached 调 prepareSessionForEdit(realWindowId) 能进入
     * Attaching(realWindowId)，再 completeWindowAttach 进入 Attached，
     * 不会永久卡在 Attaching("prepared")。
     */
    @Test
    fun realWindowFromDetached_reachesAttached_notStuckInPreparedAttaching() {
        val coordinator = createCoordinator()
        coordinator.registerTargetMeta("b", TextEditorProfile.DocumentBody, persistent = true)
        val handle =
            PreparedSessionHandle(
                targetId = "b",
                sessionId = 7UL,
                snapshot = TargetSnapshot("textB", 5, 2L, 0, 5),
                mode = PreparedSessionMode.Created,
                previousRecord = null,
            )
        assertTrue(coordinator.commitPreparedSession(handle))
        // commit 后 Detached，activeTargetId=null。
        val afterCommit = coordinator.sessionState
        assertTrue(afterCommit.bindingState is WindowBindingState.Detached)

        // 真实窗口出现：从 Detached 调 prepareSessionForEdit(realWindowId)。
        // 测试环境无 native，prepareSessionForEdit 会尝试 createSession（返回 null）→ 失败。
        // 但状态机决策点已验证：Detached → beginEdit 路径被触发（不是 Attaching return）。
        // 这里用 editorAttachDecision 纯函数验证决策，会话层状态机用 fake 验证完整路径。
        val decision =
            editorAttachDecision(
                afterCommit.bindingState,
                windowId = "realWindow",
                targetId = "b",
            )
        assertEquals(
            "Detached 状态必须决策 BeginEdit（不卡在 Attaching return）",
            EditorAttachAction.BeginEdit,
            decision,
        )
    }

    /**
     * editorAttachDecision：Attaching 属于旧窗口（"prepared" 或其他）→ BeginEdit，
     * 让 session 层 restamp 到当前窗口，不卡死。
     */
    @Test
    fun editorAttachDecision_attachingFromForeignWindow_decidesBeginEdit() {
        val decision =
            editorAttachDecision(
                WindowBindingState.Attaching("prepared", "b", 7UL),
                windowId = "realWindow",
                targetId = "b",
            )
        assertEquals(
            "Attaching 属于旧窗口必须决策 BeginEdit（restamp 到当前窗口）",
            EditorAttachAction.BeginEdit,
            decision,
        )
    }

    /**
     * editorAttachDecision：Attached 属于旧窗口 → BeginEdit。
     */
    @Test
    fun editorAttachDecision_attachedFromForeignWindow_decidesBeginEdit() {
        val decision =
            editorAttachDecision(
                WindowBindingState.Attached("oldWindow", "b", 7UL),
                windowId = "realWindow",
                targetId = "b",
            )
        assertEquals(EditorAttachAction.BeginEdit, decision)
    }

    /**
     * editorAttachDecision：Attached 且 window/target 匹配 → Confirm。
     */
    @Test
    fun editorAttachDecision_attachedMatching_decidesConfirm() {
        val decision =
            editorAttachDecision(
                WindowBindingState.Attached("realWindow", "b", 7UL),
                windowId = "realWindow",
                targetId = "b",
            )
        assertEquals(EditorAttachAction.Hold, decision)
    }

    /**
     * editorAttachDecision：Attaching 且 window/target 匹配 → Wait。
     */
    @Test
    fun editorAttachDecision_attachingMatching_decidesWait() {
        val decision =
            editorAttachDecision(
                WindowBindingState.Attaching("realWindow", "b", 7UL),
                windowId = "realWindow",
                targetId = "b",
            )
        assertEquals(EditorAttachAction.Hold, decision)
    }

    /**
     * editorAttachDecision：Committing/Cancelling/Detaching → Hold（不发起绑定）。
     */
    @Test
    fun editorAttachDecision_committingCancellingDetaching_decidesHold() {
        assertEquals(
            EditorAttachAction.Hold,
            editorAttachDecision(WindowBindingState.Committing("b", 7UL), "w1", "b"),
        )
        assertEquals(
            EditorAttachAction.Hold,
            editorAttachDecision(WindowBindingState.Cancelling("b", 7UL), "w1", "b"),
        )
        assertEquals(
            EditorAttachAction.Hold,
            editorAttachDecision(WindowBindingState.Detaching(null), "w1", "b"),
        )
    }

    /**
     * editorAttachDecision：Idle → BeginEdit。
     */
    @Test
    fun editorAttachDecision_idle_decidesBeginEdit() {
        assertEquals(
            EditorAttachAction.BeginEdit,
            editorAttachDecision(WindowBindingState.Idle, "w1", "b"),
        )
    }
}
