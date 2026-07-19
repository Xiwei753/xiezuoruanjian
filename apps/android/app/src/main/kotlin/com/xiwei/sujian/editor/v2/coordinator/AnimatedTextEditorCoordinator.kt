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

class AnimatedTextEditorCoordinator(
    private val context: Context,
    private val appServiceBridge: AppServiceBridge
) {
    private val targets = mutableMapOf<String, EditableTextTarget>()
    private var activeSessionId: ULong? = null
    private var sharedEditorView: SujianEditorView? = null
    private val persistentSessionIds = mutableMapOf<String, ULong>()
    private var callbackGeneration: Long = 0L

    var activeTargetId: String? by mutableStateOf(null)
        private set
    var editingState: EditingState by mutableStateOf(EditingState.IDLE)
        private set

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
        targets.remove(targetId)
    }

    fun beginEdit(targetId: String, initialSelection: Int? = null): Boolean {
        val target = targets[targetId] ?: return false
        if (activeTargetId == targetId && editingState == EditingState.EDITING) return true

        if (activeTargetId != null && activeTargetId != targetId) {
            val oldTarget = targets[activeTargetId]
            editingState = EditingState.REBINDING
            oldTarget?.onEditingStateChanged?.invoke(EditingState.REBINDING)

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
        }
        activeTargetId = null
        activeSessionId = null
        activeTargetGeometry = Rect()
        activeTargetTransform = Transform2D.IDENTITY

        editingState = EditingState.IDLE
        target.onEditingStateChanged?.invoke(EditingState.IDLE)
        lastCommittedText = finalText
        return true
    }

    var lastCommittedText: String? = null
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

        if (target.isPersistent) {
            if (validateSession(sessionId)) {
                appServiceBridge.textEditSessionReset(sessionId, target.currentText, target.currentText.toByteArray(Charsets.UTF_8).size.toUInt())
            } else {
                Log.w(TAG, "cancelActiveEdit($targetId): persistent session $sessionId invalid, closing and removing")
                persistentSessionIds.remove(targetId)
                closeSession(sessionId)
            }
        } else {
            closeSession(sessionId)
            persistentSessionIds.remove(targetId)
        }
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
    }

    fun getTargetGeometry(targetId: String): Rect? = targets[targetId]?.currentGeometry

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
        sharedEditorView?.let { view ->
            view.release()
        }
        sharedEditorView = null
        editingState = EditingState.RELEASED
    }

    private fun installContentCallback(view: SujianEditorView, target: EditableTextTarget) {
        callbackGeneration++
        val generation = callbackGeneration
        view.onContentChanged = { newText ->
            if (generation == callbackGeneration) {
                target.onTextChanged?.invoke(newText)
            }
        }
    }

    private fun installCommitRequestedCallback(view: SujianEditorView) {
        view.onCommitRequested = {
            commitActiveEdit()
        }
    }

    private fun clearActiveCallbacks() {
        callbackGeneration++
        sharedEditorView?.let { view ->
            view.onContentChanged = null
            view.onCommitRequested = null
        }
    }

    private fun getOrCreateEditorView(): SujianEditorView {
        return sharedEditorView ?: SujianEditorView(context).also {
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
    EXTERNAL
}
