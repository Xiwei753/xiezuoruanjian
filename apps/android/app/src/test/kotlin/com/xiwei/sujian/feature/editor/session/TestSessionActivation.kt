package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditingState

/**
 * #624 评论17 问题2：测试辅助 — commitPreparedSession 后 target 进入 Detached，
 * 真实窗口绑定需要 prepareSessionForEdit（无 native 时失败）。本辅助直接设置
 * Attached 状态，让 lease/edit 测试能建立活动 session 而不依赖 native。
 *
 * 生产代码不使用本函数 — 真实窗口走 prepareSessionForEdit + completeWindowAttach。
 */
internal fun EditorSessionCoordinator.activateAttachedForTest(
    targetId: String,
    windowId: String = "w1",
) {
    val sid = store.record(targetId)?.sessionId ?: return
    updateSessionState {
        it.copy(
            targetId = targetId,
            sessionId = sid,
            activeTargetId = targetId,
            bindingState = WindowBindingState.Attached(windowId, targetId, sid),
            editingState = EditingState.EDITING,
        )
    }
}
