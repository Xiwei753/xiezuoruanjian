package com.xiwei.sujian.editor.v2.coordinator

import android.util.Log
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.stateIn
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
 * #592 一/四：会话层协调器 — 只管理 Rust session、正文/选区纯数据快照、
 * Undo/Redo 所属 session、活动目标、窗口绑定状态机与编辑事务。
 *
 * 不持有 View、Activity、Choreographer、WindowDisplayFrameClock、窗口几何、
 * Compose mutableState、TextPaint、TargetDisplayRuntime。
 * 由 Activity 级 ViewModel 持有，跨配置变化存活；窗口/渲染对象全部在
 * [EditorWindowHost]（窗口层）。
 */
class EditorSessionCoordinator(
    private val appServiceBridge: AppServiceBridge,
) : SessionCommandPort {

    // ── 纯会话状态 ──
    private val targetProfiles = mutableMapOf<String, TextEditorProfile>()
    private val targetPersistentFlags = mutableMapOf<String, Boolean>()
    private val persistentSessionIds = mutableMapOf<String, ULong>()
    private val targetDecorations = mutableMapOf<String, TargetDecorations>()
    private val projectionSnapshots = mutableMapOf<String, ProjectionSnapshot>()

    private var activeSessionId: ULong? = null

    // #595 三：_sessionStateFlow 是会话层唯一可写 MutableStateFlow — 所有状态变化
    // 通过 [updateSessionState] 统一推进。activeTargetIdFlow / editingStateFlow /
    // windowBindingStateFlow 从 _sessionStateFlow 派生，不再独立可写。
    private val _sessionStateFlow = MutableStateFlow(EditorSessionState())
    val sessionStateFlow: StateFlow<EditorSessionState> = _sessionStateFlow.asStateFlow()
    val sessionState: EditorSessionState get() = _sessionStateFlow.value

    private val reduceScope = CoroutineScope(SupervisorJob())
    val activeTargetIdFlow: StateFlow<String?> =
        _sessionStateFlow.map { it.activeTargetId }.stateIn(reduceScope, SharingStarted.Eagerly, null)
    val activeTargetId: String? get() = _sessionStateFlow.value.activeTargetId

    val editingStateFlow: StateFlow<EditingState> =
        _sessionStateFlow.map { it.editingState }.stateIn(reduceScope, SharingStarted.Eagerly, EditingState.IDLE)
    val editingState: EditingState get() = _sessionStateFlow.value.editingState

    val windowBindingStateFlow: StateFlow<WindowBindingState> =
        _sessionStateFlow.map { it.bindingState }.stateIn(reduceScope, SharingStarted.Eagerly, WindowBindingState.Idle)
    val windowBindingState: WindowBindingState get() = _sessionStateFlow.value.bindingState

    private val _targetDecorationsVersionFlow = MutableStateFlow(0L)
    val targetDecorationsVersionFlow: StateFlow<Long> = _targetDecorationsVersionFlow.asStateFlow()
    val targetDecorationsVersion: Long get() = _targetDecorationsVersionFlow.value

    private val _lastCommittedTextFlow = MutableStateFlow<String?>(null)
    val lastCommittedTextFlow: StateFlow<String?> = _lastCommittedTextFlow.asStateFlow()
    val lastCommittedText: String? get() = _lastCommittedTextFlow.value

    // #595 七：只保留一个可写事实源 — MutableStateFlow<EditorMotionPolicy>。
    // EditorAnimationSettings 不再单独存储为 StateFlow，改为从 motionPolicy 派生计算。
    private val _motionPolicyFlow = MutableStateFlow(EditorMotionPolicy())
    val motionPolicyFlow: StateFlow<EditorMotionPolicy> = _motionPolicyFlow.asStateFlow()

    // #595 二：全局递增正文版本号源 — 所有 EditorDocumentUpdate 事件产生方调用
    // nextContentVersion() 获取版本号，reducer 据此判断事件新旧。
    private val contentVersionSource = java.util.concurrent.atomic.AtomicLong(0L)
    fun nextContentVersion(): Long = contentVersionSource.incrementAndGet()

    /**
     * #595 三：唯一状态更新入口（reducer）— 所有会话状态变化通过此方法原子推进
     * [_sessionStateFlow]。不得在其他位置直接赋值 [_sessionStateFlow] 或派生 Flow。
     */
    private fun updateSessionState(transform: (EditorSessionState) -> EditorSessionState) {
        _sessionStateFlow.value = transform(_sessionStateFlow.value)
    }

    /**
     * #595 三/七：原子应用 [EditorMotionPolicy] — 唯一可写事实源。
     */
    fun applyMotionPolicy(policy: EditorMotionPolicy) {
        _motionPolicyFlow.value = policy
    }

    fun getMotionPolicy(): EditorMotionPolicy = _motionPolicyFlow.value

    /**
     * #595 一：应用本地 IME/键盘编辑 — 更新唯一 SessionState，不触发 reset。
     *
     * 由 [EditorWindowHost.installContentCallback] 在 SujianEditorView 产生
     * EditResult 后调用。revision/transactionId 来自 Rust EditResult，
     * selectionAnchor/Head 来自 pipeline mirror（真实选区）；
     * WritingPane 收集 [sessionStateFlow] 发现 revision 已应用，只更新保存状态。
     */
    fun applyLocalEdit(update: EditorDocumentUpdate.LocalInput) {
        updateSessionState { previous ->
            EditorSessionState(
                targetId = update.targetId,
                sessionId = persistentSessionIds[update.targetId],
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previous.selectionAnchorUtf8,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previous.selectionHeadUtf8,
                lastAppliedTransactionId = update.transactionId,
                origin = EditorSessionOrigin.LOCAL_INPUT,
                bindingState = previous.bindingState,
                lastRepositoryHash = previous.lastRepositoryHash,
                editingState = previous.editingState,
                activeTargetId = previous.activeTargetId,
                lastAppliedContentVersion = maxOf(previous.lastAppliedContentVersion, update.contentVersion),
            )
        }
    }

    /**
     * #595 一：Repository 加载事件的幂等判断 — 真实 fileHash 与内容双重校验。
     *
     * - 同一 hash 已应用（幂等重放）→ false；
     * - 内容与当前 session 一致 → false（无需 reset）。
     * 不再由 UI 伪造 revision/source 参与判断；本地输入与加载的竞态由
     * ViewModel 的 loading/inputFrozen/sessionId 守卫拦截，加载事件不会
     * 在用户输入进行中落地。
     * #595 二：已删除 shouldApplyExternalReplace/applyExternalReplace —
     * 外部正文只由真实来源事件（RepositoryLoaded 真实 fileHash）驱动，
     * UI 不得再构造 revision+1 的伪造 ExternalReplace。
     */
    fun shouldApplyRepositoryLoad(update: EditorDocumentUpdate.RepositoryLoaded): Boolean {
        if (update.fileHash.isEmpty()) return false
        val current = _sessionStateFlow.value
        if (current.targetId != update.targetId) return true
        // #595 二：旧事件（contentVersion 已应用）跳过
        if (update.contentVersion <= current.lastAppliedContentVersion) return false
        if (current.lastRepositoryHash == update.fileHash && current.text == update.text) return false
        if (current.text == update.text) return false
        return true
    }

    /**
     * #595 二：通用外部更新幂等判断 — 比较targetId、contentVersion、fileHash/text。
     *
     * 适用于 [EditorDocumentUpdate.SyncMerged]、[EditorDocumentUpdate.UndoRestored]、
     * [EditorDocumentUpdate.ProgrammaticReplace]。只有事件属于当前章节且版本更新时才返回 true。
     */
    fun shouldApplyExternalUpdate(update: EditorDocumentUpdate): Boolean {
        val current = _sessionStateFlow.value
        if (current.targetId != update.targetId) return true
        if (update.contentVersion <= current.lastAppliedContentVersion) return false
        if (current.text == update.text && current.revision == update.revision) return false
        return true
    }

    /**
     * #595 一：Repository 加载事件已执行后更新 SessionState —
     * 记录真实 fileHash（幂等去重）并同步真实 snapshot revision。
     */
    fun applyRepositoryLoaded(update: EditorDocumentUpdate.RepositoryLoaded) {
        val realSnapshot = queryTargetSnapshot(update.targetId)
        val realRevision = realSnapshot?.revision ?: update.revision
        val realText = realSnapshot?.text ?: update.text
        val realAnchor = realSnapshot?.selectionAnchorUtf8 ?: 0
        val realHead = realSnapshot?.selectionHeadUtf8
            ?: (realSnapshot?.cursorUtf8 ?: update.text.toByteArray(Charsets.UTF_8).size)
        updateSessionState { previous ->
            EditorSessionState(
                targetId = update.targetId,
                sessionId = persistentSessionIds[update.targetId],
                text = realText,
                revision = realRevision,
                selectionAnchorUtf8 = realAnchor,
                selectionHeadUtf8 = realHead,
                lastAppliedTransactionId = 0L,
                origin = EditorSessionOrigin.EXTERNAL_REPLACE,
                bindingState = previous.bindingState,
                lastRepositoryHash = update.fileHash,
                editingState = previous.editingState,
                activeTargetId = previous.activeTargetId,
                lastAppliedContentVersion = maxOf(previous.lastAppliedContentVersion, update.contentVersion),
            )
        }
    }

    /**
     * #595 二：同步合并事件已执行后更新 SessionState —
     * 记录 manifestRevision 和真实 snapshot。
     */
    fun applySyncMerged(update: EditorDocumentUpdate.SyncMerged) {
        val realSnapshot = queryTargetSnapshot(update.targetId)
        val realRevision = realSnapshot?.revision ?: update.revision
        val realText = realSnapshot?.text ?: update.text
        val realAnchor = realSnapshot?.selectionAnchorUtf8 ?: 0
        val realHead = realSnapshot?.selectionHeadUtf8
            ?: (realSnapshot?.cursorUtf8 ?: update.text.toByteArray(Charsets.UTF_8).size)
        updateSessionState { previous ->
            EditorSessionState(
                targetId = update.targetId,
                sessionId = persistentSessionIds[update.targetId],
                text = realText,
                revision = realRevision,
                selectionAnchorUtf8 = realAnchor,
                selectionHeadUtf8 = realHead,
                lastAppliedTransactionId = 0L,
                origin = EditorSessionOrigin.SYNC_MERGED,
                bindingState = previous.bindingState,
                lastRepositoryHash = update.fileHash,
                editingState = previous.editingState,
                activeTargetId = previous.activeTargetId,
                lastAppliedContentVersion = maxOf(previous.lastAppliedContentVersion, update.contentVersion),
            )
        }
    }

    /**
     * #595 二：撤销/恢复事件已执行后更新 SessionState —
     * revision/transactionId 来自 Rust EditResult，来源标记为 UNDO_RESTORED。
     */
    fun applyUndoRestored(update: EditorDocumentUpdate.UndoRestored) {
        updateSessionState { previous ->
            EditorSessionState(
                targetId = update.targetId,
                sessionId = persistentSessionIds[update.targetId],
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previous.selectionAnchorUtf8,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previous.selectionHeadUtf8,
                lastAppliedTransactionId = update.transactionId,
                origin = EditorSessionOrigin.UNDO_RESTORED,
                bindingState = previous.bindingState,
                lastRepositoryHash = previous.lastRepositoryHash,
                editingState = previous.editingState,
                activeTargetId = previous.activeTargetId,
                lastAppliedContentVersion = maxOf(previous.lastAppliedContentVersion, update.contentVersion),
            )
        }
    }

    /**
     * #595 二：程序化替换事件已执行后更新 SessionState —
     * revision/transactionId 来自 Rust EditResult，来源标记为 PROGRAMMATIC_REPLACE。
     */
    fun applyProgrammaticReplace(update: EditorDocumentUpdate.ProgrammaticReplace) {
        updateSessionState { previous ->
            EditorSessionState(
                targetId = update.targetId,
                sessionId = persistentSessionIds[update.targetId],
                text = update.text,
                revision = update.revision,
                selectionAnchorUtf8 = if (update.selectionAnchorUtf8 >= 0) update.selectionAnchorUtf8 else previous.selectionAnchorUtf8,
                selectionHeadUtf8 = if (update.selectionHeadUtf8 >= 0) update.selectionHeadUtf8 else previous.selectionHeadUtf8,
                lastAppliedTransactionId = update.transactionId,
                origin = EditorSessionOrigin.PROGRAMMATIC_REPLACE,
                bindingState = previous.bindingState,
                lastRepositoryHash = previous.lastRepositoryHash,
                editingState = previous.editingState,
                activeTargetId = previous.activeTargetId,
                lastAppliedContentVersion = maxOf(previous.lastAppliedContentVersion, update.contentVersion),
            )
        }
    }

    /**
     * #595 二：通用外部更新应用 — 根据事件类型分派到具体 apply 方法。
     * 由 WritingPane 在 shouldApplyExternalUpdate 返回 true 后调用。
     */
    fun applyExternalUpdate(update: EditorDocumentUpdate) {
        when (update) {
            is EditorDocumentUpdate.RepositoryLoaded -> applyRepositoryLoaded(update)
            is EditorDocumentUpdate.SyncMerged -> applySyncMerged(update)
            is EditorDocumentUpdate.UndoRestored -> applyUndoRestored(update)
            is EditorDocumentUpdate.ProgrammaticReplace -> applyProgrammaticReplace(update)
            is EditorDocumentUpdate.LocalInput -> applyLocalEdit(update)
        }
    }

    // ── 纯数据目标元数据（窗口层 registerTarget/updateTargetSpec 镜像）──

    fun registerTarget(target: EditableTextTarget) {
        targetProfiles[target.targetId] = target.profile
        targetPersistentFlags[target.targetId] = target.isPersistent
    }

    fun updateTargetSpec(
        targetId: String,
        profile: TextEditorProfile? = null,
    ) {
        profile?.let { targetProfiles[targetId] = it }
    }

    fun isTargetPersistent(targetId: String): Boolean = targetPersistentFlags[targetId] ?: false

    fun getPersistentSessionId(targetId: String): ULong? = persistentSessionIds[targetId]

    // ── 纯数据投影快照（窗口层读写）──

    fun saveProjectionSnapshot(targetId: String, snapshot: ProjectionSnapshot) {
        projectionSnapshots[targetId] = snapshot
    }

    fun getProjectionSnapshot(targetId: String): ProjectionSnapshot? = projectionSnapshots[targetId]

    // ── 窗口绑定状态机 ──

    /**
     * #592 二：Compose onDispose 唯一入口 — 只解除窗口绑定，不关闭持久 Rust session。
     *
     * persistent target：捕获真实 snapshot 并进入 [WindowBindingState.Detached]，
     * Rust session、Undo/Redo、revision、纯数据装饰全部保留，新窗口可自动附着。
     * 非 persistent（草稿）target：关闭临时 session 并回到 Idle。
     *
     * 关闭持久 session 必须由业务事件 [closeTarget] 触发（返回章节列表、切换章节、
     * 删除章节），配置变化不改变 workspace route，因此不会关闭 session。
     */
    fun detachWindowBinding(windowId: String, targetId: String) {
        val isPersistent = targetPersistentFlags[targetId] ?: false
        val sessionId = persistentSessionIds[targetId]
        if (!isPersistent || sessionId == null) {
            // 草稿会话或已无持久会话：直接关闭/清理窗口引用
            if (sessionId != null && sessionId != 0UL) {
                closeSession(sessionId)
                persistentSessionIds.remove(targetId)
            }
            clearWindowAttach(targetId)
            return
        }
        val snapshot = if (validateSession(sessionId)) queryTargetSnapshot(targetId) else null
        if (activeTargetId == targetId) {
            activeSessionId = null
        }
        val detached = WindowBindingState.Detached(targetId, sessionId, snapshot)
        // #595 三/四：通过唯一 reducer 原子推进 bindingState/editingState/activeTargetId。
        updateSessionState { it.copy(
            bindingState = detached,
            editingState = EditingState.IDLE,
            activeTargetId = if (it.activeTargetId == targetId) null else it.activeTargetId,
        ) }
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
            sessionId.toString(), "window_detached"
        )
    }

    /**
     * #592 二：窗口绑定完成（视图已 bind/attach 成功）。
     * 由 [EditorWindowHost] 在 View 真实绑定成功后调用。
     *
     * #595 三：防御性状态守卫 — Attached 只能从 Attaching 进入。
     * View 尚未创建/绑定时（Idle/Detached/Detaching）调用本方法会被拒绝，
     * 保证“Attached 一定表示屏幕上的 View 已绑定 session”。
     */
    fun completeWindowAttach(windowId: String, targetId: String, sessionId: ULong) {
        val current = _sessionStateFlow.value.bindingState
        // 幂等重入：已经是同一 target/session 的 Attached（如 beginEdit 重复调用）保持现状。
        if (current is WindowBindingState.Attached &&
            current.targetId == targetId && current.sessionId == sessionId
        ) {
            return
        }
        // #595 三：Attached 只能从 Attaching 进入 — View 尚未创建/绑定时
        // （Idle/Detached/Detaching）调用会被拒绝，保证“Attached 一定表示
        // 屏幕上的 View 已绑定 session”。
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
     * #592 三：业务级关闭 — 由 workspace 导航事件调用（返回章节列表、切换章节、
     * 删除章节）。与窗口解绑 [detachWindowBinding] 分开：关闭会销毁 Rust session，
     * 解绑只解除窗口引用。
     *
     * 无论处于 Attached/Detached/Idle 都能完整收口状态；Detached 时不再依赖
     * target 对象存在（正文已流式保存），直接关闭 session。
     */
    fun closeTarget(targetId: String, reason: SessionCloseReason) {
        if (activeTargetId == targetId) {
            commitActiveSession(null)
        }
        val sessionId = persistentSessionIds.remove(targetId)
        if (sessionId != null && sessionId != 0UL) {
            closeSession(sessionId)
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
                sessionId.toString(), "close_target:${reason.name.lowercase()}"
            )
        }
        targetDecorations.remove(targetId)
        targetProfiles.remove(targetId)
        targetPersistentFlags.remove(targetId)
        projectionSnapshots.remove(targetId)
        if (activeTargetId == targetId) {
            activeSessionId = null
        }
        // #595 三/四：业务关闭后 SessionState 必须回到 Idle —
        // 不允许残留旧 target/旧 binding/旧 revision。
        updateSessionState { it.copy(
            editingState = EditingState.IDLE,
            bindingState = WindowBindingState.Idle,
            activeTargetId = if (it.activeTargetId == targetId) null else it.activeTargetId,
        ) }
        if (_sessionStateFlow.value.targetId == targetId || sessionId != null) {
            updateSessionState { EditorSessionState() }
        }
    }

    private fun clearWindowAttach(targetId: String) {
        targetDecorations.remove(targetId)
        targetProfiles.remove(targetId)
        targetPersistentFlags.remove(targetId)
        projectionSnapshots.remove(targetId)
        if (activeTargetId == targetId) {
            activeSessionId = null
        }
        updateSessionState { it.copy(
            editingState = EditingState.IDLE,
            bindingState = WindowBindingState.Idle,
            activeTargetId = if (it.activeTargetId == targetId) null else it.activeTargetId,
        ) }
        // #595 四：非持久 target 解绑后 SessionState 回到 Idle。
        if (_sessionStateFlow.value.targetId == targetId) {
            updateSessionState { EditorSessionState() }
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
     * 接收初始正文，是唯一一次 Core 命令），窗口层 attachSnapshot 只重建本地镜像，
     * 不再对同一 session 二次 loadText。
     */
    fun prepareSessionForEdit(targetId: String, initialText: String, initialSelection: Int?, windowId: String): SessionBindInfo? {
        val isPersistent = targetPersistentFlags[targetId] ?: false
        val profile = targetProfiles[targetId] ?: TextEditorProfile()

        if (activeTargetId == targetId && (editingState == EditingState.EDITING || editingState == EditingState.BINDING)) {
            val sid = activeSessionId ?: return null
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
        // #592 一：复用既有持久 session 时携带真实 snapshot（重绑定/窗口重建）；
        // #595 二：新建 session 也在 create 后立即读取真实 snapshot（createSession 已
        // 把初始正文装入 kernel，禁止再走 bindSession/loadText 二次 Core 命令）。
        val sessionId = if (isPersistent) {
            val existing = persistentSessionIds[targetId]
            if (existing != null && validateSession(existing)) {
                existing
            } else {
                persistentSessionIds.remove(targetId)
                if (existing != null) {
                    closeSession(existing)
                }
                createSession(targetId, textForSession, sel, isPersistent)?.also {
                    persistentSessionIds[targetId] = it
                }
            }
        } else {
            createSession(targetId, textForSession, sel, isPersistent)
        }

        if (sessionId == null || sessionId == 0UL) {
            Log.e(TAG, "prepareSessionForEdit($targetId): session creation returned invalid id=$sessionId, aborting")
            persistentSessionIds.remove(targetId)
            updateSessionState { it.copy(
                editingState = EditingState.IDLE,
                bindingState = WindowBindingState.Idle,
            ) }
            return null
        }

        activeSessionId = sessionId
        val attaching = WindowBindingState.Attaching(windowId, targetId, sessionId)

        // #595 一/二/三：通过唯一 reducer 更新 SessionState — 无论新建还是复用、
        // 持久还是草稿，都用真实 snapshot（createSession 已把初始正文装入 kernel，
        // 是唯一一次 Core 命令；草稿 session 未注册 persistentSessionIds，按 sessionId 直读）。
        val snapshot = querySnapshotForSession(sessionId)
        updateSessionState { _ ->
            if (snapshot != null) {
                EditorSessionState(
                    targetId = targetId,
                    sessionId = sessionId,
                    text = snapshot.text,
                    revision = snapshot.revision,
                    selectionAnchorUtf8 = snapshot.selectionAnchorUtf8,
                    selectionHeadUtf8 = snapshot.selectionHeadUtf8,
                    lastAppliedTransactionId = 0L,
                    origin = EditorSessionOrigin.INITIAL_LOAD,
                    bindingState = attaching,
                    editingState = EditingState.BINDING,
                    activeTargetId = targetId,
                )
            } else {
                EditorSessionState(
                    targetId = targetId,
                    sessionId = sessionId,
                    text = textForSession,
                    revision = 0L,
                    selectionAnchorUtf8 = sel,
                    selectionHeadUtf8 = sel,
                    lastAppliedTransactionId = 0L,
                    origin = EditorSessionOrigin.INITIAL_LOAD,
                    bindingState = attaching,
                    editingState = EditingState.BINDING,
                    activeTargetId = targetId,
                )
            }
        }
        return SessionBindInfo(sessionId, profile, isPersistent, snapshot = snapshot)
    }

    fun forceEditingState(state: EditingState) {
        updateSessionState { it.copy(editingState = state) }
    }

    /**
     * 提交活动编辑会话。
     *
     * - Attached：persistent 会话保持打开（软重置语义），由窗口层继续复用。
     * - Detached/非持久：直接关闭 Rust session（正文已通过 onTextChanged 流式保存）。
     * 不依赖 target 对象存在，Detached 状态下也能完整收口。
     */
    fun commitActiveSession(finalText: String?): Boolean {
        val targetId = activeTargetId ?: return false
        val sessionId = activeSessionId ?: return false
        val isPersistent = targetPersistentFlags[targetId] ?: false
        val windowBound = windowBindingState is WindowBindingState.Attached ||
            windowBindingState is WindowBindingState.Attaching

        updateSessionState { it.copy(
            editingState = EditingState.COMMITTING,
            bindingState = if (windowBound) WindowBindingState.Committing(targetId, sessionId) else it.bindingState,
        ) }
        // #595 四：正文在 SessionState 中（applyLocalEdit 已更新），不再维护第二份正文缓存。
        if (!isPersistent || !windowBound) {
            closeSession(sessionId)
            persistentSessionIds.remove(targetId)
        }
        activeSessionId = null
        // #595 三/四：提交清除后 SessionState 必须回到 Idle — 不允许残留旧 target/旧 binding。
        updateSessionState { EditorSessionState() }
        _lastCommittedTextFlow.value = if (targetProfiles[targetId]?.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) null else finalText
        return true
    }

    /**
     * 取消活动编辑会话 — Detached 状态下同样完整收口（关闭 session，不依赖 target 对象）。
     */
    fun cancelActiveSession(): Boolean {
        val targetId = activeTargetId ?: return false
        val sessionId = activeSessionId ?: return false
        val windowBound = windowBindingState is WindowBindingState.Attached ||
            windowBindingState is WindowBindingState.Attaching

        updateSessionState { it.copy(
            editingState = EditingState.CANCELLING,
            bindingState = if (windowBound) WindowBindingState.Cancelling(targetId, sessionId) else it.bindingState,
        ) }
        closeSession(sessionId)
        persistentSessionIds.remove(targetId)
        activeSessionId = null
        // #595 三/四：取消清除后 SessionState 必须回到 Idle。
        updateSessionState { EditorSessionState() }
        return true
    }

    fun resetPersistentSession(targetId: String, text: String, cursorUtf8: Int, source: SessionResetSource = SessionResetSource.EXTERNAL) {
        if (source == SessionResetSource.LOCAL_CONTENT_CHANGED) return
        if (targetPersistentFlags[targetId] != true) return

        val sessionId = persistentSessionIds[targetId]
        if (sessionId == null) {
            val newSessionId = createSession(targetId, text, cursorUtf8, true)
            if (newSessionId == null || newSessionId == 0UL) {
                Log.e(TAG, "resetPersistentSession($targetId): failed to create session for empty/missing persistent session")
                return
            }
            persistentSessionIds[targetId] = newSessionId
            if (targetId == activeTargetId) {
                activeSessionId = newSessionId
            }
            refreshDetachedSnapshot(targetId)
            return
        }

        if (!validateSession(sessionId)) {
            Log.w(TAG, "resetPersistentSession($targetId): session $sessionId no longer valid, deleting and recreating")
            persistentSessionIds.remove(targetId)
            closeSession(sessionId)
            resetPersistentSession(targetId, text, cursorUtf8, source)
            return
        }

        when (appServiceBridge.textEditSessionReset(sessionId, text, cursorUtf8.toUInt())) {
            is BridgeResult.Success -> { }
            else -> { }
        }
        refreshDetachedSnapshot(targetId)
    }

    /** Detached 状态下外部内容重置后，刷新保留的 snapshot，新窗口附着时读到最新状态。 */
    private fun refreshDetachedSnapshot(targetId: String) {
        val state = windowBindingState
        if (state is WindowBindingState.Detached && state.targetId == targetId) {
            val sid = persistentSessionIds[targetId] ?: return
            val snapshot = if (validateSession(sid)) queryTargetSnapshot(targetId) else null
            updateSessionState { it.copy(bindingState = WindowBindingState.Detached(targetId, sid, snapshot)) }
        }
    }

    // ── SessionCommandPort implementation (bridge-level, no View) ──

    /** 按 sessionId 直接读取真实 snapshot（不依赖 persistentSessionIds 注册）。 */
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
        val sessionId = persistentSessionIds[targetId] ?: return null
        return querySnapshotForSession(sessionId)
    }

    /**
     * Bridge 级命令执行（不接触投影/View）— 由窗口层 [EditorWindowHost.applyTargetCommand]
     * 在取得结果后负责应用到活动 View 或非活动投影运行时。
     */
    fun executeTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult {
        val sessionId = persistentSessionIds[targetId]
            ?: return TargetCommandResult.Failed(TargetCommandError.NO_PERSISTENT_SESSION)
        if (!validateSession(sessionId)) {
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
        targetDecorations[targetId] = decorations
        _targetDecorationsVersionFlow.value++
    }

    fun getTargetDecorations(targetId: String): TargetDecorations? = targetDecorations[targetId]

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
        persistentSessionIds.values.forEach { sessionId ->
            closeSession(sessionId)
        }
        persistentSessionIds.clear()
        targetDecorations.clear()
        targetProfiles.clear()
        targetPersistentFlags.clear()
        projectionSnapshots.clear()
        activeSessionId = null
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
