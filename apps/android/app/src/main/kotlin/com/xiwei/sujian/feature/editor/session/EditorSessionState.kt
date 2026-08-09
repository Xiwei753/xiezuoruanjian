package com.xiwei.sujian.feature.editor.session

import androidx.compose.runtime.Immutable
import com.xiwei.sujian.feature.editor.window.EditingState

/**
 * #595 一/二：会话层唯一可观察状态。
 *
 * 由 [EditorSessionCoordinator] 维护，通过 [StateFlow] 暴露给 Compose 消费者。
 * 包含当前正文、revision、选区、最后应用的事务 ID、来源类型和文档版本事实。
 *
 * WritingPane 收集该状态：本地输入时 revision 已在 SessionState 中更新，
 * UI 回显只更新保存状态，不触发 resetPersistentSession。
 *
 * #595 二：lastRepositoryHash/lastAppliedContentVersion（进程内事件序号）已删除，
 * 版本事实由 [committedVersion]/[sessionBaseVersion]/[localDirty] 表达 —
 * 锚点是 Repository/Core 的真实文档版本（contentHash + 同步 manifest）。
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
    /** #595 三：编辑状态 — 从唯一 SessionState 派生，不再独立可写。 */
    val editingState: EditingState = EditingState.IDLE,
    /** #595 三：活动目标 ID — 从唯一 SessionState 派生，不再独立可写。 */
    val activeTargetId: String? = null,
    /** #595 二：最后应用的文档版本 — 同 sourceVersion 重放被幂等忽略。 */
    val committedVersion: DocumentVersion = DocumentVersion(),
    /** #595 二：Rust session 创建/重置时基于的文档版本 — 外部事件据此判断是否基于旧 base。 */
    val sessionBaseVersion: DocumentVersion = DocumentVersion(),
    /** #595 二：存在尚未落盘的本地编辑 — true 时外部版本禁止直接 reset（冲突路径）。 */
    val localDirty: Boolean = false,
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

    /** #595 二：同步合并后外部替换。 */
    SYNC_MERGED,

    /** #595 二：撤销/恢复后正文变更。 */
    UNDO_RESTORED,

    /** #595 二：程序化批量替换后正文变更。 */
    PROGRAMMATIC_REPLACE,
}
