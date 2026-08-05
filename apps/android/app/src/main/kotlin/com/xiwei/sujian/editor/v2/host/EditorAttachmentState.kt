package com.xiwei.sujian.editor.v2.host

import com.xiwei.sujian.editor.v2.coordinator.ProjectionSnapshot
import com.xiwei.sujian.editor.v2.coordinator.TargetSnapshot
import com.xiwei.sujian.editor.v2.coordinator.WindowBindingState

/**
 * #595 六：窗口附着 sealed 状态机 — 区分临时失焦、窗口解绑和业务关闭。
 *
 * - [Idle]：无窗口绑定、无活动会话。
 * - [Attaching]：窗口正在绑定（beginEdit 进行中）。
 * - [Attached]：窗口已绑定，输入法/渲染活跃。
 * - [Paused]：窗口临时失焦（IME 切换、系统浮层、权限弹窗、导航转场）—
 *   暂停并保存当前可见帧，不永久取消事务；窗口重新获得焦点时从保存帧继续。
 * - [Detached]：窗口已解绑（配置变化、onDispose），Rust session 与 snapshot 保留，
 *   等待新窗口附着。
 * - [Releasing]：Activity 永久结束，正在释放全部资源。
 *
 * 窗口事件和业务事件分开：
 * - Compose onDispose / 配置变化 → detachWindow(windowId)
 * - 返回章节列表、切换章节、删除章节 → closeTarget(targetId, reason)
 * - ViewModel onCleared → releaseAllSessions()
 */
sealed interface EditorAttachmentState {
    data object Idle : EditorAttachmentState
    data class Attaching(
        val windowId: String,
        val targetId: String,
        val sessionId: ULong,
    ) : EditorAttachmentState
    data class Attached(
        val windowId: String,
        val targetId: String,
        val sessionId: ULong,
    ) : EditorAttachmentState
    data class Paused(
        val targetId: String,
        val sessionId: ULong,
        val frameSnapshot: EditorFrameSnapshot,
    ) : EditorAttachmentState
    data class Detached(
        val targetId: String,
        val sessionId: ULong,
        val sessionSnapshot: TargetSnapshot?,
        val projectionSnapshot: ProjectionSnapshot? = null,
    ) : EditorAttachmentState
    data object Releasing : EditorAttachmentState
}

/**
 * #595 六：暂停时保存的可见帧纯数据 — 不持有 View、Canvas 或 Bitmap。
 * 窗口重新获得焦点时从该帧继续或稳定落到事务终态。
 */
data class EditorFrameSnapshot(
    val scrollX: Float,
    val scrollY: Float,
    val viewportWidth: Int,
    val viewportHeight: Int,
    val hasActiveAnimation: Boolean,
)


/**
 * #595 六：从规范会话级 [WindowBindingState] 派生窗口附着状态 — 纯函数，不引入并行状态机。
 *
 * [WindowBindingState] 是唯一规范绑定状态机（由 [com.xiwei.sujian.editor.v2.coordinator.EditorSessionCoordinator] 维护）；
 * 本函数把它投影为窗口附着语义，并叠加窗口级 [paused] 标志（来自 View 焦点变化）。
 *
 * - [WindowBindingState.Attached] + paused=true → [EditorAttachmentState.Paused]
 * - [WindowBindingState.Committing]/[Cancelling] 仍视为已附着（事务进行中）
 * - [WindowBindingState.Detaching] 视为已解绑（瞬态，snapshot 已捕获）
 */
fun attachmentStateFromBinding(
    state: WindowBindingState,
    paused: Boolean,
    frameSnapshot: EditorFrameSnapshot?,
    projectionSnapshot: ProjectionSnapshot?,
): EditorAttachmentState = when (state) {
    WindowBindingState.Idle -> EditorAttachmentState.Idle
    is WindowBindingState.Attaching -> EditorAttachmentState.Attaching(state.windowId, state.targetId, state.sessionId)
    is WindowBindingState.Attached -> if (paused && frameSnapshot != null) {
        EditorAttachmentState.Paused(state.targetId, state.sessionId, frameSnapshot)
    } else {
        EditorAttachmentState.Attached(state.windowId, state.targetId, state.sessionId)
    }
    is WindowBindingState.Detaching -> EditorAttachmentState.Detached(
        targetId = "",
        sessionId = 0UL,
        sessionSnapshot = state.snapshot,
        projectionSnapshot = projectionSnapshot,
    )
    is WindowBindingState.Detached -> EditorAttachmentState.Detached(
        state.targetId, state.sessionId, state.snapshot, projectionSnapshot,
    )
    is WindowBindingState.Committing -> EditorAttachmentState.Attached("", state.targetId, state.sessionId)
    is WindowBindingState.Cancelling -> EditorAttachmentState.Attached("", state.targetId, state.sessionId)
}
