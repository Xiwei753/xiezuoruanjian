package com.xiwei.sujian.feature.editor.session

// ! # 编辑器会话协调器类型声明（从 EditorSessionCoordinator �?分）

import androidx.compose.runtime.Immutable

sealed interface WindowBindingState {
    data object Idle : WindowBindingState

    data class Attaching(
        val windowId: String,
        val targetId: String,
        val sessionId: ULong,
    ) : WindowBindingState

    data class Attached(
        val windowId: String,
        val targetId: String,
        val sessionId: ULong,
    ) : WindowBindingState

    data class Detaching(val snapshot: TargetSnapshot?) : WindowBindingState

    data class Detached(
        val targetId: String,
        val sessionId: ULong,
        val snapshot: TargetSnapshot?,
    ) : WindowBindingState

    data class Committing(val targetId: String, val sessionId: ULong) : WindowBindingState

    data class Cancelling(val targetId: String, val sessionId: ULong) : WindowBindingState
}

/**
 * #592 三：业务级关闭原因 — 由 workspace 导航事件明确给出，不能从
 * DisposableEffect 推断业务对象是否结束。
 */
enum class SessionCloseReason {
/** 用户从正文返回章节列表/作品列表（workspace 导航离开 Editor 目的地）。 */
    WORKSPACE_NAVIGATION,

/** 章节切换（旧章节 session 关闭，新章节新建 session）。 */
    CHAPTER_SWITCH,

/** 章节/作品被删除。 */
    DELETE,
}

/**
 * #592 四：会话层持有的纯数据投影状态 — 不含 View、TextPaint、FrameClock、
 * Rect/Transform 或 Compose mutableState。窗口层在销毁/附着时读写。
 * #595 九：仅保留滚动位置（配置变化/返回重进时恢复 View 滚动），
 * 字体/主题/视口等视觉配置由 Compose 主题和 profile 权威提供，不再保存。
 */
data class ProjectionSnapshot(
    val scrollX: Float = 0f,
    val scrollY: Float = 0f,
)

/**
 * #595 二：输入 lease — 窗口绑定时由会话层签发，随每一次窗口层编辑回调提交。
 *
 * 章节切换事务提交（[commitPreparedSession]）、业务关闭（[closeTarget]）和
 * 窗口解绑（[detachWindowBinding]）都会使 [epoch] 失效；Coordinator 只接受
 * 仍匹配当前活动 target、session 和 epoch 的事件。旧 View 即使晚到一帧，
 * 也不能修改新章节的会话或 ViewModel。
 *
 * [sessionId] 为 0UL 表示会话层尚无 Rust session（纯状态测试/初始构造阶段），
 * 校验时与空 session 状态等价。
 */
@Immutable
data class EditorInputLease(
    val targetId: String,
    val sessionId: ULong,
    val epoch: Long,
)

/**
 * #595 二：文档操作租约 — 保存/同步开始时由会话层一次性签发，
 * 包含完整的不可变文档快照。操作期间据此校验当前活动文档是否仍是同一
 * target/session/epoch/revision；任一字段不匹配则操作中止，不拼接
 * ViewModel 字段与全局 SessionState。
 *
 * 与 [EditorInputLease] 的区别：EditorInputLease 是窗口级输入租约（每次
 * 编辑回调携带，epoch 在章节切换/关闭/解绑时递增）；DocumentOperationLease
 * 是文档级操作租约（保存/同步开始时签发，携带完整快照，操作期间冻结）。
 */
@Immutable
data class DocumentOperationLease(
    val operationId: Long,
    val targetId: String,
    val coreSessionId: ULong,
    val inputEpoch: Long,
    val rustRevision: Long,
    val text: String,
    val committedVersion: DocumentVersion,
    /**
     * #624 评论12 第2项：唯一 dirty 真值 — 从对应 target 的 store 记录
     * [DocumentState.localDirty] 填入。所有保存入口只消费 lease 的 localDirty
     * + text 决策（NoOp/Clear/Save），ViewModel 不再维护第二份 contentDirty。
     */
    val localDirty: Boolean = false,
)

/**
 * #595 一：无副作用章节预准备句柄 — 由 [EditorSessionCoordinator.prepareTargetSessionForCommit]
 * 返回，是章节切换事务在最终 requestId 校验前取得的唯一预准备产物。
 *
 * 准备阶段只允许：读取 B 的记录、验证或新建 B session、读取 snapshot；
 * 禁止 commit/cancel A、修改 activeTargetId/WindowBindingState/全局
 * EditorSessionState、关闭任何既有有效 session。提交与回滚分别由
 * [EditorSessionCoordinator.commitPreparedSession] 与
 * [EditorSessionCoordinator.releasePreparedTarget] 完成。
 *
 * - [newlyCreated]=true 且 [replacedSessionId]=null：本事务新建的临时 session
 *   （记录中无既有有效 session）— 回滚时关闭 candidate、移除记录；
 * - [newlyCreated]=true 且 [replacedSessionId]!=null：#624 评论15 问题2 candidate
 *   swap — 既有持久 session 的 snapshot 正文与 initialText 不一致且 localDirty=false
 *   时，prepare 创建一个装有 initialText 的 candidate session，[replacedSessionId]
 *   记录被替换的旧 session ID。commit 成功后由 [commitPreparedSession] 关闭旧 session；
 *   回滚（[releasePreparedTarget]）只关闭 candidate，恢复 [previousRecord]（旧 session
 *   原样保留，Undo/Redo 不丢）；
 * - [newlyCreated]=false：snapshot 正文与 initialText 一致 → 复用既有保留 session
 *   （含 Undo 历史）— 回滚时恢复 [previousRecord]，不关闭 session。
 */
@Immutable
data class PreparedSessionHandle(
    val targetId: String,
    val sessionId: ULong,
    val snapshot: TargetSnapshot,
    val newlyCreated: Boolean,
    val previousRecord: EditorSessionRecord?,
    /**
     * #624 评论15 问题2：candidate swap 时被替换的旧 session ID；null 表示纯新建
     * （[previousRecord]==null 或记录中无既有有效 session）或纯复用（[newlyCreated]==false）。
     */
    val replacedSessionId: ULong? = null,
)

/**
 * #595 二：外部文档事实的应用决策 — 替代旧 shouldApply* 布尔判断。
 *
 * - [Apply]：版本更新且无本地 dirty，可执行一次 Core reset；
 * - [IgnoreReplay]：同 sourceVersion 重放 — 幂等忽略；
 * - [IgnoreOlder]：外部 sourceVersion 可比较且旧于 committedVersion — 忽略；
 * - [IgnoreDirtyConflict]：存在本地未保存编辑 — 禁止直接 reset，
 *   必须走三方合并/冲突路径（发布类型化冲突，不覆盖用户输入）；
 * - [IgnoreSameContent]：正文与当前 session 一致 — 无需 reset；
 * - [IgnoreEmptyVersion]：事件未携带任何版本锚点 — 不可应用；
 * - [IgnoreUncomparableConflict]：两侧版本不可比较（无相同 revision 锚点、
 *   incoming 的父版本链不含 committed）且正文不同 — 不得默认 Apply，
 *   必须进入重新读取/三方合并/冲突路径。
 */
sealed interface ExternalContentDecision {
    data object Apply : ExternalContentDecision

    data object IgnoreReplay : ExternalContentDecision

    data object IgnoreOlder : ExternalContentDecision

    data object IgnoreDirtyConflict : ExternalContentDecision

    data object IgnoreSameContent : ExternalContentDecision

    data object IgnoreEmptyVersion : ExternalContentDecision

    data object IgnoreUncomparableConflict : ExternalContentDecision
}

/**
 * #595 五：外部正文 reset 的可提交事务结果 — 替代旧 resetPersistentSession 返回 Unit。
 *
 * 旧实现 reset 失败（Core textEditSessionReset 失败、session 无效、非持久 target）
 * 时只进入空分支，WritingPane 仍无条件执行 applyExternalContentFact +
 * applyExternalContentToUi，导致 Rust session（旧正文）/ SessionStore（新版本）/
 * ViewModel（新正文+hash）三份状态分裂。
 *
 * - [Success]：Core reset 成功，携带真实 snapshot — 调用方据此一次性提交会话事实与 UI；
 * - [Failed]：reset 未执行或 Core 失败 — 调用方必须保持旧正文与旧版本，不得推进任何状态。
 */
sealed interface ExternalResetResult {
    data class Success(val snapshot: TargetSnapshot) : ExternalResetResult

    data object Failed : ExternalResetResult
}
