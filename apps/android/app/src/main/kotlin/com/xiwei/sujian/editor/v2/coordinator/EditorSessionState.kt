package com.xiwei.sujian.editor.v2.coordinator

import androidx.compose.runtime.Immutable

/**
 * #595 一：会话层唯一可观察状态 — 替代 targetTexts 并行缓存与 generation/hashCode 猜测。
 *
 * 由 [EditorSessionCoordinator] 维护，通过 [StateFlow] 暴露给 Compose 消费者。
 * 包含当前正文、revision、选区、最后应用的事务 ID 和来源类型。
 *
 * WritingPane 收集该状态：本地输入时 revision 已在 SessionState 中更新，
 * UI 回显只更新保存状态，不触发 resetPersistentSession。
 */
@Immutable
data class EditorSessionState(
    val targetId: String? = null,
    val sessionId: ULong? = null,
    val text: String = "",
    val revision: Long = 0L,
    val selectionAnchorUtf8: Int = 0,
    val selectionHeadUtf8: Int = 0,
    val lastAppliedTransactionId: Long = 0L,
    val origin: EditorSessionOrigin = EditorSessionOrigin.NONE,
    val bindingState: WindowBindingState = WindowBindingState.Idle,
    /** #595 一：最后应用的 Repository 正文 hash — 幂等去重 RepositoryLoaded 事件。 */
    val lastRepositoryHash: String = "",
    /** #595 三：编辑状态 — 从唯一 SessionState 派生，不再独立可写。 */
    val editingState: EditingState = EditingState.IDLE,
    /** #595 三：活动目标 ID — 从唯一 SessionState 派生，不再独立可写。 */
    val activeTargetId: String? = null,
)

/**
 * 会话内容来源 — 区分本地输入和外部替换。
 */
@Immutable
enum class EditorSessionOrigin {
    NONE,
    LOCAL_INPUT,
    EXTERNAL_REPLACE,
    INITIAL_LOAD,
}
