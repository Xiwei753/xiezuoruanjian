package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditingState

/**
 * #644 评论 5462826712 第1节：窗口绑定状态转换 — 从 [EditorSessionCoordinator] 抽取。
 *
 * Compose Surface 附着动作：告诉窗口层"这个 surface 真正有 layout 了"，
 * 窗口层用自己的 windowId 完成 binding；UI 不知道 sessionId，也不直接写状态机。
 */
fun EditorSessionCoordinator.attachSurface(
    windowId: String,
    targetId: String,
): EditorInputLease? =
    mutateSession {
        val current = sessionState.bindingState
        val currentSessionId = sessionState.sessionId

        // 已经是同一个 Attached — 幂等返回当前 lease
        if (current is WindowBindingState.Attached &&
            current.windowId == windowId &&
            current.targetId == targetId
        ) {
            val targetIdForLease = sessionState.activeTargetId ?: return@mutateSession null
            return@mutateSession EditorInputLease(
                targetId = targetIdForLease,
                sessionId = currentSessionId ?: 0UL,
                epoch = leaseEpoch,
            )
        }

        // 必须是 Attaching 且 windowId/targetId/sessionId 匹配
        if (current !is WindowBindingState.Attaching ||
            current.windowId != windowId ||
            current.targetId != targetId
        ) {
            return@mutateSession null
        }

        val sessionId = current.sessionId
        // 推进到 Attached
        sessionState =
            sessionState.copy(
                bindingState = WindowBindingState.Attached(windowId, targetId, sessionId),
                editingState = EditingState.EDITING,
            )

        EditorInputLease(
            targetId = targetId,
            sessionId = sessionId,
            epoch = leaseEpoch,
        )
    }
