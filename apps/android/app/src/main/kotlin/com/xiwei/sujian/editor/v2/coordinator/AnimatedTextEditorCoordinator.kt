package com.xiwei.sujian.editor.v2.coordinator

import android.content.Context
import android.graphics.Rect
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.host.EditorKernelBridge
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge

class AnimatedTextEditorCoordinator(
    private val context: Context,
    private val appServiceBridge: AppServiceBridge
) {
    private val targets = mutableMapOf<String, EditableTextTarget>()
    private var activeTargetId: String? = null
    private var activeSessionId: ULong? = null
    private var state: EditingState = EditingState.IDLE
    private var sharedEditorView: SujianEditorView? = null

    fun registerTarget(target: EditableTextTarget) {
        targets[target.targetId] = target
    }

    fun unregisterTarget(targetId: String) {
        if (activeTargetId == targetId) {
            cancelActiveEdit()
        }
        targets.remove(targetId)
    }

    fun beginEdit(targetId: String, initialSelection: Int? = null): Boolean {
        val target = targets[targetId] ?: return false
        if (activeTargetId == targetId && state == EditingState.EDITING) return true

        if (activeTargetId != null && activeTargetId != targetId) {
            if (!commitActiveEdit()) {
                cancelActiveEdit()
            }
        }

        state = EditingState.BINDING
        target.onEditingStateChanged?.invoke(EditingState.BINDING)

        val sel = initialSelection ?: target.initialSelection
        val sessionId = createSession(target, sel) ?: run {
            state = EditingState.IDLE
            return false
        }

        activeTargetId = targetId
        activeSessionId = sessionId

        val view = getOrCreateEditorView()
        val bridge = TextEditSessionBridge(appServiceBridge, sessionId)
        view.bindSession(bridge, target.profile, target.initialText, sel)

        state = EditingState.EDITING
        target.onEditingStateChanged?.invoke(EditingState.EDITING)
        return true
    }

    fun commitActiveEdit(): Boolean {
        val targetId = activeTargetId ?: return false
        val target = targets[targetId] ?: return false
        val sessionId = activeSessionId ?: return false

        state = EditingState.COMMITTING
        target.onEditingStateChanged?.invoke(EditingState.COMMITTING)

        val view = sharedEditorView
        if (view != null) {
            val finalText = view.getText()
            target.onCommit?.invoke(finalText)
            view.unbindSession("commit")
        }

        closeSession(sessionId)
        activeTargetId = null
        activeSessionId = null

        state = EditingState.IDLE
        target.onEditingStateChanged?.invoke(EditingState.IDLE)
        return true
    }

    fun cancelActiveEdit(): Boolean {
        val targetId = activeTargetId ?: return false
        val target = targets[targetId] ?: return false
        val sessionId = activeSessionId ?: return false

        state = EditingState.CANCELLING
        target.onEditingStateChanged?.invoke(EditingState.CANCELLING)

        sharedEditorView?.unbindSession("cancel")
        target.onCancel?.invoke()

        closeSession(sessionId)
        activeTargetId = null
        activeSessionId = null

        state = EditingState.IDLE
        target.onEditingStateChanged?.invoke(EditingState.IDLE)
        return true
    }

    fun updateTargetGeometry(targetId: String, geometry: Rect) {
        targets[targetId]?.updateGeometry(geometry)
        if (targetId == activeTargetId) {
            sharedEditorView?.updateHostGeometry(
                geometry.width().toFloat(),
                geometry.height().toFloat()
            )
        }
    }

    fun updateTargetTransform(targetId: String, transform: Transform2D) {
        targets[targetId]?.updateTransform(transform)
    }

    fun getActiveTargetId(): String? = activeTargetId

    fun getEditingState(): EditingState = state

    fun getSharedEditorView(): SujianEditorView? = sharedEditorView

    fun setSharedEditorView(view: SujianEditorView) {
        sharedEditorView = view
    }

    fun releaseHost() {
        if (activeTargetId != null) {
            cancelActiveEdit()
        }
        sharedEditorView?.let { view ->
            view.release()
        }
        sharedEditorView = null
        state = EditingState.RELEASED
    }

    private fun getOrCreateEditorView(): SujianEditorView {
        return sharedEditorView ?: SujianEditorView(context).also {
            sharedEditorView = it
        }
    }

    private fun createSession(target: EditableTextTarget, cursorByteOffset: Int): ULong? {
        return when (val result = appServiceBridge.textEditSessionCreate(
            target.targetId,
            target.initialText,
            cursorByteOffset.toUInt(),
            target.isPersistent
        )) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    private fun closeSession(sessionId: ULong) {
        when (appServiceBridge.textEditSessionClose(sessionId)) {
            is BridgeResult.Success -> { }
            else -> { }
        }
    }
}
