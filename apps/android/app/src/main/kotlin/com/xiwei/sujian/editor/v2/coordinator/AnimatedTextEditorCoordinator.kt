package com.xiwei.sujian.editor.v2.coordinator

import android.content.Context
import android.graphics.Rect
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge
import com.xiwei.sujian.editor.v2.coordinator.SecretPolicy
import com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

/**
 * Window-level coordinator that manages a single shared [SujianEditorView] and dispatches
 * it among registered [EditableTextTarget]s.
 *
 * Session lifecycle model (per #541):
 * - **Persistent content sessions** (chapter body, starmap node body): the session is
 *   created on first [beginEdit] and kept alive across multiple edit/commit cycles.
 *   [commitActiveEdit] performs a soft reset (cancels animation + invalidates composition)
 *   without closing the Rust session, so the Undo/Redo stack and revision history survive.
 *   The session is closed only on [unregisterTarget] or [releaseHost].
 * - **Draft sessions** (project title, search query, short labels): the session is created
 *   on [beginEdit] and closed on every [commitActiveEdit] or [cancelActiveEdit].
 *   The final text is committed to the domain model in one shot; there is no Undo/Redo
 *   across edits. This prevents draft Undo stacks from leaking between targets.
 *
 * Rebind: when [beginEdit] is called while another target is active, the coordinator
 * commits or cancels the old target, then binds the shared host to the new target.
 * The shared [SujianEditorView] is reused (not recreated) — only the kernel bridge and
 * profile are swapped via [SujianEditorView.bindSession].
 */
class AnimatedTextEditorCoordinator(
    private val context: Context,
    private val appServiceBridge: AppServiceBridge,
    private val animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource = com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
    private val transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource = com.xiwei.sujian.editor.v2.visual.TransactionIdSource()
) : SessionCommandPort {
    private val targets = mutableMapOf<String, EditableTextTarget>()
    private var activeSessionId: ULong? = null
    private var sharedEditorView: SujianEditorView? = null
    private val persistentSessionIds = mutableMapOf<String, ULong>()
    private val targetProjections = mutableMapOf<String, TargetDisplayRuntime>()
    private val windowFrameClock = WindowDisplayFrameClock()

    var targetDecorationsVersion by mutableStateOf(0L)
        private set
    private val targetDecorations = mutableMapOf<String, TargetDecorations>()

    var activeTargetId: String? by mutableStateOf(null)
        private set
    var editingState: EditingState by mutableStateOf(EditingState.IDLE)
        private set

    var onTargetContentChanged: ((targetId: String, newText: String) -> Unit)? = null

    var activeTargetGeometry: Rect by mutableStateOf(Rect())
        private set
    var activeTargetTransform: Transform2D by mutableStateOf(Transform2D.IDENTITY)
        private set

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
            cancelActiveEdit()
        }
        val persistentSessionId = persistentSessionIds.remove(targetId)
        if (persistentSessionId != null) {
            closeSession(persistentSessionId)
        }
        targetProjections.remove(targetId)?.release()
        targetDecorations.remove(targetId)
        targets.remove(targetId)
    }

    fun beginEdit(targetId: String, initialSelection: Int? = null): Boolean {
        val target = targets[targetId] ?: return false
        if (activeTargetId == targetId && editingState == EditingState.EDITING) return true

        if (activeTargetId != null && activeTargetId != targetId) {
            val oldTarget = targets[activeTargetId]
            editingState = EditingState.REBINDING
            oldTarget?.onEditingStateChanged?.invoke(EditingState.REBINDING)

            saveActiveTargetProjection()

            clearActiveCallbacks()

            if (!commitActiveEditInternal()) {
                cancelActiveEditInternal()
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
            Log.e(TAG, "beginEdit($targetId): session creation returned invalid id=$sessionId, aborting")
            persistentSessionIds.remove(targetId)
            editingState = EditingState.IDLE
            target.onEditingStateChanged?.invoke(EditingState.IDLE)
            return false
        }

        activeTargetId = targetId
        activeSessionId = sessionId
        activeTargetGeometry = target.currentGeometry
        activeTargetTransform = target.currentTransform

        val view = getOrCreateEditorView()
        val bridge = TextEditSessionBridge(appServiceBridge, sessionId)
        view.bindSession(bridge, target.profile, textForSession, sel)

        installContentCallback(view, target)
        installCommitRequestedCallback(view)
        installCancelRequestedCallback(view)

        val geometry = target.currentGeometry
        if (geometry.width() > 0 && geometry.height() > 0) {
            activeTargetGeometry = geometry
        }

        editingState = EditingState.EDITING
        target.onEditingStateChanged?.invoke(EditingState.EDITING)

        view.post {
            view.requestFocus()
            val imm = context.getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as? android.view.inputmethod.InputMethodManager
            imm?.showSoftInput(view, 0)
        }

        return true
    }

    fun commitActiveEdit(): Boolean {
        clearActiveCallbacks()
        return commitActiveEditInternal()
    }

    private fun commitActiveEditInternal(): Boolean {
        val targetId = activeTargetId ?: return false
        val target = targets[targetId] ?: return false
        val sessionId = activeSessionId ?: return false

        editingState = EditingState.COMMITTING
        target.onEditingStateChanged?.invoke(EditingState.COMMITTING)

        val view = sharedEditorView
        var finalText: String? = null
        if (view != null) {
            finalText = view.getText()
            target.onCommit?.invoke(finalText)
            target.updateText(finalText)
            // Per #541 session lifecycle: persistent sessions survive commit — the Undo/Redo
            // stack and revision history are preserved across edit/commit cycles. Only
            // transient animation and composition state are cleared (softResetForPersistentCommit).
            // Draft sessions are fully closed on commit — the final text is submitted to the
            // domain model in one shot, and the Rust session is destroyed to prevent draft
            // Undo stacks from leaking between targets.
            if (target.isPersistent) {
                view.softResetForPersistentCommit()
            } else {
                view.unbindSession("commit")
                closeSession(sessionId)
            }
        } else {
            if (!target.isPersistent) {
                closeSession(sessionId)
            }
        }

        if (!target.isPersistent) {
            persistentSessionIds.remove(targetId)
            if (target.profile.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) {
                target.updateText("")
            }
        }
        activeTargetId = null
        activeSessionId = null
        activeTargetGeometry = Rect()
        activeTargetTransform = Transform2D.IDENTITY

        editingState = EditingState.IDLE
        target.onEditingStateChanged?.invoke(EditingState.IDLE)
        lastCommittedText = if (target.profile.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) null else finalText
        return true
    }

    var lastCommittedText: String? by mutableStateOf(null)
        private set

    fun cancelActiveEdit(): Boolean {
        clearActiveCallbacks()
        return cancelActiveEditInternal()
    }

    private fun cancelActiveEditInternal(): Boolean {
        val targetId = activeTargetId ?: return false
        val target = targets[targetId] ?: return false
        val sessionId = activeSessionId ?: return false

        editingState = EditingState.CANCELLING
        target.onEditingStateChanged?.invoke(EditingState.CANCELLING)

        sharedEditorView?.let { view ->
            view.unbindSession("cancel")
        }
        target.onCancel?.invoke()

        closeSession(sessionId)
        persistentSessionIds.remove(targetId)

        activeTargetId = null
        activeSessionId = null
        activeTargetGeometry = Rect()
        activeTargetTransform = Transform2D.IDENTITY

        editingState = EditingState.IDLE
        target.onEditingStateChanged?.invoke(EditingState.IDLE)
        return true
    }

    fun updateTargetGeometry(targetId: String, geometry: Rect) {
        targets[targetId]?.updateGeometry(geometry)
        if (targetId == activeTargetId) {
            activeTargetGeometry = geometry
        }
    }

    fun updateTargetTransform(targetId: String, transform: Transform2D) {
        targets[targetId]?.updateTransform(transform)
        if (targetId == activeTargetId) {
            activeTargetTransform = transform
        }
    }

    fun updateTargetText(targetId: String, text: String) {
        targets[targetId]?.updateText(text)
    }

    fun resetPersistentSession(targetId: String, text: String, cursorUtf8: Int, source: SessionResetSource = SessionResetSource.EXTERNAL) {
        // LOCAL_CONTENT_CHANGED resets are ignored because the local editor is the authority
        // for its own content; only external resets (chapter switch, sync) are applied.
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
                val view = sharedEditorView
                if (view != null) {
                    clearActiveCallbacks()
                    val bridge = TextEditSessionBridge(appServiceBridge, newSessionId)
                    view.bindSession(bridge, target.profile, text, cursorUtf8)
                    installContentCallback(view, target)
                    installCommitRequestedCallback(view)
                    installCancelRequestedCallback(view)
                }
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
        if (targetId == activeTargetId) {
            sharedEditorView?.loadText(text, cursorUtf8)
        }
        updateTargetProjection(targetId)
    }

    fun getTargetGeometry(targetId: String): Rect? = targets[targetId]?.currentGeometry

    fun getTargetText(targetId: String): String? = targets[targetId]?.currentText

    fun getPersistentSessionId(targetId: String): ULong? = persistentSessionIds[targetId]

    fun getSharedEditorView(): SujianEditorView? = sharedEditorView

    fun setSharedEditorView(view: SujianEditorView) {
        sharedEditorView = view
    }

    fun updateHostGeometry(width: Float, height: Float) {
        sharedEditorView?.updateHostGeometry(width, height)
    }

    fun releaseHost() {
        clearActiveCallbacks()
        if (activeTargetId != null) {
            cancelActiveEditInternal()
        }
        persistentSessionIds.values.forEach { sessionId ->
            closeSession(sessionId)
        }
        persistentSessionIds.clear()
        targetProjections.values.forEach { it.release() }
        targetProjections.clear()
        targetDecorations.clear()
        sharedEditorView?.let { view ->
            view.setFrameClock(null)
            view.release()
        }
        sharedEditorView = null
        windowFrameClock.release()
        editingState = EditingState.RELEASED
    }

    private fun installContentCallback(view: SujianEditorView, target: EditableTextTarget) {
        view.onContentChanged = { newText ->
            target.onTextChanged?.invoke(newText)
        }
        view.onSearchHighlightsCleared = {
            for ((targetId, _) in targetProjections) {
                setTargetDecorations(targetId, TargetDecorations())
            }
            if (activeTargetId != null) {
                targetDecorations.remove(activeTargetId)
                targetDecorationsVersion++
            }
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

    // ── SessionCommandPort implementation ──

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

        if (targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null) {
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
                val snapshotAfter = queryTargetSnapshot(targetId)
                    ?: return TargetCommandResult.Failed(TargetCommandError.SNAPSHOT_UNAVAILABLE)
                targets[targetId]?.updateText(snapshotAfter.text)
                updateTargetProjection(targetId)
                return TargetCommandResult.Success(snapshotAfter)
            }
        }

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

    // ── Read-only projection for inactive targets ──

    fun getTargetProjection(targetId: String): TargetDisplayRuntime? {
        return targetProjections[targetId]
    }

    private fun saveActiveTargetProjection() {
        val targetId = activeTargetId ?: return
        val target = targets[targetId] ?: return
        if (!target.isPersistent) return

        val projection = getOrCreateProjection(targetId, target)
        val snapshot = queryTargetSnapshot(targetId)
        if (snapshot != null) {
            projection.updateFromSnapshot(snapshot.text, snapshot.cursorUtf8, snapshot.revision)
        } else {
            val view = sharedEditorView ?: return
            projection.updateFromSnapshot(
                view.getText(),
                view.getPipeline().getCursorUtf8(),
                view.getPipeline().getRevision()
            )
        }
        val view = sharedEditorView
        if (view != null) {
            projection.setScrollPosition(view.getScrollXPos(), view.getScrollYPos())
            projection.setViewportSize(view.width, view.height)
            projection.setFontSize(view.getPipelineTextPaintSize())
            projection.setLineSpacingMultiplier(view.getPipelineLineSpacingMultiplier())
            val themeColors = view.getPipelineThemeColors()
            if (themeColors != null) {
                projection.setThemeColors(
                    themeColors.text,
                    themeColors.cursor,
                    themeColors.selection,
                    themeColors.composing,
                    themeColors.background,
                    themeColors.searchHighlight
                )
            }
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
            val projection = TargetDisplayRuntime(mirror, textPaint, animationTimeSource, transactionIdSource)
            projection.setFrameClock(windowFrameClock)
            if (target.profile.secretPolicy == SecretPolicy.MASK_AND_CLEAR_ON_COMMIT) {
                projection.setSecretMasked(true)
            }
            projection
        }
    }

    private fun getOrCreateEditorView(): SujianEditorView {
        return sharedEditorView ?: SujianEditorView(context, animationTimeSource = animationTimeSource, transactionIdSource = transactionIdSource).also {
            it.setFrameClock(windowFrameClock)
            sharedEditorView = it
        }
    }

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
                } else id
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

    companion object {
        private const val TAG = "AnimatedTextEditorCoordinator"
    }
}

enum class SessionResetSource {
    LOCAL_CONTENT_CHANGED,
    EXTERNAL,
    CHAPTER_SWITCH
}
