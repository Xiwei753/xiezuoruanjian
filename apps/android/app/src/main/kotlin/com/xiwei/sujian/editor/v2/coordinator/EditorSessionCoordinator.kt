package com.xiwei.sujian.editor.v2.coordinator

import android.util.Log
import androidx.compose.runtime.Immutable
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import com.xiwei.sujian.editor.v2.motion.EditorMotionPolicy

/**
 * #592 四：窗口绑定状态机 — 会话层唯一的窗口生命周期事实。
 *
 * - [Idle]：无窗口绑定、无活动会话。
 * - [Attaching]：窗口正在绑定（beginEdit 进行中）。
 * - [Attached]：窗口已绑定，输入法/渲染活跃。
 * - [Detaching]：窗口销毁中，正在保存 snapshot。
 * - [Detached]：窗口已销毁，Rust session 与 snapshot 保留，等待新窗口附着。
 * - [Committing] / [Cancelling]：编辑事务结束中。
 *
 * Detached 状态下 commit/cancel 不再依赖 target 对象存在：正文已通过
 * onTextChanged 流式保存，业务关闭（[EditorSessionCoordinator.closeTarget]）
 * 直接关闭 Rust session 并回到 Idle。
 */
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
 * - [newlyCreated]=true：本事务新建的临时 session — 回滚时关闭；
 * - [newlyCreated]=false：事务前已存在的保留 session（含 Undo 历史）—
 *   回滚时恢复 [previousRecord]，不关闭 session。
 */
@Immutable
data class PreparedSessionHandle(
    val targetId: String,
    val sessionId: ULong,
    val snapshot: TargetSnapshot,
    val newlyCreated: Boolean,
    val previousRecord: EditorSessionRecord?,
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

/**
 * #592 一/四：#595 四：会话层协调器 — 只管理 Rust session、正文/选区纯数据快照、
 * Undo/Redo 所属 session、活动目标、窗口绑定状态机与编辑事务。
 *
 * 不持有 View、Activity、Choreographer、WindowDisplayFrameClock、窗口几何、
 * Compose mutableState、TextPaint、TargetDisplayRuntime。
 * 由 Activity 级 ViewModel 持有，跨配置变化存活；窗口/渲染对象全部在
 * [EditorWindowHost]（窗口层）。
 *
 * #595 四：per-target 持久事实由 [EditorSessionStore]（Map<TargetId, EditorSessionRecord>）
 * 保存（sessionId 属于所有活动 session，非持久 target 同样记录）；
 * 窗口重绑只改 binding，正文版本/hash/transaction/selection 保留。
 * #595 二：正文版本使用 [DocumentVersion]（Repository/Core 锚点），
 * 不再使用进程内 contentVersion 计数器。
 */
@Suppress("LargeClass", "TooManyFunctions") // #597 技术债：待重构拆分
class EditorSessionCoordinator(
    private val appServiceBridge: AppServiceBridge,
) : SessionCommandPort {

    // ── 纯会话状态 ──
    /** #595 四：per-target 会话记录存储 — 会话层持久事实的唯一来源。 */
    private val store = EditorSessionStore()

    // #595 三：_sessionStateFlow 是会话层唯一可写 MutableStateFlow — 所有状态变化
    // 通过 [updateSessionState]（原子 [MutableStateFlow.update]）统一推进。
    private val _sessionStateFlow = MutableStateFlow(EditorSessionState())
    val sessionStateFlow: StateFlow<EditorSessionState> = _sessionStateFlow.asStateFlow()
    val sessionState: EditorSessionState get() = _sessionStateFlow.value

    val activeTargetId: String? get() = _sessionStateFlow.value.activeTargetId

    val editingState: EditingState get() = _sessionStateFlow.value.editingState

    val windowBindingState: WindowBindingState get() = _sessionStateFlow.value.bindingState

    /**
     * #595 三：活动 session — 从唯一 SessionState 快照派生（sessionId + 活动 target）。
     * Detached 时 activeTargetId 为 null，本 getter 同样返回 null
     * （会话已保留在 store 记录中，由 closeTarget 收口）。
     */
    private val activeSessionId: ULong?
        get() = _sessionStateFlow.value.sessionId?.takeIf {
            _sessionStateFlow.value.activeTargetId != null
        }

    private val _targetDecorationsVersionFlow = MutableStateFlow(0L)
    val targetDecorationsVersionFlow: StateFlow<Long> = _targetDecorationsVersionFlow.asStateFlow()
    val targetDecorationsVersion: Long get() = _targetDecorationsVersionFlow.value

    private val _lastCommittedTextFlow = MutableStateFlow<String?>(null)
    val lastCommittedTextFlow: StateFlow<String?> = _lastCommittedTextFlow.asStateFlow()
    val lastCommittedText: String? get() = _lastCommittedTextFlow.value

    // ── #595 二：输入 lease / epoch ──

    /**
     * 输入 lease epoch — 章节切换提交、业务关闭、窗口解绑时递增，使旧窗口
     * 持有的 lease 失效。所有编辑回调必须携带绑定时的 lease，Coordinator
     * 只接受仍匹配当前活动 target、session 和 epoch 的事件。
     */
    @Volatile
    private var inputLeaseEpoch = 0L

    /** #595 二：文档操作 ID 生成器 — 每次签发 DocumentOperationLease 递增。 */
    private val operationIdCounter = java.util.concurrent.atomic.AtomicLong(0)

    /**
     * 窗口绑定成功时签发当前 lease — 窗口层在 performViewBind 时取得，
     * 之后每次 onLocalEdit/onExternalEdit/onContentChanged 提交都携带。
     * 无活动 target（未绑定）时返回 null — 没有可接收输入的会话。
     */
    fun currentInputLease(): EditorInputLease? {
        val s = _sessionStateFlow.value
        val targetId = s.activeTargetId ?: return null
        return EditorInputLease(targetId, s.sessionId ?: 0UL, inputLeaseEpoch)
    }

    /** 使当前所有输入 lease 失效 — 旧 View 晚到的回调不能再进入会话层。 */
    fun invalidateInputLease() {
        inputLeaseEpoch++
    }

    /**
     * #595 二：签发文档操作租约 — 保存/同步开始时一次性取得完整不可变文档快照。
     *
     * 包含 target/session/epoch/revision/text/committedVersion 全部字段，
     * 调用方不再自行拼接 currentSession + sessionState。无活动 target 或
     * 无 session 时返回 null（无可操作的文档）。
     */
    fun issueDocumentOperationLease(): DocumentOperationLease? {
        val s = _sessionStateFlow.value
        val targetId = s.activeTargetId ?: return null
        val sessionId = s.sessionId ?: return null
        val record = store.record(targetId) ?: return null
        return DocumentOperationLease(
            operationId = operationIdCounter.incrementAndGet(),
            targetId = targetId,
            coreSessionId = sessionId,
            inputEpoch = inputLeaseEpoch,
            rustRevision = s.revision,
            text = s.text,
            committedVersion = record.documentState.committedVersion,
        )
    }

    /**
     * #595 二：校验文档操作租约是否仍匹配当前活动文档。
     *
     * 保存/同步返回后由同一 reducer 判断：
     * - target/session/epoch 完全一致 → 操作有效；
     * - 任一字段不匹配 → 只记录旧版本确实落盘，当前文档保持 Unsaved。
     */
    fun isDocumentOperationLeaseCurrent(lease: DocumentOperationLease): Boolean {
        val s = _sessionStateFlow.value
        if (s.activeTargetId != lease.targetId) return false
        val expectedSession = s.sessionId ?: return false
        if (expectedSession != lease.coreSessionId) return false
        return inputLeaseEpoch == lease.inputEpoch
    }

    /**
     * 校验事件携带的 lease 是否仍匹配当前活动 target/session/epoch。
     * 窗口层在转发 onContentChanged 前也使用同一校验。
     *
     * 无活动绑定（Idle/Detached）时只校验 epoch + target — 此时窗口回调已
     * 被清除，晚到事件由 epoch 拒绝（commit/close/detach 都递增 epoch），
     * 不允许旧 View 在切换提交后写入新会话。
     */
    fun isInputLeaseCurrent(lease: EditorInputLease?, eventTargetId: String? = null): Boolean {
        if (lease == null) return false
        if (lease.epoch != inputLeaseEpoch) return false
        if (eventTargetId != null && lease.targetId != eventTargetId) return false
        val s = _sessionStateFlow.value
        val active = s.activeTargetId
        if (active == null) return true
        if (active != lease.targetId) return false
        val expectedSession = s.sessionId ?: 0UL
        return lease.sessionId == expectedSession
    }

    // #595 七：只保留一个可写事实源 — MutableStateFlow<EditorMotionPolicy>。
    private val _motionPolicyFlow = MutableStateFlow(EditorMotionPolicy())
    val motionPolicyFlow: StateFlow<EditorMotionPolicy> = _motionPolicyFlow.asStateFlow()

    /**
     * #595 三：唯一状态更新入口（reducer）— 所有会话状态变化通过
     * [MutableStateFlow.update] 原子推进 [_sessionStateFlow]（CAS 重试，
     * 不存在读后写竞态窗口）。不得在其他位置直接赋值 [_sessionStateFlow]
     * 或创建第二套可写 Flow。
     */
    private fun updateSessionState(transform: (EditorSessionState) -> EditorSessionState) {
        _sessionStateFlow.update(transform)
    }

    /**
     * #595 三/七：原子应用 [EditorMotionPolicy] — 唯一可写事实源。
     */
    fun applyMotionPolicy(policy: EditorMotionPolicy) {
        _motionPolicyFlow.value = policy
    }

    fun getMotionPolicy(): EditorMotionPolicy = _motionPolicyFlow.value

    // ── #595 一：本地 IME/键盘编辑 ──

    /**
     * #595 一：应用本地 IME/键盘编辑 — 更新唯一 SessionState，不触发 reset。
     *
     * 由 [EditorWindowHost.installContentCallback] 在 SujianEditorView 产生
     * EditResult 后调用。revision/transactionId 来自 Rust EditResult，
     * selectionAnchor/Head 来自 pipeline mirror（真实选区）。
     * #595 二：本地输入置 localDirty=true（存在未落盘编辑），
     * 外部版本不得直接 reset 覆盖。
     * #595 四：sessionId 从 store 记录读取 — 非持久 target 同样有 sessionId。
     * #595 二：必须携带窗口绑定时的 [EditorInputLease] — 旧章节 View 在切换
     * 提交后晚到的输入（epoch/target 不匹配）被拒绝，不得写入新章节会话。
     */
    @Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod") // #597 技术债：待重构拆分
    fun applyLocalEdit(update: EditorDocumentUpdate.LocalInput) {
        if (!isInputLeaseCurrent(update.lease, update.targetId)) {
            Log.w(TAG, "applyLocalEdit(${update.targetId}): stale input lease rejected")
            return
        }
        var pendingRecord: EditorSessionRecord? = null
        updateSessionState { previous ->
            val existing = store.record(update.targetId)
            val previousDoc = existing?.documentState ?: DocumentState()
            val sessionId = existing?.sessionId
                ?: previous.sessionId?.takeIf { previous.targetId == update.targetId }
                ?: 0UL
            val contentChanged = update.text != previousDoc.text
            val dirty = if (!contentChanged && update.operationKind == EditorOperationKind.SELECTION) {
                previousDoc.localDirty
            } else {
                contentChanged
            }
            pendingRecord = existing?.copy(
                documentState = previousDoc.copy(
                    text = update.text,
                    revision = update.revision,
                    selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
                    selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
                    lastAppliedTransactionId = update.transactionId,
                    localDirty = dirty,
                ),
            ) ?: EditorSessionRecord(
                targetId = update.targetId,
                documentState = DocumentState(
                    text = update.text,
                    revision = update.revision,
                    selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else 0,
                    selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else 0,
                    lastAppliedTransactionId = update.transactionId,
                    localDirty = true,
                ),
            )
            EditorSessionState(
                targetId = update.targetId,
                sessionId = sessionId,
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
                lastAppliedTransactionId = update.transactionId,
                origin = EditorSessionOrigin.LOCAL_INPUT,
                bindingState = previous.bindingState,
                editingState = previous.editingState,
                activeTargetId = previous.activeTargetId,
                committedVersion = previousDoc.committedVersion,
                sessionBaseVersion = previousDoc.sessionBaseVersion,
                localDirty = dirty,
            )
        }
        pendingRecord?.let { store.put(it) }
    }

    /**
     * #595 二：撤销/恢复事件已执行后更新 SessionState —
     * revision/transactionId 来自 Rust EditResult，来源标记为 UNDO_RESTORED。
     * 撤销/恢复改变正文但保留在 session 内（Undo 栈就是本地历史），
     * 不改变 localDirty 的既有事实（撤销后正文仍未落盘时保持 dirty）。
     * #595 二：陈旧 lease（章节切换后旧 View 晚到的撤销事件）拒绝。
     */
    fun applyUndoRestored(update: EditorDocumentUpdate.UndoRestored) {
        if (!isInputLeaseCurrent(update.lease, update.targetId)) {
            Log.w(TAG, "applyUndoRestored(${update.targetId}): stale input lease rejected")
            return
        }
        var pendingRecord: EditorSessionRecord? = null
        updateSessionState { previous ->
            val existing = store.record(update.targetId)
            val previousDoc = existing?.documentState ?: DocumentState()
            val sessionId = existing?.sessionId
                ?: previous.sessionId?.takeIf { previous.targetId == update.targetId }
                ?: 0UL
            val contentChanged = update.text != previousDoc.text
            val dirty = previousDoc.localDirty || contentChanged
            pendingRecord = existing?.copy(
                documentState = previousDoc.copy(
                    text = update.text,
                    revision = update.revision,
                    selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
                    selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
                    lastAppliedTransactionId = update.transactionId,
                    localDirty = dirty,
                    ),
                ) ?: EditorSessionRecord(
                    targetId = update.targetId,
                    documentState = DocumentState(
                        text = update.text, revision = update.revision,
                        selectionAnchorUtf8 = update.selectionAnchorUtf8.coerceAtLeast(0),
                        selectionHeadUtf8 = update.selectionHeadUtf8.coerceAtLeast(0),
                        lastAppliedTransactionId = update.transactionId,
                        localDirty = true,
                    ),
                )
            EditorSessionState(
                targetId = update.targetId,
                sessionId = sessionId,
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
                lastAppliedTransactionId = update.transactionId,
                origin = EditorSessionOrigin.UNDO_RESTORED,
                bindingState = previous.bindingState,
                editingState = previous.editingState,
                activeTargetId = previous.activeTargetId,
                committedVersion = previousDoc.committedVersion,
                sessionBaseVersion = previousDoc.sessionBaseVersion,
                localDirty = dirty,
            )
        }
        pendingRecord?.let { store.put(it) }
    }

    /**
     * #595 二：程序化替换事件已执行后更新 SessionState —
     * revision/transactionId 来自 Rust EditResult，来源标记为 PROGRAMMATIC_REPLACE。
     * #595 二：陈旧 lease（章节切换后旧 View 晚到的程序化结果）拒绝。
     */
    fun applyProgrammaticReplace(update: EditorDocumentUpdate.ProgrammaticReplace) {
        if (!isInputLeaseCurrent(update.lease, update.targetId)) {
            Log.w(TAG, "applyProgrammaticReplace(${update.targetId}): stale input lease rejected")
            return
        }
        var pendingRecord: EditorSessionRecord? = null
        updateSessionState { previous ->
            val existing = store.record(update.targetId)
            val previousDoc = existing?.documentState ?: DocumentState()
            val sessionId = existing?.sessionId
                ?: previous.sessionId?.takeIf { previous.targetId == update.targetId }
                ?: 0UL
            val contentChanged = update.text != previousDoc.text
            val dirty = previousDoc.localDirty || contentChanged
            pendingRecord = existing?.copy(
                documentState = previousDoc.copy(
                    text = update.text,
                    revision = update.revision,
                    selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
                    selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
                    lastAppliedTransactionId = update.transactionId,
                    localDirty = dirty,
                    ),
                ) ?: EditorSessionRecord(
                    targetId = update.targetId,
                    documentState = DocumentState(
                        text = update.text, revision = update.revision,
                        selectionAnchorUtf8 = update.selectionAnchorUtf8.coerceAtLeast(0),
                        selectionHeadUtf8 = update.selectionHeadUtf8.coerceAtLeast(0),
                        lastAppliedTransactionId = update.transactionId,
                        localDirty = true,
                    ),
                )
            EditorSessionState(
                targetId = update.targetId,
                sessionId = sessionId,
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previousDoc.selectionAnchorUtf8,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previousDoc.selectionHeadUtf8,
                lastAppliedTransactionId = update.transactionId,
                origin = EditorSessionOrigin.PROGRAMMATIC_REPLACE,
                bindingState = previous.bindingState,
                editingState = previous.editingState,
                activeTargetId = previous.activeTargetId,
                committedVersion = previousDoc.committedVersion,
                sessionBaseVersion = previousDoc.sessionBaseVersion,
                localDirty = dirty,
            )
        }
        pendingRecord?.let { store.put(it) }
    }

    // ── #595 二：外部文档事实（Repository 加载 / 同步合并）──

    /**
     * #595 二：外部文档事实的新旧判断 — 文档版本锚点 + localDirty。
     *
     * 规则（与 Issue #595 解决二/五一致）：
     * - 空版本 → 不可应用；
     * - 同 sourceVersion 重放 → 忽略（幂等，新 collector 读到当前文档事实也不会
     *   再次执行副作用）；
     * - 外部 sourceVersion 可比较且旧于 committedVersion → 忽略；
     * - localDirty=true → 冲突 — 禁止直接 reset（本地输入不得被同步下载覆盖），
     *   调用方必须发布类型化冲突；
     * - 正文一致 → 忽略（无需 reset）；
     * - 从未建立任何版本（空 committed）→ 可应用（首次加载/首次事实）；
     * - 两侧版本不可比较（无相同 revision 锚点、incoming 父版本链不含
     *   committed）且正文不同 → 不得默认 Apply — 进入重新读取/三方合并/冲突
     *   （旧实现把"不可比较"当作"可应用"，旧 IO 结果最后返回仍可能覆盖本地）。
     */
    fun shouldApplyExternalContent(fact: TargetDocumentFact): ExternalContentDecision {
        if (fact.sourceVersion.isEmpty) return ExternalContentDecision.IgnoreEmptyVersion
        // #595 二/四：比较基于该 target 的 store 记录文档事实（committedVersion /
        // localDirty / text）— 与可观察 SessionState 是否恰好指向活动 target 无关，
        // 新 collector 读到的是当前文档事实，重放旧事实幂等忽略。
        val doc = store.record(fact.targetId)?.documentState ?: DocumentState()
        if (doc.committedVersion == fact.sourceVersion) return ExternalContentDecision.IgnoreReplay
        if (doc.localDirty) return ExternalContentDecision.IgnoreDirtyConflict
        if (isVersionOlder(doc.committedVersion, fact.sourceVersion)) return ExternalContentDecision.IgnoreOlder
        if (doc.text == fact.text) return ExternalContentDecision.IgnoreSameContent
        // 空 committed（从未建立版本事实）→ 首次应用（章节首次加载）。
        if (doc.committedVersion.isEmpty) return ExternalContentDecision.Apply
        // #595 四：正文不同且版本不可比较 — Repository 加载（用户主动打开章节）
        // 信任磁盘内容直接 Apply；同步合并不得盲目覆盖（Git 回退/外部修改/迟到 IO），
        // 进入 IgnoreUncomparableConflict 由调用方走重新读取/三方合并/冲突路径。
        if (!isComparable(doc.committedVersion, fact.sourceVersion)) {
            return if (fact.origin == DocumentFactOrigin.REPOSITORY_LOAD) {
                ExternalContentDecision.Apply
            } else {
                ExternalContentDecision.IgnoreUncomparableConflict
            }
        }
        return ExternalContentDecision.Apply
    }

    /**
     * 版本新旧比较 — 只有同一单调 revision 锚点可比较时才判定"旧"：
     * 两侧都携带非零 [DocumentVersion.repositoryRevision] → 比较之；
     * 否则视为不可比较（不判定"旧"，由 [isComparable] 决定是否可应用）。
     * 时间锚点（lastSyncTime）不得参与因果比较。
     */
    private fun isVersionOlder(committed: DocumentVersion, incoming: DocumentVersion): Boolean {
        if (committed.repositoryRevision != 0L && incoming.repositoryRevision != 0L) {
            return incoming.repositoryRevision < committed.repositoryRevision
        }
        return false
    }

    /**
     * #595 五：版本可比较性 — 满足任一条件即存在因果顺序：
     * - 同一非空 contentHash（相同版本）；
     * - 两侧都携带非零 repositoryRevision（单调锚点）；
     * - incoming 的父版本链包含 committed（或与其同内容）— incoming 是
     *   committed 的后代（同步合并结果携带 parentVersion=同步前磁盘版本）。
     * 其余不同版本不可比较 — 不得默认 Apply。
     */
    private fun isComparable(committed: DocumentVersion, incoming: DocumentVersion): Boolean {
        if (committed.contentHash.isNotEmpty() && committed.contentHash == incoming.contentHash) return true
        if (committed.repositoryRevision != 0L && incoming.repositoryRevision != 0L) return true
        var parent = incoming.parentVersion
        while (parent != null) {
            if (parent == committed) return true
            if (committed.contentHash.isNotEmpty() && parent.contentHash == committed.contentHash) return true
            parent = parent.parentVersion
        }
        return false
    }

    /**
     * #595 二：外部文档事实已应用后记录版本 — 更新 store 记录与活动 SessionState。
     *
     * 只记录版本事实（committedVersion=sourceVersion、localDirty=false），
     * 不执行 Core reset（reset 由调用方在 decision==Apply 时经
     * [resetPersistentSession] 执行，最终 revision 来自 reset 后的真实 snapshot）。
     * IgnoreSameContent 时调用本方法同样安全（幂等记录版本）。
     *
     * #595 五：session 被 reset 到新正文后，它的新 base 就是 sourceVersion
     * （旧实现保留 fact.baseVersion — 旧 base 会让下一次同步仍以过时祖先判断
     * 冲突）。Repository 加载/同步合并都来自磁盘，磁盘已处于 sourceVersion，
     * 因此 lastSavedVersion 同步推进。
     */
    fun applyExternalContentFact(fact: TargetDocumentFact) {
        var pendingRecord: EditorSessionRecord? = null
        updateSessionState { previous ->
            val record = store.record(fact.targetId)
            val previousDoc = record?.documentState ?: DocumentState()
            val newDoc = previousDoc.copy(
                committedVersion = fact.sourceVersion,
                sessionBaseVersion = fact.sourceVersion,
                lastSavedVersion = fact.sourceVersion,
                localDirty = false,
            )
            pendingRecord = record?.copy(documentState = newDoc)
                ?: EditorSessionRecord(targetId = fact.targetId, documentState = newDoc)
            // 无活动 target（state.targetId == null）时同样把文档事实反映到可观察状态；
            // 活动 target 属于其他章节时只更新 store 记录，不清掉活动状态。
            if (previous.targetId != fact.targetId && previous.targetId != null) return@updateSessionState previous
            previous.copy(
                committedVersion = fact.sourceVersion,
                sessionBaseVersion = fact.sourceVersion,
                localDirty = false,
                origin = if (fact.origin == DocumentFactOrigin.SYNC_MERGED) {
                    EditorSessionOrigin.SYNC_MERGED
                } else {
                    EditorSessionOrigin.EXTERNAL_REPLACE
                },
            )
        }
        pendingRecord?.let { store.put(it) }
    }

    /**
     * #595 二：保存成功上报 — 由 [com.xiwei.sujian.ui.EditorViewModel] 在保存
     * 成功后调用。保存回执必须作为文档提交原子推进：
     *
     * ```text
     * committedVersion = savedVersion
     * sessionBaseVersion = savedVersion
     * lastSavedVersion = savedVersion
     * localDirty = false
     * ```
     *
     * 正文已落盘，下一次同步以磁盘版本为三方合并基础；同步合并事实携带
     * parentVersion=该版本时即可比较并安全应用。
     */
    fun markSaved(targetId: String, savedVersion: DocumentVersion) {
        if (savedVersion.isEmpty) return
        var pendingRecord: EditorSessionRecord? = null
        updateSessionState { state ->
            val record = store.record(targetId)
            if (record != null) {
                pendingRecord = record.withDocumentState {
                    it.copy(
                        committedVersion = savedVersion,
                        sessionBaseVersion = savedVersion,
                        lastSavedVersion = savedVersion,
                        localDirty = false,
                    )
                }
            }
            if (state.targetId == targetId) {
                state.copy(
                    committedVersion = savedVersion,
                    sessionBaseVersion = savedVersion,
                    localDirty = false,
                )
            } else {
                state
            }
        }
        pendingRecord?.let { store.put(it) }
    }

    /**
     * #595 五：按 target 从 store 读取 committedVersion — 同步事实的 baseVersion
     * 必须按 target 查询（旧实现读全局 sessionState.committedVersion，活动状态
     * 属于其他 target 时 B 的同步事件会携带 A 的 base）。
     */
    fun documentCommittedVersionFor(targetId: String): DocumentVersion =
        store.record(targetId)?.documentState?.committedVersion ?: DocumentVersion()

    // ── 纯数据目标元数据（窗口层 registerTarget/updateTargetSpec 镜像）──

    /**
     * #595 一/四：注册 target 元数据（无窗口对象，供章节切换事务预准备 session 使用）。
     * 幂等：已注册时更新 profile/persistent，保留 sessionId 与文档事实。
     */
    fun registerTargetMeta(targetId: String, profile: TextEditorProfile, persistent: Boolean) {
        val existing = store.record(targetId)
        if (existing != null) {
            store.update(targetId) { it.copy(profile = profile, persistent = persistent) }
        } else {
            store.put(EditorSessionRecord(targetId = targetId, profile = profile, persistent = persistent))
        }
    }

    fun registerTarget(target: EditableTextTarget) {
        registerTargetMeta(target.targetId, target.profile, target.isPersistent)
    }

    fun updateTargetSpec(
        targetId: String,
        profile: TextEditorProfile? = null,
    ) {
        profile?.let { registerTargetMeta(targetId, it, store.record(targetId)?.persistent ?: false) }
    }

    fun isTargetPersistent(targetId: String): Boolean = store.record(targetId)?.persistent ?: false

    /** #595 四：所有活动 session 都有 ID — 非持久 target 同样返回记录中的 ID。 */
    fun getPersistentSessionId(targetId: String): ULong? = store.record(targetId)?.sessionId

    fun isTargetRegistered(targetId: String): Boolean = store.isRegistered(targetId)

    /**
     * #595 一：无副作用预准备 — 章节切换事务在最终 requestId 校验前调用。
     *
     * 只允许：读取 B 的记录、验证或新建 B session、读取 snapshot、返回句柄。
     * 禁止：commit/cancel A、修改 activeTargetId、修改 WindowBindingState、
     * 修改全局 EditorSessionState、关闭任何既有有效 session。
     *
     * 事务的提交（A→B 一次性切换）由 [commitPreparedSession] 完成；
     * 回滚由 [releasePreparedTarget] 完成（newlyCreated 才关闭 session，
     * 借用的既有 session 恢复 previousRecord 保留 Undo 历史）。
     *
     * 记录中已有有效 session 时直接复用（Undo/Redo、composition、selection
     * 原样保留）；无效/缺失时新建（唯一一次 Core 命令，snapshot 由 create
     * 后的真实内核读取）。snapshot 读取失败视为预准备失败（返回 null，
     * 新建的临时 session 立即关闭，不留下无 snapshot 的孤儿 session）。
     */
    fun prepareTargetSessionForCommit(
        targetId: String,
        initialText: String,
        initialSelection: Int?,
    ): PreparedSessionHandle? {
        val record = store.record(targetId) ?: return null
        val isPersistent = record.persistent
        val existingId = record.sessionId
        var newlyCreated = false
        val sessionId: ULong? = if (existingId != null && existingId != 0UL && validateSession(existingId)) {
            existingId
        } else {
            // 记录中的 ID 无效/缺失：清理失效 ID（Core 侧已不存在）并新建临时 session。
            if (existingId != null && existingId != 0UL) {
                closeSession(existingId)
            }
            newlyCreated = true
            val sel = initialSelection ?: initialText.toByteArray(Charsets.UTF_8).size
            createSession(targetId, initialText, sel, isPersistent)
        }
        if (sessionId == null || sessionId == 0UL) return null
        val snapshot = querySnapshotForSession(sessionId)
        if (snapshot == null) {
            // 新建的 session 读不到 snapshot — 预准备失败，关闭临时 session 不留下孤儿。
            if (newlyCreated) closeSession(sessionId)
            return null
        }
        return PreparedSessionHandle(
            targetId = targetId,
            sessionId = sessionId,
            snapshot = snapshot,
            newlyCreated = newlyCreated,
            previousRecord = record,
        )
    }

    /**
     * #595 一：提交预准备 — 最终 requestId 校验通过后一次性执行 A→B 切换：
     *
     * ```text
     * 1. 冻结并撤销 A 的输入 lease（epoch++，旧 View 晚到一帧也不能写入 B）；
     * 2. 提交旧活动目标 A（persistent + 窗口绑定保持 Rust session，Undo 保留；
     *    非持久/未绑定关闭）；
     * 3. 激活 B：activeTargetId=B、Attaching(prepared)、BINDING，
     *    snapshot 装入 SessionState（正文/revision/选区），文档事实保留。
     * ```
     *
     * 窗口层 beginEdit 复用同一 session 并把 windowId 替换为真实窗口。
     * 失败（记录被并发移除或 session 不再匹配）返回 false，调用方必须回滚。
     */
    fun commitPreparedSession(handle: PreparedSessionHandle, windowId: String = "prepared"): Boolean {
        val record = store.record(handle.targetId) ?: return false
        // #595 一：句柄仍有效 — 新建事务要求记录 sessionId 仍是 prepare 前的值
        // （prepare 不修改 store；previousRecord.sessionId 是事务前值，默认 0UL）；
        // 复用事务要求记录仍指向同一 session。不能要求"记录已存在 handle.sessionId"，
        // 否则新建 session（prepare 不写 store）永远无法提交。
        val expectedSessionId = if (handle.newlyCreated) {
            handle.previousRecord?.sessionId ?: 0UL
        } else {
            handle.sessionId
        }
        if (record.sessionId != expectedSessionId) return false
        // 1. 冻结并撤销 A 的输入 lease。
        invalidateInputLease()
        // 2. 一次性提交旧活动目标（若仍是活动状态）。
        val oldActive = activeTargetId
        @Suppress("CollapsibleIfStatements") // commitActiveSession 有副作用，拆分更清晰
        if (oldActive != null && oldActive != handle.targetId) {
            if (!commitActiveSession(null)) {
                cancelActiveSession()
            }
        }
        // 3. 激活 B — 通过唯一 reducer 原子推进。
        val snapshot = handle.snapshot
        val attaching = WindowBindingState.Attaching(windowId, handle.targetId, handle.sessionId)
        var pendingRecord: EditorSessionRecord? = null
        updateSessionState { _ ->
            // #595 一：提交时把 handle.sessionId + snapshot 写入 B 的正式记录 —
            // prepare 不修改 store，commit 是唯一写入点，保证 store 与 SessionState 一致。
            val rec = store.record(handle.targetId)
            val doc = rec?.documentState ?: DocumentState()
            pendingRecord = (rec ?: EditorSessionRecord(
                targetId = handle.targetId,
                persistent = handle.previousRecord?.persistent ?: false,
            )).copy(sessionId = handle.sessionId)
                .withDocumentState {
                    it.copy(
                        text = snapshot.text,
                        revision = snapshot.revision,
                        selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                        selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                    )
                }
            EditorSessionState(
                targetId = handle.targetId,
                sessionId = handle.sessionId,
                text = snapshot.text,
                revision = snapshot.revision,
                selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                lastAppliedTransactionId = doc.lastAppliedTransactionId,
                origin = EditorSessionOrigin.INITIAL_LOAD,
                bindingState = attaching,
                editingState = EditingState.BINDING,
                activeTargetId = handle.targetId,
                committedVersion = doc.committedVersion,
                sessionBaseVersion = doc.sessionBaseVersion,
                localDirty = doc.localDirty,
            )
        }
        pendingRecord?.let { store.put(it) }
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
            handle.sessionId.toString(), "commit_prepared"
        )
        return true
    }

    /**
     * #595 一：回滚预准备 — Abort 规则：
     *
     * - [PreparedSessionHandle.newlyCreated]=true → 只关闭本事务新建的临时 session
     *   并移除记录（若记录仍指向该 session）；
     * - newlyCreated=false → 恢复事务前的 [PreparedSessionHandle.previousRecord]
     *   （文档事实/selection/装饰快照），不关闭借用的既有 session — 回滚不得
     *   销毁事务开始前已经存在的 B session 与 Undo 历史。
     *
     * 无副作用：准备阶段从未修改全局 EditorSessionState，这里同样不修改。
     */
    fun releasePreparedTarget(handle: PreparedSessionHandle) {
        val record = store.record(handle.targetId)
        if (handle.newlyCreated) {
            closeSession(handle.sessionId)
            if (handle.sessionId != 0UL) {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
                    handle.sessionId.toString(), "release_prepared_new"
                )
            }
            // 只移除仍指向该 session 的记录 — 若事务期间记录已被其他路径替换，
            // 不覆盖其状态。
            if (record != null && record.sessionId == handle.sessionId) {
                store.remove(handle.targetId)
            }
        } else {
            // 借用的既有 session：恢复事务前记录，保留 Undo/Redo 与文档事实。
            val previous = handle.previousRecord
            if (previous != null) {
                store.put(previous)
            }
            if (handle.sessionId != 0UL) {
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
                    handle.sessionId.toString(), "release_prepared_borrowed"
                )
            }
        }
    }

    // ── 纯数据投影快照（窗口层读写）──

    fun saveProjectionSnapshot(targetId: String, snapshot: ProjectionSnapshot) {
        store.update(targetId) { it.copy(projection = snapshot) }
    }

    fun getProjectionSnapshot(targetId: String): ProjectionSnapshot? = store.record(targetId)?.projection

    // ── 窗口绑定状态机 ──

    /**
     * #592 二：Compose onDispose 唯一入口 — 只解除窗口绑定，不关闭持久 Rust session。
     *
     * persistent target：捕获真实 snapshot 并进入 [WindowBindingState.Detached]，
     * Rust session、Undo/Redo、revision、文档事实全部保留，新窗口可自动附着。
     * 非 persistent（草稿）target：关闭临时 session、删除记录并回到 Idle。
     *
     * 关闭持久 session 必须由业务事件 [closeTarget] 触发（返回章节列表、切换章节、
     * 删除章节），配置变化不改变 workspace route，因此不会关闭 session。
     * #595 四：只清理本 target 的窗口状态，不得把其他活动 target 的 binding
     * 状态一并清成 Idle。
     */
    fun detachWindowBinding(windowId: String, targetId: String) {
        val record = store.record(targetId)
        val isPersistent = record?.persistent ?: false
        val sessionId = record?.sessionId
        // #595 二：窗口解绑使该窗口持有的输入 lease 失效 — 解绑后晚到的回调
        // （回调清除窗口期内的竞态）不能再进入会话层。
        invalidateInputLease()
        if (!isPersistent || sessionId == null || sessionId == 0UL) {
            // 草稿会话或已无会话：直接关闭/清理窗口引用
            if (sessionId != null && sessionId != 0UL) {
                closeSession(sessionId)
            }
            store.remove(targetId)
            clearWindowAttach(targetId)
            return
        }
        val snapshot = if (validateSession(sessionId)) queryTargetSnapshot(targetId) else null
        val detached = WindowBindingState.Detached(targetId, sessionId, snapshot)
        // #595 三/四：通过唯一 reducer 原子推进 bindingState/editingState/activeTargetId。
        // #595 一：只清理本 target 的窗口状态 — 章节切换事务已把 B 设为活动目标时，
        // 旧 pane 的 onDispose（detachWindowBinding(A)）不得把 B 的 Attaching/Attached
        // 状态清成 Detached(A)。
        updateSessionState { state ->
            if (state.targetId != targetId) {
                state
            } else {
                state.copy(
                    bindingState = detached,
                    editingState = EditingState.IDLE,
                    activeTargetId = null,
                )
            }
        }
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
            sessionId.toString(), "window_detached"
        )
    }

    /**
     * #592 二：窗口绑定完成（视图已 bind/attach 成功）。
     * 由 [EditorWindowHost] 在 View 真实绑定成功后调用。
     *
     * #595 三：防御性状态守卫 — Attached 只能从 Attaching 进入。
     */
    fun completeWindowAttach(windowId: String, targetId: String, sessionId: ULong) {
        val current = _sessionStateFlow.value.bindingState
        // 幂等重入：已经是同一 target/session 的 Attached（如 beginEdit 重复调用）保持现状。
        if (current is WindowBindingState.Attached &&
            current.targetId == targetId && current.sessionId == sessionId
        ) {
            return
        }
        if (current !is WindowBindingState.Attaching || current.targetId != targetId) {
            Log.w(TAG, "completeWindowAttach($targetId): current state $current is not Attaching for target — ignoring (Attached requires a bound View)")
            return
        }
        val attached = WindowBindingState.Attached(windowId, targetId, sessionId)
        updateSessionState { it.copy(
            bindingState = attached,
            editingState = EditingState.EDITING,
        ) }
    }

    /**
     * #592 三：#595 四：业务级关闭 — 由 workspace 导航事件调用（返回章节列表、
     * 切换章节、删除章节）。与窗口解绑 [detachWindowBinding] 分开：关闭会销毁
     * Rust session，解绑只解除窗口引用。
     *
     * 只有关闭的 target 是当前活动/当前 SessionState 的 target 时才重置全局状态；
     * 关闭非活动 target 不得清掉活动 target 的 binding/editing。
     * 关闭同时使该 target 的输入 lease 失效（旧 View 晚到的回调不再被接受）。
     */
    fun closeTarget(targetId: String, reason: SessionCloseReason) {
        val wasActive = activeTargetId == targetId
        if (wasActive) {
            commitActiveSession(null)
        }
        val record = store.record(targetId)
        val sessionId = record?.sessionId
        if (sessionId != null && sessionId != 0UL) {
            closeSession(sessionId)
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
                sessionId.toString(), "close_target:${reason.name.lowercase()}"
            )
        }
        store.remove(targetId)
        invalidateInputLease()
        // #595 三/四：被关闭的 target 是当前 SessionState target 时整体回到 Idle；
        // 否则保留活动 target 的状态（旧实现会把新章节的 Attached 清成 Idle）。
        // 记录已删除，残留的 Detached(targetId) 状态没有意义 — 统一回 Idle。
        if (_sessionStateFlow.value.targetId == targetId) {
            updateSessionState {
                if (it.targetId == targetId) {
                    EditorSessionState(
                        editingState = EditingState.IDLE,
                        bindingState = WindowBindingState.Idle,
                        activeTargetId = null,
                    )
                } else {
                    it
                }
            }
        }
    }

    private fun clearWindowAttach(targetId: String) {
        store.remove(targetId)
        updateSessionState {
            if (it.targetId == targetId) {
                it.copy(
                    editingState = EditingState.IDLE,
                    bindingState = WindowBindingState.Idle,
                    activeTargetId = null,
                    targetId = null,
                    sessionId = null,
                )
            } else {
                it
            }
        }
    }

    /**
     * 准备会话绑定 — 创建/复用 session 并设置活动状态。
     * 返回绑定信息或 null（失败时）。
     *
     * #592 一：复用既有持久 session 时，绑定信息携带 Rust 的真实
     * textEditSessionSnapshot（text/revision/cursor/selection），窗口层据此执行
     * attachSnapshot，不再用新 Compose target 的正文/末尾光标执行 loadText
     * （那会 revision+1 并清空 Undo/Redo）。
     * #595 二：新建 session 同样携带 create 后的真实 snapshot（createSession 已
     * 接收初始正文，是唯一一次 Core 命令），窗口层 attachSnapshot 只重建本地镜像。
     * #595 一：章节切换事务预准备 session 时传入 windowId="prepared" —
     * 窗口层 beginEdit 复用同一 session，completeWindowAttach 时替换为真实 windowId。
     * #595 四：sessionId 写入 store 记录 — 非持久 target 同样记录。
     */
    @Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod", "LongMethod") // #597 技术债：待重构拆分
    fun prepareSessionForEdit(
        targetId: String,
        initialText: String,
        initialSelection: Int?,
        windowId: String = "prepared",
    ): SessionBindInfo? {
        val record = store.record(targetId) ?: return null
        val isPersistent = record.persistent
        val profile = record.profile

        if (activeTargetId == targetId && (editingState == EditingState.EDITING || editingState == EditingState.BINDING)) {
            val sid = store.record(targetId)?.sessionId ?: return null
            if (sid == 0UL) return null
            return SessionBindInfo(sid, profile, isPersistent, snapshot = querySnapshotForSession(sid))
        }

        if (activeTargetId != null && activeTargetId != targetId) {
            updateSessionState { it.copy(editingState = EditingState.REBINDING) }
            if (!commitActiveSession(null)) {
                cancelActiveSession()
            }
        }

        updateSessionState { it.copy(editingState = EditingState.BINDING) }

        val textForSession = initialText
        val sel = initialSelection ?: textForSession.toByteArray(Charsets.UTF_8).size
        val existingId = store.record(targetId)?.sessionId
        val sessionId = if (existingId != null && existingId != 0UL && validateSession(existingId)) {
            existingId
        } else {
            if (existingId != null && existingId != 0UL) {
                closeSession(existingId)
            }
            createSession(targetId, textForSession, sel, isPersistent)
        }

        if (sessionId == null || sessionId == 0UL) {
            Log.e(TAG, "prepareSessionForEdit($targetId): session creation returned invalid id=$sessionId, aborting")
            store.remove(targetId)
            updateSessionState { it.copy(
                editingState = EditingState.IDLE,
                bindingState = WindowBindingState.Idle,
            ) }
            return null
        }

        val attaching = WindowBindingState.Attaching(windowId, targetId, sessionId)

        // #595 一/二/三/四：通过唯一 reducer 更新 SessionState — 无论新建还是复用、
        // 持久还是草稿，都用真实 snapshot（createSession 已把初始正文装入 kernel，
        // 是唯一一次 Core 命令；草稿 session 同样记录 sessionId）。
        val snapshot = querySnapshotForSession(sessionId)
        store.update(targetId) { r ->
            r.copy(
                sessionId = sessionId,
                documentState = if (snapshot != null) {
                    r.documentState.copy(
                        text = snapshot.text,
                        revision = snapshot.revision,
                        selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                        selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                    )
                } else {
                    r.documentState.copy(
                        text = textForSession,
                        revision = 0L,
                        selectionAnchorUtf8 = sel,
                        selectionHeadUtf8 = sel,
                    )
                },
            )
        }
        updateSessionState { _ ->
            val rec = store.record(targetId)
            val doc = rec?.documentState
            EditorSessionState(
                targetId = targetId,
                sessionId = sessionId,
                text = doc?.text ?: textForSession,
                revision = doc?.revision ?: 0L,
                selectionAnchorUtf8 = doc?.selectionAnchorUtf8 ?: sel,
                selectionHeadUtf8 = doc?.selectionHeadUtf8 ?: sel,
                lastAppliedTransactionId = doc?.lastAppliedTransactionId ?: 0L,
                origin = EditorSessionOrigin.INITIAL_LOAD,
                bindingState = attaching,
                editingState = EditingState.BINDING,
                activeTargetId = targetId,
                committedVersion = doc?.committedVersion ?: DocumentVersion(),
                sessionBaseVersion = doc?.sessionBaseVersion ?: DocumentVersion(),
                localDirty = doc?.localDirty ?: false,
            )
        }
        return SessionBindInfo(sessionId, profile, isPersistent, snapshot = snapshot)
    }

    fun forceEditingState(state: EditingState) {
        updateSessionState { it.copy(editingState = state) }
    }

    /**
     * 提交活动编辑会话。
     *
     * - Attached：persistent 会话保持打开（软重置语义），记录保留，由窗口层继续复用。
     * - Detached/非持久：直接关闭 Rust session 并删除记录（正文已流式保存）。
     * 不依赖 target 对象存在，Detached 状态下也能完整收口。
     */
    fun commitActiveSession(finalText: String?): Boolean {
        val targetId = activeTargetId ?: return false
        val record = store.record(targetId) ?: return false
        val sessionId = record.sessionId
        if (sessionId == 0UL) return false
        val isPersistent = record.persistent
        val windowBound = windowBindingState is WindowBindingState.Attached ||
            windowBindingState is WindowBindingState.Attaching

        updateSessionState { it.copy(
            editingState = EditingState.COMMITTING,
            bindingState = if (windowBound) WindowBindingState.Committing(targetId, sessionId) else it.bindingState,
        ) }
        // #595 四：正文在 store 记录/SessionState 中（applyLocalEdit 已更新），
        // 不再维护第二份正文缓存。
        if (!isPersistent || !windowBound) {
            closeSession(sessionId)
            store.remove(targetId)
        }
        // #595 三/四：提交清除后 SessionState 必须回到 Idle。
        updateSessionState { EditorSessionState() }
        _lastCommittedTextFlow.value = if (record.profile.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) null else finalText
        return true
    }

    /**
     * 取消活动编辑会话 — Detached 状态下同样完整收口（关闭 session，不依赖 target 对象）。
     */
    fun cancelActiveSession(): Boolean {
        val targetId = activeTargetId ?: return false
        val record = store.record(targetId) ?: return false
        val sessionId = record.sessionId
        if (sessionId == 0UL) return false
        val windowBound = windowBindingState is WindowBindingState.Attached ||
            windowBindingState is WindowBindingState.Attaching

        updateSessionState { it.copy(
            editingState = EditingState.CANCELLING,
            bindingState = if (windowBound) WindowBindingState.Cancelling(targetId, sessionId) else it.bindingState,
        ) }
        closeSession(sessionId)
        store.remove(targetId)
        // #595 三/四：取消清除后 SessionState 必须回到 Idle。
        updateSessionState { EditorSessionState() }
        return true
    }

    fun resetPersistentSession(targetId: String, text: String, cursorUtf8: Int, source: SessionResetSource = SessionResetSource.EXTERNAL): ExternalResetResult {
        // #595 一/五：返回可提交事务结果 — reset 未执行或 Core 失败时返回 Failed，
        // 调用方不得推进 SessionStore/ViewModel 状态（旧实现返回 Unit，WritingPane
        // 无条件推进导致三份状态分裂）。
        // #595 一：禁止构造 revision=0 的兜底 snapshot — Core reset/create 成功后
        // 必须再次读取真实 snapshot，读取失败则整个操作失败（旧实现用输入参数补
        // revision=0 快照，调用方仍当成功，导致 Rust session/SessionStore/ViewModel
        // 三份状态分裂）。
        if (source == SessionResetSource.LOCAL_CONTENT_CHANGED) return ExternalResetResult.Failed
        val record = store.record(targetId)
        if (record?.persistent != true) return ExternalResetResult.Failed

        val sessionId = record.sessionId
        if (sessionId == 0UL) {
            val newSessionId = createSession(targetId, text, cursorUtf8, true)
            if (newSessionId == null || newSessionId == 0UL) {
                Log.e(TAG, "resetPersistentSession($targetId): failed to create session for empty/missing persistent session")
                return ExternalResetResult.Failed
            }
            return commitResetSnapshot(targetId, newSessionId)
        }

        if (!validateSession(sessionId)) {
            Log.w(TAG, "resetPersistentSession($targetId): session $sessionId no longer valid, deleting and recreating")
            store.update(targetId) { it.copy(sessionId = 0UL) }
            closeSession(sessionId)
            return resetPersistentSession(targetId, text, cursorUtf8, source)
        }

        // #595 一：候选 session 原子交换 — 不原地 reset 旧 session。原地 reset 成功后
        // snapshot 读取失败时旧 Undo/Redo/composition/正文已被 load_text 破坏，无法回滚。
        // 创建 candidate session 装入新正文，读取真实 snapshot：失败则关闭 candidate、
        // 旧 session/store/view 完全不动；成功则原子提交 candidate record 并关闭旧 session。
        val candidateSessionId = createSession(targetId, text, cursorUtf8, true)
        if (candidateSessionId == null || candidateSessionId == 0UL) {
            Log.e(TAG, "resetPersistentSession($targetId): failed to create candidate session — old session preserved")
            return ExternalResetResult.Failed
        }
        return commitResetSnapshot(targetId, candidateSessionId, oldSessionIdToClose = sessionId)
    }

    /**
     * #595 一：原子提交候选 session 的真实 snapshot — 一次性更新 store 记录与活动
     * SessionState 的 text/revision/selection，消除 Rust session（新正文）/
     * SessionStore（旧正文）/ViewModel（新正文+hash）三份状态分裂。
     *
     * 候选 session 原子交换语义：
     * - [sessionId] 是已创建并装入新正文的候选 session（不是被原地 reset 的旧 session）；
     * - [oldSessionIdToClose] 是提交成功后要关闭的旧 session（null 表示无旧 session）；
     * - snapshot 读取失败时关闭候选 session、不关闭旧 session、不推进 store/state，
     *   返回 [ExternalResetResult.Failed] — 旧 session 的 Undo/Redo/composition/正文
     *   完整保留（旧实现原地 reset 旧 session 后 snapshot 失败，旧 session 已被破坏）。
     */
    private fun commitResetSnapshot(
        targetId: String,
        sessionId: ULong,
        oldSessionIdToClose: ULong? = null,
    ): ExternalResetResult {
        val snapshot = querySnapshotForSession(sessionId)
        if (snapshot == null) {
            Log.e(TAG, "commitResetSnapshot($targetId): snapshot read failed — closing candidate $sessionId, old session preserved")
            closeSession(sessionId)
            return ExternalResetResult.Failed
        }
        var pendingRecord: EditorSessionRecord? = null
        updateSessionState { previous ->
            val rec = store.record(targetId)
            pendingRecord = (rec ?: EditorSessionRecord(targetId = targetId, persistent = true)).copy(
                sessionId = sessionId,
                documentState = (rec?.documentState ?: DocumentState()).copy(
                    text = snapshot.text,
                    revision = snapshot.revision,
                    selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                    selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                ),
            )
            if (previous.targetId != targetId) return@updateSessionState previous
            previous.copy(
                sessionId = sessionId,
                text = snapshot.text,
                revision = snapshot.revision,
                selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                origin = EditorSessionOrigin.EXTERNAL_REPLACE,
            )
        }
        pendingRecord?.let { store.put(it) }
        if (oldSessionIdToClose != null && oldSessionIdToClose != 0UL && oldSessionIdToClose != sessionId) {
            closeSession(oldSessionIdToClose)
        }
        val state = windowBindingState
        if (state is WindowBindingState.Detached && state.targetId == targetId) {
            updateSessionState { it.copy(bindingState = WindowBindingState.Detached(targetId, sessionId, snapshot)) }
        }
        return ExternalResetResult.Success(snapshot)
    }

    /**
     * Detached 状态下外部内容重置后，刷新保留的 snapshot，新窗口附着时读到最新状态。
     * #595 五：返回 reset 后的真实 snapshot 供 [resetPersistentSession] 上报。
     */
    private fun refreshDetachedSnapshot(targetId: String): TargetSnapshot? {
        val snapshot = queryTargetSnapshot(targetId)
        val state = windowBindingState
        if (state is WindowBindingState.Detached && state.targetId == targetId) {
            val sid = store.record(targetId)?.sessionId ?: return snapshot
            updateSessionState { it.copy(bindingState = WindowBindingState.Detached(targetId, sid, snapshot)) }
        }
        return snapshot
    }

    // ── SessionCommandPort implementation (bridge-level, no View) ──

    /** 按 sessionId 直接读取真实 snapshot（不依赖持久注册）。 */
    private fun querySnapshotForSession(sessionId: ULong): TargetSnapshot? {
        if (!validateSession(sessionId)) return null
        return when (val result = appServiceBridge.textEditSessionSnapshot(sessionId)) {
            is BridgeResult.Success -> {
                val snap = result.data ?: return null
                TargetSnapshot(
                    text = snap.text,
                    cursorUtf8 = snap.cursor.toInt(),
                    revision = snap.revision.toLong(),
                    selectionAnchorUtf8 = snap.selectionAnchor.toInt(),
                    selectionHeadUtf8 = snap.cursor.toInt()
                )
            }
            else -> null
        }
    }

    override fun queryTargetSnapshot(targetId: String): TargetSnapshot? {
        val sessionId = store.record(targetId)?.sessionId ?: return null
        if (sessionId == 0UL) return null
        return querySnapshotForSession(sessionId)
    }

    /**
     * Bridge 级命令执行（不接触投影/View）— 由窗口层 [EditorWindowHost.applyTargetCommand]
     * 在取得结果后负责应用到活动 View 或非活动投影运行时。
     */
    fun executeTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult {
        val sessionId = store.record(targetId)?.sessionId
            ?: return TargetCommandResult.Failed(TargetCommandError.NO_PERSISTENT_SESSION)
        if (sessionId == 0UL || !validateSession(sessionId)) {
            return TargetCommandResult.Failed(TargetCommandError.SESSION_INVALID)
        }

        val snapshotBefore = queryTargetSnapshot(targetId)
            ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)

        val bridge = TextEditSessionBridge(appServiceBridge, sessionId)
        val dtoResult = when (command) {
            is TargetCommand.Replace -> {
                bridge.replace(
                    command.byteStart, command.byteEndExclusive,
                    command.replacementText, command.originalText,
                    uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC,
                    snapshotBefore.revision
                )
            }
            is TargetCommand.ReplaceAll -> {
                bridge.replaceAll(
                    command.searchText, command.replacementText,
                    snapshotBefore.revision
                )
            }
            is TargetCommand.SetSelection -> {
                bridge.setSelection(
                    command.anchorUtf8, command.headUtf8,
                    snapshotBefore.revision
                )
            }
        }

        if (dtoResult == null) {
            return TargetCommandResult.Failed(TargetCommandError.KERNEL_NULL_RESULT)
        }

        val editResult = com.xiwei.sujian.editor.v2.mirror.EditResult.fromDto(dtoResult)
        if (!editResult.isApplied()) {
            return TargetCommandResult.Failed(TargetCommandError.KERNEL_REJECTED)
        }

        val snapshotAfter = queryTargetSnapshot(targetId)
            ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)

        return TargetCommandResult.Success(snapshotAfter)
    }

    override fun applyTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult =
        executeTargetCommand(targetId, command)

    override fun setTargetDecorations(targetId: String, decorations: TargetDecorations) {
        val existing = store.record(targetId)
        if (existing != null) {
            store.update(targetId) { it.copy(decorations = decorations) }
        } else {
            store.put(EditorSessionRecord(targetId = targetId, decorations = decorations))
        }
        _targetDecorationsVersionFlow.value++
    }

    fun getTargetDecorations(targetId: String): TargetDecorations? = store.record(targetId)?.decorations

    // ── Session lifecycle ──

    private fun createSession(targetId: String, text: String, cursorByteOffset: Int, isPersistent: Boolean): ULong? {
        return when (val result = appServiceBridge.textEditSessionCreate(
            targetId,
            text,
            cursorByteOffset.toUInt(),
            isPersistent
        )) {
            is BridgeResult.Success -> {
                val id = result.data
                if (id == null || id == 0UL) {
                    Log.e(TAG, "createSession($targetId): Core returned null/0 session id")
                    null
                } else {
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(id.toString(), "create")
                    id
                }
            }
            else -> {
                Log.e(TAG, "createSession($targetId): Core session creation failed")
                null
            }
        }
    }

    private fun closeSession(sessionId: ULong) {
        if (sessionId == 0UL) return
        when (appServiceBridge.textEditSessionClose(sessionId)) {
            is BridgeResult.Success -> { }
            else -> { }
        }
    }

    private fun validateSession(sessionId: ULong): Boolean {
        if (sessionId == 0UL) return false
        return appServiceBridge.textEditSessionSnapshot(sessionId) is BridgeResult.Success
    }

    fun releaseHost() {
        if (activeTargetId != null) {
            cancelActiveSession()
        }
        store.allRecords().forEach { record ->
            if (record.sessionId != 0UL) {
                closeSession(record.sessionId)
            }
        }
        store.clear()
        updateSessionState { EditorSessionState(editingState = EditingState.RELEASED) }
    }

    companion object {
        private const val TAG = "EditorSessionCoordinator"
    }
}

data class SessionBindInfo(
    val sessionId: ULong,
    val profile: TextEditorProfile,
    val isPersistent: Boolean,
    /**
     * #592 一：既有持久 session 的真实 Rust snapshot（text/revision/cursor/selection）。
     * 非 null 时窗口层必须走 attachSession（不调用 textEditSessionLoadText），
     * 保证 Undo/Redo 与 composition 不被重置；新建 session 为 null。
     * #595 三：snapshot 为 null 时窗口层拒绝绑定（不允许 fallback 二次 loadText）。
     */
    val snapshot: TargetSnapshot? = null,
)
