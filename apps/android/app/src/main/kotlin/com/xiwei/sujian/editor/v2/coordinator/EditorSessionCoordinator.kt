package com.xiwei.sujian.editor.v2.coordinator

import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge
import com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

/**
 * #592 一：会话层协调器 — 只管理 Rust session、正文状态、Undo/Redo、活动目标、
 * 投影纯数据状态、编辑事务与 revision。
 *
 * 不得持有 View、Activity、Choreographer、WindowDisplayFrameClock、窗口几何回调、Compose lambda。
 * 由 Activity 级 ViewModel 持有，跨配置变化存活。
 */
class EditorSessionCoordinator(
    private val appServiceBridge: AppServiceBridge,
    private val animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource,
    private val transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource,
) : SessionCommandPort {

    private val targets = mutableMapOf<String, EditableTextTarget>()
    private var activeSessionId: ULong? = null
    private val persistentSessionIds = mutableMapOf<String, ULong>()
    private val targetProjections = mutableMapOf<String, TargetDisplayRuntime>()

    var targetDecorationsVersion by mutableStateOf(0L)
        private set
    private val targetDecorations = mutableMapOf<String, TargetDecorations>()

    var activeTargetId: String? by mutableStateOf(null)
        private set
    var editingState: EditingState by mutableStateOf(EditingState.IDLE)
        private set

    var onTargetContentChanged: ((targetId: String, newText: String) -> Unit)? = null

    private var editorAnimationSettings: EditorAnimationSettings = EditorAnimationSettings()

    var lastCommittedText: String? by mutableStateOf(null)
        private set

    fun getEditorAnimationSettings(): EditorAnimationSettings = editorAnimationSettings

    fun setEditorAnimationSettings(settings: EditorAnimationSettings) {
        editorAnimationSettings = settings
    }

    fun registerTarget(target: EditableTextTarget) {
        targets[target.targetId] = target
    }

    fun updateTargetSpec(
        targetId: String,
        onTextChanged: ((String) -> Unit)? = null,
        onCommit: ((String) -> Unit)? = null,
        onCancel: (() -> Unit)? = null,
        onEditingStateChanged: ((EditingState) -> Unit)? = null,
        profile: TextEditorProfile? = null,
        currentText: String? = null
    ) {
        val target = targets[targetId] ?: return
        onTextChanged?.let { target.onTextChanged = it }
        onCommit?.let { target.onCommit = it }
        onCancel?.let { target.onCancel = it }
        onEditingStateChanged?.let { target.onEditingStateChanged = it }
        profile?.let { target.updateProfile(it) }
        currentText?.let { target.updateText(it) }
    }

    fun unregisterTarget(targetId: String) {
        if (activeTargetId == targetId && editingState != EditingState.COMMITTING && editingState != EditingState.CANCELLING) {
            cancelActiveSession()
        }
        val persistentSessionId = persistentSessionIds.remove(targetId)
        if (persistentSessionId != null) {
            closeSession(persistentSessionId)
        }
        targetProjections.remove(targetId)?.release()
        targetDecorations.remove(targetId)
        targets.remove(targetId)
    }

    /**
     * #592 一：窗口配置变化时解除窗口绑定，不关闭持久 Rust session。
     *
     * persistent target：移除窗口层 target 引用和投影 runtime，保留 Rust session、
     * 活动会话状态和纯数据装饰，使新窗口可复用同一 session 跨配置变化存活。
     * 非 persistent target：回退到 unregisterTarget，关闭临时 session。
     *
     * 由 Compose onDispose 调用；关闭持久 session 必须由明确业务事件
     * （章节关闭、永久删除、onCleared）触发，不得由配置变化触发。
     */
    fun detachTarget(targetId: String) {
        val target = targets[targetId]
        if (target != null && !target.isPersistent) {
            unregisterTarget(targetId)
            return
        }
        targetProjections.remove(targetId)?.release()
        targets.remove(targetId)
    }

    fun updateTargetText(targetId: String, text: String) {
        targets[targetId]?.updateText(text)
    }

    fun getTargetGeometry(targetId: String): android.graphics.Rect? = targets[targetId]?.currentGeometry

    fun getTargetText(targetId: String): String? = targets[targetId]?.currentText

    fun getPersistentSessionId(targetId: String): ULong? = persistentSessionIds[targetId]

    fun isTargetPersistent(targetId: String): Boolean = targets[targetId]?.isPersistent ?: false

    fun getTarget(targetId: String): EditableTextTarget? = targets[targetId]

    /**
     * 准备会话绑定 — 处理重绑定逻辑（提交/取消旧目标），创建/复用 session，
     * 设置活动状态。返回绑定信息或 null（失败时）。
     */
    fun prepareSessionForEdit(targetId: String, initialSelection: Int?): SessionBindInfo? {
        val target = targets[targetId] ?: return null
        if (activeTargetId == targetId && (editingState == EditingState.EDITING || editingState == EditingState.BINDING)) {
            val sid = activeSessionId ?: return null
            return SessionBindInfo(sid, target.currentText, initialSelection ?: target.currentText.toByteArray(Charsets.UTF_8).size, target.profile, target.isPersistent)
        }

        if (activeTargetId != null && activeTargetId != targetId) {
            val oldTarget = targets[activeTargetId]
            editingState = EditingState.REBINDING
            oldTarget?.onEditingStateChanged?.invoke(EditingState.REBINDING)
            if (!commitActiveSessionInternal(null)) {
                cancelActiveSessionInternal()
            }
        }

        editingState = EditingState.BINDING
        target.onEditingStateChanged?.invoke(EditingState.BINDING)

        val textForSession = target.currentText
        val sel = initialSelection ?: textForSession.toByteArray(Charsets.UTF_8).size
        val sessionId = if (target.isPersistent) {
            val existing = persistentSessionIds[targetId]
            if (existing != null && validateSession(existing)) {
                existing
            } else {
                persistentSessionIds.remove(targetId)
                if (existing != null) {
                    closeSession(existing)
                }
                createSession(target, textForSession, sel)?.also {
                    persistentSessionIds[targetId] = it
                }
            }
        } else {
            createSession(target, textForSession, sel)
        }

        if (sessionId == null || sessionId == 0UL) {
            Log.e(TAG, "prepareSessionForEdit($targetId): session creation returned invalid id=$sessionId, aborting")
            persistentSessionIds.remove(targetId)
            editingState = EditingState.IDLE
            target.onEditingStateChanged?.invoke(EditingState.IDLE)
            return null
        }

        activeTargetId = targetId
        activeSessionId = sessionId

        return SessionBindInfo(sessionId, textForSession, sel, target.profile, target.isPersistent)
    }

    fun forceEditingState(state: EditingState) {
        editingState = state
        val targetId = activeTargetId
        if (targetId != null) {
            targets[targetId]?.onEditingStateChanged?.invoke(state)
        }
    }

    fun commitActiveSession(finalText: String?): Boolean {
        return commitActiveSessionInternal(finalText)
    }

    private fun commitActiveSessionInternal(finalText: String?): Boolean {
        val targetId = activeTargetId ?: return false
        val target = targets[targetId] ?: return false
        val sessionId = activeSessionId ?: return false

        editingState = EditingState.COMMITTING
        target.onEditingStateChanged?.invoke(EditingState.COMMITTING)

        if (finalText != null) {
            target.onCommit?.invoke(finalText)
            target.updateText(finalText)
        }

        if (!target.isPersistent) {
            closeSession(sessionId)
            persistentSessionIds.remove(targetId)
            if (target.profile.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) {
                target.updateText("")
            }
        }

        activeTargetId = null
        activeSessionId = null

        editingState = EditingState.IDLE
        target.onEditingStateChanged?.invoke(EditingState.IDLE)
        lastCommittedText = if (target.profile.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) null else finalText
        return true
    }

    fun cancelActiveSession(): Boolean {
        return cancelActiveSessionInternal()
    }

    private fun cancelActiveSessionInternal(): Boolean {
        val targetId = activeTargetId ?: return false
        val target = targets[targetId] ?: return false
        val sessionId = activeSessionId ?: return false

        editingState = EditingState.CANCELLING
        target.onEditingStateChanged?.invoke(EditingState.CANCELLING)

        target.onCancel?.invoke()

        closeSession(sessionId)
        persistentSessionIds.remove(targetId)

        activeTargetId = null
        activeSessionId = null

        editingState = EditingState.IDLE
        target.onEditingStateChanged?.invoke(EditingState.IDLE)
        return true
    }

    fun resetPersistentSession(targetId: String, text: String, cursorUtf8: Int, source: SessionResetSource = SessionResetSource.EXTERNAL) {
        if (source == SessionResetSource.LOCAL_CONTENT_CHANGED) return

        val target = targets[targetId] ?: return
        if (!target.isPersistent) return

        val sessionId = persistentSessionIds[targetId]
        if (sessionId == null) {
            target.updateText(text)
            val newSessionId = createSession(target, text, cursorUtf8)
            if (newSessionId == null || newSessionId == 0UL) {
                Log.e(TAG, "resetPersistentSession($targetId): failed to create session for empty/missing persistent session")
                return
            }
            persistentSessionIds[targetId] = newSessionId
            if (targetId == activeTargetId) {
                activeSessionId = newSessionId
            }
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
                targets[targetId]?.updateText(text)
            }
            else -> { }
        }
        updateTargetProjection(targetId)
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

    override fun applyTargetCommand(targetId: String, command: TargetCommand): TargetCommandResult {
        val sessionId = persistentSessionIds[targetId]
            ?: return TargetCommandResult.Failed(TargetCommandError.NO_PERSISTENT_SESSION)
        if (!validateSession(sessionId)) {
            return TargetCommandResult.Failed(TargetCommandError.SESSION_INVALID)
        }

        val snapshotBefore = queryTargetSnapshot(targetId)
            ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)

        val projectionForRevision = targetProjections[targetId]
        val effectiveRevision = projectionForRevision?.getRevision() ?: snapshotBefore.revision

        val bridge = TextEditSessionBridge(appServiceBridge, sessionId)
        val dtoResult = when (command) {
            is TargetCommand.Replace -> {
                bridge.replace(
                    command.byteStart, command.byteEndExclusive,
                    command.replacementText, command.originalText,
                    uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC,
                    effectiveRevision
                )
            }
            is TargetCommand.ReplaceAll -> {
                bridge.replaceAll(
                    command.searchText, command.replacementText,
                    effectiveRevision
                )
            }
            is TargetCommand.SetSelection -> {
                bridge.setSelection(
                    command.anchorUtf8, command.headUtf8,
                    effectiveRevision
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

        val applyProjection = targetProjections[targetId]
        if (applyProjection != null) {
            applyProjection.applyEditResult(editResult)
            val decorations = targetDecorations[targetId]
            if (decorations != null) {
                applyProjection.setSearchHighlights(decorations.searchHighlightsUtf8)
                if (decorations.selectionStartUtf8 >= 0 && decorations.selectionEndUtf8 >= 0) {
                    applyProjection.setSelection(decorations.selectionStartUtf8, decorations.selectionEndUtf8)
                }
            }
        } else {
            updateTargetProjection(targetId)
        }

        targetDecorationsVersion++

        val snapshotAfter = queryTargetSnapshot(targetId)
            ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)

        targets[targetId]?.updateText(snapshotAfter.text)
        if (targetId != activeTargetId) {
            onTargetContentChanged?.invoke(targetId, snapshotAfter.text)
        }

        return TargetCommandResult.Success(snapshotAfter)
    }

    override fun setTargetDecorations(targetId: String, decorations: TargetDecorations) {
        targetDecorations[targetId] = decorations
        targetDecorationsVersion++
        val projection = targetProjections[targetId]
        if (projection != null) {
            if (decorations.searchHighlightsUtf8.isEmpty() &&
                decorations.selectionStartUtf8 < 0 && decorations.selectionEndUtf8 < 0) {
                projection.clearDecorations()
            } else {
                projection.setSearchHighlights(decorations.searchHighlightsUtf8)
                if (decorations.selectionStartUtf8 >= 0 && decorations.selectionEndUtf8 >= 0) {
                    projection.setSelection(decorations.selectionStartUtf8, decorations.selectionEndUtf8)
                }
            }
        }
    }

    // ── Projection management ──

    fun getTargetProjection(targetId: String): TargetDisplayRuntime? {
        return targetProjections[targetId]
    }

    fun saveActiveTargetProjection(
        scrollX: Float, scrollY: Float, viewportW: Int, viewportH: Int,
        fontSize: Float, lineSpacing: Float,
        themeColors: ThemeColorsSnapshot?,
    ) {
        val targetId = activeTargetId ?: return
        val target = targets[targetId] ?: return
        if (!target.isPersistent) return

        val projection = getOrCreateProjection(targetId, target)
        val snapshot = queryTargetSnapshot(targetId)
        if (snapshot != null) {
            projection.updateFromSnapshot(snapshot.text, snapshot.cursorUtf8, snapshot.revision)
        }
        projection.setScrollPosition(scrollX, scrollY)
        projection.setViewportSize(viewportW, viewportH)
        projection.setFontSize(fontSize)
        projection.setLineSpacingMultiplier(lineSpacing)
        if (themeColors != null) {
            projection.setThemeColors(
                themeColors.text, themeColors.cursor, themeColors.selection,
                themeColors.composing, themeColors.background, themeColors.searchHighlight
            )
        }
        val decorations = targetDecorations[targetId]
        if (decorations != null) {
            projection.setSearchHighlights(decorations.searchHighlightsUtf8)
            if (decorations.selectionStartUtf8 >= 0 && decorations.selectionEndUtf8 >= 0) {
                projection.setSelection(decorations.selectionStartUtf8, decorations.selectionEndUtf8)
            }
        }
    }

    private fun updateTargetProjection(targetId: String) {
        val target = targets[targetId] ?: return
        if (!target.isPersistent) return
        val snapshot = queryTargetSnapshot(targetId) ?: return
        val projection = getOrCreateProjection(targetId, target)
        projection.updateFromSnapshot(snapshot.text, snapshot.cursorUtf8, snapshot.revision)
        val decorations = targetDecorations[targetId]
        if (decorations != null) {
            projection.setSearchHighlights(decorations.searchHighlightsUtf8)
            if (decorations.selectionStartUtf8 >= 0 && decorations.selectionEndUtf8 >= 0) {
                projection.setSelection(decorations.selectionStartUtf8, decorations.selectionEndUtf8)
            }
        }
    }

    private fun getOrCreateProjection(targetId: String, target: EditableTextTarget): TargetDisplayRuntime {
        return targetProjections.getOrPut(targetId) {
            val mirror = DisplayTextMirror()
            val textPaint = android.text.TextPaint().apply {
                textSize = target.profile.fontSizePx.coerceAtLeast(1f)
                isAntiAlias = true
            }
            TargetDisplayRuntime(mirror, textPaint, animationTimeSource, transactionIdSource)
        }
    }

    /**
     * #592 三：把投影运行时接到当前窗口的 FrameClock，使投影动画按真实 VSync 推进。
     *
     * 传入 null 解除绑定（窗口销毁前），避免已释放的 FrameClock 继续驱动投影。
     * 由 [EditorWindowHost] 在 beginEdit/releaseWindow 调用，不在会话层自建时钟。
     */
    fun setProjectionFrameClock(targetId: String, frameClock: WindowDisplayFrameClock?) {
        val target = targets[targetId] ?: return
        if (!target.isPersistent) return
        if (frameClock == null) {
            targetProjections[targetId]?.setFrameClock(null)
            return
        }
        val projection = getOrCreateProjection(targetId, target)
        projection.setFrameClock(frameClock)
        if (target.profile.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) {
            projection.setSecretMasked(true)
        }
    }

    // ── Session lifecycle ──

    private fun createSession(target: EditableTextTarget, text: String, cursorByteOffset: Int): ULong? {
        return when (val result = appServiceBridge.textEditSessionCreate(
            target.targetId,
            text,
            cursorByteOffset.toUInt(),
            target.isPersistent
        )) {
            is BridgeResult.Success -> {
                val id = result.data
                if (id == null || id == 0UL) {
                    Log.e(TAG, "createSession(${target.targetId}): Core returned null/0 session id")
                    null
                } else {
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.sessionLifecycle(id.toString(), "create")
                    id
                }
            }
            else -> {
                Log.e(TAG, "createSession(${target.targetId}): Core session creation failed")
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
            cancelActiveSessionInternal()
        }
        persistentSessionIds.values.forEach { sessionId ->
            closeSession(sessionId)
        }
        persistentSessionIds.clear()
        targetProjections.values.forEach { it.release() }
        targetProjections.clear()
        targetDecorations.clear()
        editingState = EditingState.RELEASED
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
)

data class ThemeColorsSnapshot(
    val text: Int,
    val cursor: Int,
    val selection: Int,
    val composing: Int,
    val background: Int,
    val searchHighlight: Int,
)
