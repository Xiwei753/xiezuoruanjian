package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.feature.editor.session.WindowBindingState
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #624 评论17 问题2：editorAttachDecision 纯函数决策测试 —
 * 覆盖所有 WindowBindingState 分支，验证旧窗口 restamp 和中间状态不冻结。
 *
 * 删除 prepared 假窗口后，绑定状态机为
 * Detached -> Attaching(realWindowId) -> Attached(realWindowId)。
 * - Attached 且 windowId/targetId 都匹配 -> Confirm；
 * - Attaching 且 windowId/targetId 都匹配 -> Wait；
 * - Attaching/Attached 属于旧 window -> BeginEdit（restamp 到新窗口）；
 * - Idle/Detached -> BeginEdit；
 * - Committing/Cancelling/Detaching -> Hold（等待当前事务结束，不冻结）。
 */
class EditorAttachDecisionTest {
    @Test
    fun attached_matchingWindow_returnsConfirm() {
        val state = WindowBindingState.Attached("w1", "t1", 5UL)
        assertEquals(
            EditorAttachAction.Hold,
            editorAttachDecision(state, "w1", "t1"),
        )
    }

    @Test
    fun attached_oldWindow_returnsBeginEditForRestamp() {
        val state = WindowBindingState.Attached("oldWindow", "t1", 5UL)
        assertEquals(
            "旧窗口 Attached 必须返回 BeginEdit 以 restamp 到新窗口",
            EditorAttachAction.BeginEdit,
            editorAttachDecision(state, "w1", "t1"),
        )
    }

    @Test
    fun attached_differentTarget_returnsBeginEdit() {
        val state = WindowBindingState.Attached("w1", "oldTarget", 5UL)
        assertEquals(
            EditorAttachAction.BeginEdit,
            editorAttachDecision(state, "w1", "t1"),
        )
    }

    @Test
    fun attaching_matchingWindow_returnsWait() {
        val state = WindowBindingState.Attaching("w1", "t1", 5UL)
        assertEquals(
            EditorAttachAction.Hold,
            editorAttachDecision(state, "w1", "t1"),
        )
    }

    @Test
    fun attaching_oldWindow_returnsBeginEditForRestamp() {
        val state = WindowBindingState.Attaching("oldWindow", "t1", 5UL)
        assertEquals(
            "旧窗口 Attaching 必须返回 BeginEdit 以 restamp 到新窗口",
            EditorAttachAction.BeginEdit,
            editorAttachDecision(state, "w1", "t1"),
        )
    }

    @Test
    fun detached_returnsBeginEdit() {
        val state = WindowBindingState.Detached("t1", 5UL, null)
        assertEquals(
            EditorAttachAction.BeginEdit,
            editorAttachDecision(state, "w1", "t1"),
        )
    }

    @Test
    fun idle_returnsBeginEdit() {
        assertEquals(
            EditorAttachAction.BeginEdit,
            editorAttachDecision(WindowBindingState.Idle, "w1", "t1"),
        )
    }

    @Test
    fun committing_returnsHold_notFrozen() {
        val state = WindowBindingState.Committing("t1", 5UL)
        assertEquals(
            "Committing 中间状态必须返回 Hold（不冻结，等待事务结束）",
            EditorAttachAction.Hold,
            editorAttachDecision(state, "w1", "t1"),
        )
    }

    @Test
    fun cancelling_returnsHold_notFrozen() {
        val state = WindowBindingState.Cancelling("t1", 5UL)
        assertEquals(
            EditorAttachAction.Hold,
            editorAttachDecision(state, "w1", "t1"),
        )
    }

    @Test
    fun detaching_returnsHold_notFrozen() {
        val state = WindowBindingState.Detaching(null)
        assertEquals(
            EditorAttachAction.Hold,
            editorAttachDecision(state, "w1", "t1"),
        )
    }
}
