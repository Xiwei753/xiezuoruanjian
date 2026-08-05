package com.xiwei.sujian.editor.v2.coordinator

import android.content.Context
import android.graphics.Rect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge

/**
 * #592 一：窗口层宿主 — 每个 Activity/窗口创建一份，只管理 SujianEditorView、
 * WindowDisplayFrameClock、IME 与焦点、窗口几何和变换、当前窗口的目标回调。
 *
 * 窗口销毁时调用 [releaseWindow] 完整释放 View 和 FrameClock，但保留 Rust 会话；
 * 新窗口创建后从 [EditorSessionCoordinator] 读取活动 session 和目标快照并重新绑定。
 *
 * 不得持有自建 CoroutineScope、Context（除 application context）、仓库、同步 I/O。
 */
class EditorWindowHost(
    private val context: Context,
    val sessionCoordinator: EditorSessionCoordinator,
    private val appServiceBridge: AppServiceBridge,
    private val animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource,
    private val transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource,
    frameClock: WindowDisplayFrameClock? = null,
) : SessionCommandPort {

    private var sharedEditorView: SujianEditorView? = null
    val windowFrameClock: WindowDisplayFrameClock = frameClock ?: WindowDisplayFrameClock()

    var activeTargetGeometry: Rect by mutableStateOf(Rect())
        private set
    var activeTargetTransform: Transform2D by mutableStateOf(Transform2D.IDENTITY)
        private set

    // ── Delegated session-level state ──

    val activeTargetId: String? get() = sessionCoordinator.activeTargetId
    val editingState: EditingState get() = sessionCoordinator.editingState
    val targetDecorationsVersion: Long get() = sessionCoordinator.targetDecorationsVersion
    val lastCommittedText: String? get() = sessionCoordinator.lastCommittedText
    var onTargetContentChanged: ((targetId: String, newText: String) -> Unit)?
        get() = sessionCoordinator.onTargetContentChanged
        set(value) { sessionCoordinator.onTargetContentChanged = value }

    // ── Target management (delegated) ──

    fun registerTarget(target: EditableTextTarget) = sessionCoordinator.registerTarget(target)

    fun updateTargetSpec(
        targetId: String,
        onTextChanged: ((String) -> Unit)? = null,
        onCommit: ((String) -> Unit)? = null,
        onCancel: (() -> Unit)? = null,
        onEditingStateChanged: ((EditingState) -> Unit)? = null,
        profile: TextEditorProfile? = null,
        currentText: String? = null
    ) = sessionCoordinator.updateTargetSpec(targetId, onTextChanged, onCommit, onCancel, onEditingStateChanged, profile, currentText)

    fun unregisterTarget(targetId: String) = sessionCoordinator.unregisterTarget(targetId)

    fun updateTargetGeometry(targetId: String, geometry: Rect) {
        sessionCoordinator.getTarget(targetId)?.updateGeometry(geometry)
        if (targetId == activeTargetId) {
            activeTargetGeometry = geometry
        }
    }

    fun updateTargetTransform(targetId: String, transform: Transform2D) {
        sessionCoordinator.getTarget(targetId)?.updateTransform(transform)
        if (targetId == activeTargetId) {
            activeTargetTransform = transform
        }
    }

    fun updateTargetText(targetId: String, text: String) = sessionCoordinator.updateTargetText(targetId, text)

    fun getTargetGeometry(targetId: String): Rect? = sessionCoordinator.getTargetGeometry(targetId)

    fun getTargetText(targetId: String): String? = sessionCoordinator.getTargetText(targetId)

    fun getPersistentSessionId(targetId: String): ULong? = sessionCoordinator.getPersistentSessionId(targetId)

    fun getTargetProjection(targetId: String) = sessionCoordinator.getTargetProjection(targetId)

    fun getEditorAnimationSettings(): EditorAnimationSettings = sessionCoordinator.getEditorAnimationSettings()

    fun setEditorAnimationSettings(settings: EditorAnimationSettings) {
        sessionCoordinator.setEditorAnimationSettings(settings)
        sharedEditorView?.let { view ->
            view.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
            view.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
        }
    }

    // ── Edit operations (orchestrates session + window) ──

    fun beginEdit(targetId: String, initialSelection: Int? = null): Boolean {
        saveActiveTargetProjection()
        clearActiveCallbacks()
        val bindInfo = sessionCoordinator.prepareSessionForEdit(targetId, initialSelection) ?: return false

        val target = sessionCoordinator.getTarget(targetId) ?: return false
        activeTargetGeometry = target.currentGeometry
        activeTargetTransform = target.currentTransform

        val view = getOrCreateEditorView()
        val bridge = TextEditSessionBridge(appServiceBridge, bindInfo.sessionId)
        view.bindSession(bridge, bindInfo.profile, bindInfo.text, bindInfo.selection)

        installContentCallback(view, target)
        installCommitRequestedCallback(view)
        installCancelRequestedCallback(view)

        val geometry = target.currentGeometry
        if (geometry.width() > 0 && geometry.height() > 0) {
            activeTargetGeometry = geometry
        }

        sessionCoordinator.forceEditingState(EditingState.EDITING)

        view.post {
            view.requestFocus()
            val imm = context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as? android.view.inputmethod.InputMethodManager
            imm?.showSoftInput(view, 0)
        }

        return true
    }

    fun commitActiveEdit(): Boolean {
        clearActiveCallbacks()
        val targetId = activeTargetId ?: return false
        val isPersistent = sessionCoordinator.isTargetPersistent(targetId)
        val view = sharedEditorView
        var finalText: String? = null
        if (view != null) {
            finalText = view.getText()
            if (isPersistent) {
                view.softResetForPersistentCommit()
            } else {
                view.unbindSession("commit")
            }
        }
        return sessionCoordinator.commitActiveSession(finalText).also {
            if (it) {
                activeTargetGeometry = Rect()
                activeTargetTransform = Transform2D.IDENTITY
            }
        }
    }

    fun cancelActiveEdit(): Boolean {
        clearActiveCallbacks()
        sharedEditorView?.let { view ->
            view.unbindSession("cancel")
        }
        return sessionCoordinator.cancelActiveSession().also {
            if (it) {
                activeTargetGeometry = Rect()
                activeTargetTransform = Transform2D.IDENTITY
            }
        }
    }

    fun resetPersistentSession(targetId: String, text: String, cursorUtf8: Int, source: SessionResetSource = SessionResetSource.EXTERNAL) {
        sessionCoordinator.resetPersistentSession(targetId, text, cursorUtf8, source)
        if (targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null) {
                view.loadText(text, cursorUtf8)
            }
        }
    }

    // ── SessionCommandPort (uses view pipeline when available) ──

    override fun queryTargetSnapshot(targetId: String): TargetSnapshot? = sessionCoordinator.queryTargetSnapshot(targetId)

    override fun applyTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult {
        if (targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null) {
                val snapshotBefore = sessionCoordinator.queryTargetSnapshot(targetId)
                    ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)
                val commandPort = view.getPipeline()
                when (command) {
                    is TargetCommand.Replace -> {
                        val pipelineOutput = commandPort.replaceRangeTyped(
                            command.byteStart, command.byteEndExclusive,
                            command.replacementText, command.originalText,
                            uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC
                        )
                        view.handlePipelineOutput(pipelineOutput)
                    }
                    is TargetCommand.ReplaceAll -> {
                        view.replaceAll(command.searchText, command.replacementText)
                    }
                    is TargetCommand.SetSelection -> {
                        val pipelineOutput = commandPort.setSelectionTyped(command.anchorUtf8, command.headUtf8)
                        view.handlePipelineOutput(pipelineOutput)
                    }
                }
                val snapshotAfter = sessionCoordinator.queryTargetSnapshot(targetId)
                    ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)
                sessionCoordinator.updateTargetText(targetId, snapshotAfter.text)
                return TargetCommandResult.Success(snapshotAfter)
            }
        }
        return sessionCoordinator.applyTargetCommand(targetId, command)
    }

    override fun setTargetDecorations(targetId: String, decorations: TargetDecorations) {
        sessionCoordinator.setTargetDecorations(targetId, decorations)
        if (targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null) {
                if (decorations.searchHighlightsUtf8.isNotEmpty()) {
                    view.setSearchHighlights(decorations.searchHighlightsUtf8)
                } else {
                    view.clearSearchHighlights()
                }
                if (decorations.selectionStartUtf8 >= 0 && decorations.selectionEndUtf8 >= 0) {
                    view.setSelectionRange(decorations.selectionStartUtf8, decorations.selectionEndUtf8)
                }
            }
        }
    }

    // ── View management ──

    fun getSharedEditorView(): SujianEditorView? = sharedEditorView

    fun obtainSharedEditorView(): SujianEditorView = getOrCreateEditorView()

    fun setSharedEditorView(view: SujianEditorView) {
        sharedEditorView = view
    }

    fun updateHostGeometry(width: Float, height: Float) {
        sharedEditorView?.updateHostGeometry(width, height)
    }

    // ── Window lifecycle ──

    /**
     * #592 二：窗口销毁时完整释放 View 和 FrameClock，但保留 Rust 会话。
     * activeTargetId 和 activeSessionId 由会话协调层统一维护，
     * 不再出现"有活动 session 但没有窗口绑定"的不可描述状态。
     */
    fun releaseWindow() {
        clearActiveCallbacks()
        sharedEditorView?.let { view ->
            view.unbindSession("config_change")
            view.setFrameClock(null)
            view.release()
        }
        sharedEditorView = null
        windowFrameClock.release()
        activeTargetGeometry = Rect()
        activeTargetTransform = Transform2D.IDENTITY
    }

    /**
     * Activity 永久结束 — 释放窗口和全部会话。
     */
    fun releaseHost() {
        clearActiveCallbacks()
        if (activeTargetId != null) {
            sharedEditorView?.let { view ->
                view.unbindSession("release")
            }
        }
        sharedEditorView?.let { view ->
            view.setFrameClock(null)
            view.release()
        }
        sharedEditorView = null
        windowFrameClock.release()
        sessionCoordinator.releaseHost()
        activeTargetGeometry = Rect()
        activeTargetTransform = Transform2D.IDENTITY
    }

    // ── Private helpers ──

    private fun installContentCallback(view: SujianEditorView, target: EditableTextTarget) {
        view.onContentChanged = { newText ->
            target.onTextChanged?.invoke(newText)
        }
        view.onSearchHighlightsCleared = {
            sessionCoordinator.setTargetDecorations(target.targetId, TargetDecorations())
        }
    }

    private fun installCommitRequestedCallback(view: SujianEditorView) {
        view.onCommitRequested = {
            commitActiveEdit()
        }
    }

    private fun installCancelRequestedCallback(view: SujianEditorView) {
        view.onCancelRequested = {
            cancelActiveEdit()
        }
    }

    private fun clearActiveCallbacks() {
        sharedEditorView?.let { view ->
            view.onContentChanged = null
            view.onCommitRequested = null
            view.onCancelRequested = null
            view.onSearchHighlightsCleared = null
        }
    }

    private fun saveActiveTargetProjection() {
        val targetId = activeTargetId ?: return
        if (!sessionCoordinator.isTargetPersistent(targetId)) return
        val view = sharedEditorView ?: return
        val themeColors = view.getPipelineThemeColors()
        val themeSnapshot = if (themeColors != null) {
            ThemeColorsSnapshot(
                themeColors.text, themeColors.cursor, themeColors.selection,
                themeColors.composing, themeColors.background, themeColors.searchHighlight
            )
        } else null
        sessionCoordinator.saveActiveTargetProjection(
            view.getScrollXPos(), view.getScrollYPos(),
            view.width, view.height,
            view.getPipelineTextPaintSize(),
            view.getPipelineLineSpacingMultiplier(),
            themeSnapshot,
        )
    }

    private fun getOrCreateEditorView(): SujianEditorView {
        return sharedEditorView ?: SujianEditorView(context, animationTimeSource = animationTimeSource, transactionIdSource = transactionIdSource).also {
            it.setFrameClock(windowFrameClock)
            val settings = sessionCoordinator.getEditorAnimationSettings()
            it.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
            it.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
            sharedEditorView = it
        }
    }
}
