package com.xiwei.sujian.editor.v2.coordinator

import android.graphics.Rect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

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

enum class EditingState {
    IDLE,
    BINDING,
    EDITING,
    COMMITTING,
    CANCELLING,
    REBINDING,
    RELEASED
}
