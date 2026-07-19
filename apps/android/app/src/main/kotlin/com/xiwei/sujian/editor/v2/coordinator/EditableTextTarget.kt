package com.xiwei.sujian.editor.v2.coordinator

import android.graphics.Rect

data class EditableTextTarget(
    val targetId: String,
    val profile: TextEditorProfile,
    val initialText: String,
    val initialSelection: Int = initialText.toByteArray(Charsets.UTF_8).size,
    val isPersistent: Boolean = false,
    val commitPolicy: CommitPolicy = if (isPersistent) CommitPolicy.COMMIT_ON_EVERY_CHANGE else CommitPolicy.COMMIT_ON_CONFIRM,
    var onTextChanged: ((String) -> Unit)? = null,
    var onCommit: ((String) -> Unit)? = null,
    var onCancel: (() -> Unit)? = null,
    var onEditingStateChanged: ((EditingState) -> Unit)? = null
) {
    var currentGeometry: Rect = Rect()
        private set
    var currentTransform: Transform2D = Transform2D.IDENTITY
        private set
    var currentText: String = initialText
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
