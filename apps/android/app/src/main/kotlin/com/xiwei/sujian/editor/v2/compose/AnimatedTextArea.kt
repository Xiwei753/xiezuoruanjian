package com.xiwei.sujian.editor.v2.compose

import android.util.Log
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.semantics.editableText
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setText
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile

private const val TAG = "AnimatedTextArea"

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AnimatedTextArea(
    targetId: String,
    value: String,
    onValueChange: (String) -> Unit,
    onCommit: (String) -> Unit,
    modifier: Modifier = Modifier,
    profile: TextEditorProfile = TextEditorProfile.ShortDescription,
    label: @Composable (() -> Unit)? = null,
    placeholder: @Composable (() -> Unit)? = null,
    enabled: Boolean = true,
    minLines: Int = profile.minLines,
    maxLines: Int = profile.maxLines,
    coordinator: AnimatedTextEditorCoordinator? = null
) {
    val effectiveCoordinator = coordinator ?: LocalAnimatedTextEditorCoordinator.current

    if (effectiveCoordinator != null) {
        AnimatedTextAreaWithCoordinator(
            targetId = targetId,
            value = value,
            onValueChange = onValueChange,
            onCommit = onCommit,
            modifier = modifier,
            profile = profile,
            label = label,
            placeholder = placeholder,
            enabled = enabled,
            minLines = minLines,
            maxLines = maxLines,
            coordinator = effectiveCoordinator
        )
    } else {
        LaunchedEffect(targetId) {
            Log.w(TAG, "AnimatedTextArea($targetId) has no AnimatedTextEditorCoordinator. " +
                "Falling back to OutlinedTextField. Provide a coordinator via CompositionLocal " +
                "or the coordinator parameter for full animated text editing support.")
        }
        OutlinedTextField(
            value = value,
            onValueChange = onValueChange,
            modifier = modifier,
            label = label,
            placeholder = placeholder,
            enabled = enabled,
            singleLine = false,
            minLines = minLines,
            maxLines = maxLines
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AnimatedTextAreaWithCoordinator(
    targetId: String,
    value: String,
    onValueChange: (String) -> Unit,
    onCommit: (String) -> Unit,
    modifier: Modifier,
    profile: TextEditorProfile,
    label: @Composable (() -> Unit)?,
    placeholder: @Composable (() -> Unit)?,
    enabled: Boolean,
    @Suppress("UNUSED_PARAMETER") minLines: Int,
    @Suppress("UNUSED_PARAMETER") maxLines: Int,
    coordinator: AnimatedTextEditorCoordinator
) {
    var localValue by remember(value) { mutableStateOf(value) }
    var isEditing by remember { mutableStateOf(false) }

    val currentOnValueChange by rememberUpdatedState(onValueChange)
    val currentOnCommit by rememberUpdatedState(onCommit)
    val currentValue by rememberUpdatedState(value)

    val target = remember(targetId) {
        EditableTextTarget(
            targetId = targetId,
            profile = profile,
            initialText = "",
            isPersistent = profile == TextEditorProfile.DocumentBody,
            onTextChanged = null,
            onCommit = null,
            onCancel = {
                isEditing = false
            },
            onEditingStateChanged = { state ->
                isEditing = state == EditingState.EDITING || state == EditingState.BINDING
            }
        )
    }

    LaunchedEffect(targetId) {
        coordinator.updateTargetSpec(
            targetId,
            onTextChanged = { newText ->
                localValue = newText
                currentOnValueChange(newText)
            },
            onCommit = { finalText ->
                localValue = finalText
                isEditing = false
                currentOnCommit(finalText)
            },
            onCancel = { isEditing = false },
            onEditingStateChanged = { state ->
                isEditing = state == EditingState.EDITING || state == EditingState.BINDING
            },
            profile = profile,
            currentText = currentValue
        )
    }

    LaunchedEffect(value) {
        if (value != localValue) {
            localValue = value
            coordinator.updateTargetText(targetId, value)
        }
    }

    DisposableEffect(targetId) {
        coordinator.registerTarget(target)
        onDispose {
            coordinator.unregisterTarget(targetId)
        }
    }

    val focusRequester = remember { FocusRequester() }

    Box(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = (minLines * 24).dp)
            .onGloballyPositioned { coordinates ->
                val position = coordinates.positionInWindow()
                val size = coordinates.size
                val rect = Rect(
                    position.x, position.y,
                    position.x + size.width, position.y + size.height
                )
                coordinator.updateTargetGeometry(targetId, rect.toAndroidRect())
            }
            .focusRequester(focusRequester)
            .focusable(enabled = enabled)
            .then(
                if (enabled) {
                    Modifier.pointerInput(Unit) {
                        awaitPointerEventScope {
                            while (true) {
                                val event = awaitPointerEvent()
                                if (event.changes.any { it.pressed }) {
                                    event.changes.forEach { it.consume() }
                                    if (!isEditing) {
                                        coordinator.updateTargetText(targetId, localValue)
                                        val cursorUtf8 = localValue.toByteArray(Charsets.UTF_8).size
                                        coordinator.beginEdit(targetId, cursorUtf8)
                                    } else {
                                        coordinator.getSharedEditorView()?.requestFocus()
                                    }
                                }
                            }
                        }
                    }
                } else {
                    Modifier
                }
            )
            .semantics {
                editableText = androidx.compose.ui.text.AnnotatedString(localValue)
                setText {
                    coordinator.updateTargetText(targetId, it.text)
                    coordinator.beginEdit(targetId, it.text.toByteArray(Charsets.UTF_8).size)
                    true
                }
            }
    ) {
        OutlinedTextFieldDefaults.DecorationBox(
            value = localValue,
            innerTextField = {
                Text(
                    text = localValue,
                    style = MaterialTheme.typography.bodyLarge,
                    color = if (isEditing) {
                        MaterialTheme.colorScheme.onSurface.copy(alpha = 0f)
                    } else {
                        MaterialTheme.colorScheme.onSurface
                    }
                )
            },
            enabled = enabled,
            singleLine = false,
            visualTransformation = androidx.compose.ui.text.input.VisualTransformation.None,
            interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() },
            label = label,
            placeholder = placeholder
        )
    }
}

private fun Rect.toAndroidRect(): android.graphics.Rect {
    return android.graphics.Rect(
        left.toInt(), top.toInt(), right.toInt(), bottom.toInt()
    )
}
