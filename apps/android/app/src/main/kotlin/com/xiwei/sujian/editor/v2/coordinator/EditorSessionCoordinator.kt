package com.xiwei.sujian.editor.v2.coordinator

import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge

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
 */
data class ProjectionSnapshot(
    val scrollX: Float = 0f,
    val scrollY: Float = 0f,
    val viewportWidth: Float = 0f,
    val viewportHeight: Float = 0f,
    val fontSizePx: Float = 0f,
    val lineSpacingMultiplier: Float = 0f,
    val themeColors: ThemeColorsSnapshot? = null,
)

/**
 * #592 一/四：会话层协调器 — 只管理 Rust session、正文/选区纯数据快照、
 * Undo/Redo 所属 session、活动目标、窗口绑定状态机与编辑事务。
 *
 * 不持有 View、Activity、Choreographer、WindowDisplayFrameClock、窗口几何、
 * Compose mutableState 回调、TextPaint、TargetDisplayRuntime。
 * 由 Activity 级 ViewModel 持有，跨配置变化存活；窗口/渲染对象全部在
 * [EditorWindowHost]（窗口层）。
 */
class EditorSessionCoordinator(
    private val appServiceBridge: AppServiceBridge,
) : SessionCommandPort {

    // ── 纯会话状态 ──
    private val targetProfiles = mutableMapOf<String, TextEditorProfile>()
    private val targetPersistentFlags = mutableMapOf<String, Boolean>()
    private val targetTexts = mutableMapOf<String, String>()
    private val persistentSessionIds = mutableMapOf<String, ULong>()
    private val targetDecorations = mutableMapOf<String, TargetDecorations>()
    private val projectionSnapshots = mutableMapOf<String, ProjectionSnapshot>()

    private var activeSessionId: ULong? = null

    var activeTargetId: String? by mutableStateOf(null)
        private set
    var editingState: EditingState by mutableStateOf(EditingState.IDLE)
        private set
    var windowBindingState: WindowBindingState by mutableStateOf(WindowBindingState.Idle)
        private set

    var targetDecorationsVersion by mutableStateOf(0L)
        private set

    var lastCommittedText: String? by mutableStateOf(null)
        private set

    private var editorAnimationSettings: EditorAnimationSettings = EditorAnimationSettings()

    fun getEditorAnimationSettings(): EditorAnimationSettings = editorAnimationSettings

    fun setEditorAnimationSettings(settings: EditorAnimationSettings) {
        editorAnimationSettings = settings
    }

    // ── 纯数据目标元数据（窗口层 registerTarget/updateTargetSpec 镜像）──

    fun registerTarget(target: EditableTextTarget) {
        targetProfiles[target.targetId] = target.profile
        targetPersistentFlags[target.targetId] = target.isPersistent
        targetTexts[target.targetId] = target.currentText
    }

    fun updateTargetSpec(
        targetId: String,
        profile: TextEditorProfile? = null,
        currentText: String? = null
    ) {
        profile?.let { targetProfiles[targetId] = it }
        currentText?.let { targetTexts[targetId] = it }
    }

    fun updateTargetText(targetId: String, text: String) {
        targetTexts[targetId] = text
    }

    fun getTargetText(targetId: String): String? = targetTexts[targetId]

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
        windowBindingState = WindowBindingState.Detaching(snapshot)
        if (activeTargetId == targetId) {
            activeTargetId = null
            activeSessionId = null
        }
        editingState = EditingState.IDLE
        windowBindingState = WindowBindingState.Detached(targetId, sessionId, snapshot)
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(
            sessionId.toString(), "window_detached"
        )
    }

    /**
     * #592 二：窗口绑定完成（视图已 bind/attach 成功）。
     * 由 [EditorWindowHost.beginEdit] 在视图绑定后调用。
     */
    fun completeWindowAttach(windowId: String, targetId: String, sessionId: ULong) {
        windowBindingState = WindowBindingState.Attached(windowId, targetId, sessionId)
        editingState = EditingState.EDITING
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
        targetTexts.remove(targetId)
        projectionSnapshots.remove(targetId)
        if (activeTargetId == targetId) {
            activeTargetId = null
            activeSessionId = null
        }
        editingState = EditingState.IDLE
        windowBindingState = WindowBindingState.Idle
    }

    private fun clearWindowAttach(targetId: String) {
        targetDecorations.remove(targetId)
        targetProfiles.remove(targetId)
        targetPersistentFlags.remove(targetId)
        targetTexts.remove(targetId)
        projectionSnapshots.remove(targetId)
        if (activeTargetId == targetId) {
            activeTargetId = null
            activeSessionId = null
        }
        editingState = EditingState.IDLE
        windowBindingState = WindowBindingState.Idle
    }

    /**
     * 准备会话绑定 — 创建/复用 session 并设置活动状态。
     * 返回绑定信息或 null（失败时）。
     *
     * #592 一：复用既有持久 session 时，绑定信息携带 Rust 的真实
     * textEditSessionSnapshot（text/revision/cursor/selection），窗口层据此执行
     * attachSnapshot，不再用新 Compose target 的正文/末尾光标执行 loadText
     * （那会 revision+1 并清空 Undo/Redo）。
     */
    fun prepareSessionForEdit(targetId: String, initialSelection: Int?, windowId: String): SessionBindInfo? {
        val isPersistent = targetPersistentFlags[targetId] ?: false
        val targetText = targetTexts[targetId] ?: ""
        val profile = targetProfiles[targetId] ?: TextEditorProfile()

        if (activeTargetId == targetId && (editingState == EditingState.EDITING || editingState == EditingState.BINDING)) {
            val sid = activeSessionId ?: return null
            return SessionBindInfo(sid, targetText, initialSelection ?: targetText.toByteArray(Charsets.UTF_8).size, profile, isPersistent, snapshot = queryTargetSnapshot(targetId))
        }

        if (activeTargetId != null && activeTargetId != targetId) {
            editingState = EditingState.REBINDING
            if (!commitActiveSession(null)) {
                cancelActiveSession()
            }
        }

        editingState = EditingState.BINDING

        val textForSession = targetText
        val sel = initialSelection ?: textForSession.toByteArray(Charsets.UTF_8).size
        // #592 一：仅对调用前已存在的持久 session 携带真实 snapshot（重绑定/窗口重建）；
        // 新建 session 走正常 bindSession/loadText 路径。
        var reusedExistingSession = false
        val sessionId = if (isPersistent) {
            val existing = persistentSessionIds[targetId]
            if (existing != null && validateSession(existing)) {
                reusedExistingSession = true
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
            editingState = EditingState.IDLE
            windowBindingState = WindowBindingState.Idle
            return null
        }

        activeTargetId = targetId
        activeSessionId = sessionId
        windowBindingState = WindowBindingState.Attaching(windowId, targetId, sessionId)

        // #592 一：复用既有持久 session 时携带真实 snapshot；新建 session 无 snapshot。
        val snapshot = if (reusedExistingSession) queryTargetSnapshot(targetId) else null
        return SessionBindInfo(sessionId, textForSession, sel, profile, isPersistent, snapshot = snapshot)
    }

    fun forceEditingState(state: EditingState) {
        editingState = state
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

        editingState = EditingState.COMMITTING
        if (windowBound) {
            windowBindingState = WindowBindingState.Committing(targetId, sessionId)
        }
        if (finalText != null) {
            targetTexts[targetId] = finalText
        }
        if (!isPersistent || !windowBound) {
            closeSession(sessionId)
            persistentSessionIds.remove(targetId)
        }
        activeTargetId = null
        activeSessionId = null
        editingState = EditingState.IDLE
        windowBindingState = WindowBindingState.Idle
        lastCommittedText = if (targetProfiles[targetId]?.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) null else finalText
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

        editingState = EditingState.CANCELLING
        if (windowBound) {
            windowBindingState = WindowBindingState.Cancelling(targetId, sessionId)
        }
        closeSession(sessionId)
        persistentSessionIds.remove(targetId)
        activeTargetId = null
        activeSessionId = null
        editingState = EditingState.IDLE
        windowBindingState = WindowBindingState.Idle
        return true
    }

    fun resetPersistentSession(targetId: String, text: String, cursorUtf8: Int, source: SessionResetSource = SessionResetSource.EXTERNAL) {
        if (source == SessionResetSource.LOCAL_CONTENT_CHANGED) return
        if (targetPersistentFlags[targetId] != true) return

        val sessionId = persistentSessionIds[targetId]
        if (sessionId == null) {
            targetTexts[targetId] = text
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
            is BridgeResult.Success -> {
                targetTexts[targetId] = text
            }
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
            windowBindingState = WindowBindingState.Detached(targetId, sid, snapshot)
        }
    }

    // ── SessionCommandPort implementation (bridge-level, no View) ──

    override fun queryTargetSnapshot(targetId: String): TargetSnapshot? {
        val sessionId = persistentSessionIds[targetId] ?: return null
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

        targetTexts[targetId] = snapshotAfter.text
        return TargetCommandResult.Success(snapshotAfter)
    }

    override fun applyTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult =
        executeTargetCommand(targetId, command)

    override fun setTargetDecorations(targetId: String, decorations: TargetDecorations) {
        targetDecorations[targetId] = decorations
        targetDecorationsVersion++
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
        targetTexts.clear()
        projectionSnapshots.clear()
        editingState = EditingState.RELEASED
        windowBindingState = WindowBindingState.Idle
        activeTargetId = null
        activeSessionId = null
    }

    companion object {
        private const val TAG = "EditorSessionCoordinator"
    }
}

data class SessionBindInfo(
    val sessionId: ULong,
    val text: String,
    val selection: Int,
    val profile: TextEditorProfile,
    val isPersistent: Boolean,
    /**
     * #592 一：既有持久 session 的真实 Rust snapshot（text/revision/cursor/selection）。
     * 非 null 时窗口层必须走 attachSession（不调用 textEditSessionLoadText），
     * 保证 Undo/Redo 与 composition 不被重置；新建 session 为 null。
     */
    val snapshot: TargetSnapshot? = null,
)

data class ThemeColorsSnapshot(
    val text: Int,
    val cursor: Int,
    val selection: Int,
    val composing: Int,
    val background: Int,
    val searchHighlight: Int,
)
