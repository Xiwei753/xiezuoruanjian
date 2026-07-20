package com.xiwei.sujian.editor.v2.coordinator

import android.graphics.Rect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Registration descriptor for an editable text region within the window.
 *
 * Per #541: each target represents a UI/business object (project title, chapter body,
 * search query, starmap node, etc.) that can be activated for editing via
 * [AnimatedTextEditorCoordinator.beginEdit]. The [targetId] is a namespaced string
 * (e.g. "chapter-title:{chapterId}") that identifies the business object; it is
 * separate from the Rust TextEditSessionId which represents an editing transaction.
 *
 * [isPersistent] determines the session lifecycle: persistent targets (chapter body)
 * keep their session across edits; draft targets (project title) close on every commit.
 * [commitPolicy] controls when the coordinator commits text to the domain model.
 */
class EditableTextTarget(
    val targetId: String,
    profile: TextEditorProfile = TextEditorProfile(),
    isPersistent: Boolean = false,
    commitPolicy: CommitPolicy = if (isPersistent) CommitPolicy.COMMIT_ON_EVERY_CHANGE else CommitPolicy.COMMIT_ON_CONFIRM
) {
    var profile: TextEditorProfile by mutableStateOf(profile)
        private set
    var isPersistent: Boolean by mutableStateOf(isPersistent)
    var commitPolicy: CommitPolicy by mutableStateOf(commitPolicy)
    var onTextChanged: ((String) -> Unit)? = null
    var onCommit: ((String) -> Unit)? = null
    var onCancel: (() -> Unit)? = null
    var onEditingStateChanged: ((EditingState) -> Unit)? = null

    var currentGeometry: Rect by mutableStateOf(Rect())
        private set
    var currentTransform: Transform2D by mutableStateOf(Transform2D.IDENTITY)
        private set
    var currentText: String by mutableStateOf("")
        private set

    fun updateGeometry(rect: Rect) {
        currentGeometry = rect
    }

    fun updateTransform(transform: Transform2D) {
        currentTransform = transform
    }

    fun updateText(text: String) {
        currentText = text
    }

    fun updateProfile(newProfile: TextEditorProfile) {
        profile = newProfile
    }

    fun updatePersistent(persistent: Boolean) {
        isPersistent = persistent
    }

    fun updateCommitPolicy(newCommitPolicy: CommitPolicy) {
        commitPolicy = newCommitPolicy
    }
}

data class Transform2D(
    val translateX: Float = 0f,
    val translateY: Float = 0f,
    val scaleX: Float = 1f,
    val scaleY: Float = 1f
) {
    companion object {
        val IDENTITY = Transform2D()
    }
}

/**
 * Lifecycle states of the shared editing host within the coordinator.
 *
 * Per #541: the shared [SujianEditorView] transitions through these states as the
 * coordinator binds, rebinds, commits, or cancels editing targets. Only EDITING produces
 * an active InputConnection; all other states either prepare or tear down the session.
 *
 * State transitions:
 * - IDLE → BINDING (beginEdit called)
 * - BINDING → EDITING (session created and host bound)
 * - EDITING → COMMITTING / CANCELLING (user or programmatic action)
 * - EDITING → REBINDING (beginEdit called for a different target while editing)
 * - COMMITTING / CANCELLING / REBINDING → IDLE (action completed)
 * - IDLE → RELEASED (releaseHost called — terminal, host cannot be reused)
 */
enum class EditingState {
    IDLE,
    BINDING,
    EDITING,
    COMMITTING,
    CANCELLING,
    REBINDING,
    RELEASED
}
