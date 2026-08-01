package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.semantics.editableText
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setText
import androidx.compose.ui.semantics.setSelection
import androidx.compose.ui.semantics.insertTextAtCursor
import androidx.compose.ui.semantics.textSelectionRange
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile

@Composable
fun AnimatedInlineText(
    targetId: String,
    value: String,
    onValueChange: (String) -> Unit,
    onCommit: (String) -> Unit,
    modifier: Modifier = Modifier,
    profile: TextEditorProfile = TextEditorProfile.CanvasLabel,
    enabled: Boolean = true,
    coordinator: AnimatedTextEditorCoordinator? = null
) {
    val effectiveCoordinator = coordinator ?: LocalAnimatedTextEditorCoordinator.current
        ?: throw IllegalStateException(
            "AnimatedInlineText($targetId) requires an AnimatedTextEditorCoordinator. " +
            "Ensure the host Activity provides one via CompositionLocalProvider."
        )

    var localValue by remember(value) { mutableStateOf(value) }
    var selectionRange by remember { mutableStateOf(TextRange(value.length)) }
    var isEditing by remember { mutableStateOf(false) }

    val currentValue by rememberUpdatedState(value)
    val currentProfile by rememberUpdatedState(profile)
    val refOnValueChanged by rememberUpdatedState(onValueChange)
    val refOnCommit by rememberUpdatedState(onCommit)

    val target = remember(targetId) {
        EditableTextTarget(targetId = targetId)
    }

    target.onTextChanged = { newText ->
        localValue = newText
        refOnValueChanged(newText)
    }
    target.onCommit = { finalText ->
        localValue = finalText
        isEditing = false
        refOnCommit(finalText)
    }
    target.onCancel = { isEditing = false }
    target.onEditingStateChanged = { state ->
        isEditing = state == EditingState.EDITING || state == EditingState.BINDING
    }
    target.updateProfile(currentProfile)
    target.updatePersistent(false)
    target.updateText(currentValue)

    LaunchedEffect(value) {
        if (value != localValue) {
            localValue = value
            effectiveCoordinator.updateTargetText(targetId, value)
        }
    }

    DisposableEffect(targetId) {
        effectiveCoordinator.registerTarget(target)
        onDispose {
            effectiveCoordinator.unregisterTarget(targetId)
        }
    }

    Box(
        modifier = modifier
            .wrapContentSize()
            .onGloballyPositioned { coordinates ->
                val position = coordinates.positionInWindow()
                val size = coordinates.size
                val rect = Rect(
                    position.x, position.y,
                    position.x + size.width, position.y + size.height
                )
                effectiveCoordinator.updateTargetGeometry(targetId, rect.toAndroidRect())
            }
            .then(
                if (enabled) {
                    Modifier.pointerInput(targetId) {
                        detectTapGestures {
                            effectiveCoordinator.updateTargetText(targetId, currentValue)
                            val cursorUtf8 = currentValue.toByteArray(Charsets.UTF_8).size
                            effectiveCoordinator.beginEdit(targetId, cursorUtf8)
                        }
                    }
                } else {
                    Modifier
                }
            )
            .then(
                if (enabled) {
                    Modifier.semantics {
                        editableText = androidx.compose.ui.text.AnnotatedString(localValue)
                        textSelectionRange = selectionRange
                        setText {
                            val newText = it.text
                            localValue = newText
                            selectionRange = TextRange(newText.length)
                            effectiveCoordinator.updateTargetText(targetId, newText)
                            effectiveCoordinator.beginEdit(targetId, newText.toByteArray(Charsets.UTF_8).size)
                            refOnValueChanged(newText)
                            true
                        }
                        insertTextAtCursor { annotatedText ->
                            val insertText = annotatedText.text
                            val sel = selectionRange
                            val before = localValue.substring(0, sel.min)
                            val after = localValue.substring(sel.max)
                            val newText = before + insertText + after
                            val newCursor = sel.min + insertText.length
                            localValue = newText
                            selectionRange = TextRange(newCursor)
                            val utf8Offset = localValue.substring(0, newCursor).toByteArray(Charsets.UTF_8).size
                            effectiveCoordinator.updateTargetText(targetId, newText)
                            effectiveCoordinator.beginEdit(targetId, utf8Offset)
                            refOnValueChanged(newText)
                            true
                        }
                        setSelection { selStart, selEnd, _ ->
                            val clampedStart = selStart.coerceIn(0, localValue.length)
                            val clampedEnd = selEnd.coerceIn(0, localValue.length)
                            selectionRange = TextRange(clampedStart, clampedEnd)
                            true
                        }
                    }
                } else {
                    Modifier.semantics {
                        editableText = androidx.compose.ui.text.AnnotatedString(localValue)
                    }
                }
            )
    ) {
        Text(
            text = localValue,
            style = MaterialTheme.typography.bodyMedium,
            color = if (isEditing) {
                MaterialTheme.colorScheme.onSurface.copy(alpha = 0f)
            } else {
                MaterialTheme.colorScheme.onSurface
            }
        )
    }
}

private fun Rect.toAndroidRect(): android.graphics.Rect {
    return android.graphics.Rect(
        left.toInt(), top.toInt(), right.toInt(), bottom.toInt()
    )
}
