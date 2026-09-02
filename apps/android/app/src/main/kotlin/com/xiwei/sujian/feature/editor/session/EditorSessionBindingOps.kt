package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditingState

private fun WindowBindingState.Attached.isSameAttachment(
    windowId: String,
    targetId: String,
    currentSessionId: ULong?,
): Boolean =
    this.windowId == windowId &&
        this.targetId == targetId &&
        currentSessionId != null &&
        this.sessionId == currentSessionId

private fun isNotAttachingOrMismatch(
    current: WindowBindingState,
    windowId: String,
    targetId: String,
    currentSessionId: ULong?,
): Boolean =
    current !is WindowBindingState.Attaching ||
        current.windowId != windowId ||
        current.targetId != targetId ||
        currentSessionId == null ||
        current.sessionId != currentSessionId ||
        current.sessionId == 0UL

/**
 * #644 评论 5462826712 第1节：窗口绑定状态转换 — 从 [EditorSessionCoordinator] 抽取。
 *
 * Compose Surface 附着动作：告诉窗口层"这个 surface 真正有 layout 了"，
 * 窗口层用自己的 windowId 完成 binding；UI 不知道 sessionId，也不直接写状态机。
 *
 * #644 评论 5467821839 第4节：window / target / session 任一不匹配都返回 null。
 */
fun EditorSessionCoordinator.attachSurface(
    windowId: String,
    targetId: String,
): EditorInputLease? =
    mutateSession {
        val current = sessionState.bindingState
        val currentSessionId = sessionState.sessionId

        // 已经是同一个 Attached — 幂等返回当前 lease（也校验 sessionId 一致）
        if (current is WindowBindingState.Attached &&
            current.isSameAttachment(windowId, targetId, currentSessionId)
        ) {
            return@mutateSession EditorInputLease(
                targetId = targetId,
                sessionId = currentSessionId!!,
                epoch = leaseEpoch,
            )
        }

        // 必须是 Attaching 且 windowId/targetId/sessionId 全部匹配
        if (isNotAttachingOrMismatch(current, windowId, targetId, currentSessionId)) {
            return@mutateSession null
        }

        // 此时 current 一定是 Attaching（isNotAttachingOrMismatch 返回 false）
        val attaching = current as? WindowBindingState.Attaching ?: return@mutateSession null
        val sessionId = attaching.sessionId
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
