package com.xiwei.sujian.editor.v2.coordinator

import android.content.Context
import android.graphics.Rect
import android.widget.FrameLayout
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

    var activeTargetId: String? by mutableStateOf(null)
        private set
    var editingState: EditingState by mutableStateOf(EditingState.IDLE)
        private set

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
        if (activeTargetId == targetId && editingState == EditingState.EDITING) return true

        if (activeTargetId != null && activeTargetId != targetId) {
            if (!commitActiveEdit()) {
                cancelActiveEdit()
            }
        }

        editingState = EditingState.BINDING
        target.onEditingStateChanged?.invoke(EditingState.BINDING)

        val sel = initialSelection ?: target.initialSelection
        val sessionId = createSession(target, sel) ?: run {
            editingState = EditingState.IDLE
            return false
        }

        activeTargetId = targetId
        activeSessionId = sessionId

        val view = getOrCreateEditorView()
        val bridge = TextEditSessionBridge(appServiceBridge, sessionId)
        view.bindSession(bridge, target.profile, target.initialText, sel)

        val geometry = target.currentGeometry
        if (geometry.width() > 0 && geometry.height() > 0) {
            positionViewOverTarget(view, geometry)
        }

        editingState = EditingState.EDITING
        target.onEditingStateChanged?.invoke(EditingState.EDITING)
        return true
    }

    fun commitActiveEdit(): Boolean {
        val targetId = activeTargetId ?: return false
        val target = targets[targetId] ?: return false
        val sessionId = activeSessionId ?: return false

        editingState = EditingState.COMMITTING
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

        editingState = EditingState.IDLE
        target.onEditingStateChanged?.invoke(EditingState.IDLE)
        return true
    }

    fun cancelActiveEdit(): Boolean {
        val targetId = activeTargetId ?: return false
        val target = targets[targetId] ?: return false
        val sessionId = activeSessionId ?: return false

        editingState = EditingState.CANCELLING
        target.onEditingStateChanged?.invoke(EditingState.CANCELLING)

        sharedEditorView?.unbindSession("cancel")
        target.onCancel?.invoke()

        closeSession(sessionId)
        activeTargetId = null
        activeSessionId = null

        editingState = EditingState.IDLE
        target.onEditingStateChanged?.invoke(EditingState.IDLE)
        return true
    }

    fun updateTargetGeometry(targetId: String, geometry: Rect) {
        targets[targetId]?.updateGeometry(geometry)
        if (targetId == activeTargetId) {
            val view = sharedEditorView
            if (view != null && editingState == EditingState.EDITING) {
                positionViewOverTarget(view, geometry)
            }
        }
    }

    fun updateTargetTransform(targetId: String, transform: Transform2D) {
        targets[targetId]?.updateTransform(transform)
    }

    fun getTargetGeometry(targetId: String): Rect? = targets[targetId]?.currentGeometry

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
        editingState = EditingState.RELEASED
    }

    fun positionActiveTargetView(view: SujianEditorView) {
        val targetId = activeTargetId ?: return
        val geometry = targets[targetId]?.currentGeometry ?: return
        positionViewOverTarget(view, geometry)
    }

    private fun positionViewOverTarget(view: SujianEditorView, geometry: Rect) {
        val lp = view.layoutParams as? FrameLayout.LayoutParams
            ?: FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.WRAP_CONTENT,
                FrameLayout.LayoutParams.WRAP_CONTENT
            )
        lp.width = if (geometry.width() > 0) geometry.width() else FrameLayout.LayoutParams.MATCH_PARENT
        lp.height = if (geometry.height() > 0) geometry.height() else FrameLayout.LayoutParams.MATCH_PARENT
        lp.leftMargin = geometry.left
        lp.topMargin = geometry.top
        view.layoutParams = lp
        view.updateHostGeometry(
            geometry.width().toFloat(),
            geometry.height().toFloat()
        )
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
