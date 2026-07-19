package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
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
        ?: throw IllegalStateException(
            "AnimatedTextArea($targetId) requires an AnimatedTextEditorCoordinator. " +
            "Every Activity must provide one via CompositionLocal or the coordinator parameter."
        )

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
            isPersistent = profile == TextEditorProfile.DocumentBody
        )
    }

    target.onTextChanged = { newText ->
        localValue = newText
        currentOnValueChange(newText)
    }
    target.onCommit = { finalText ->
        localValue = finalText
        isEditing = false
        currentOnCommit(finalText)
    }
    target.onCancel = { isEditing = false }
    target.onEditingStateChanged = { state ->
        isEditing = state == EditingState.EDITING || state == EditingState.BINDING
    }
    target.updateProfile(profile)
    target.updatePersistent(profile == TextEditorProfile.DocumentBody)
    target.updateText(currentValue)

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
            .then(
                if (enabled) {
                    Modifier.pointerInput(targetId) {
                        awaitPointerEventScope {
                            while (true) {
                                val event = awaitPointerEvent()
                                if (event.changes.any { it.pressed }) {
                                    event.changes.forEach { it.consume() }
                                    coordinator.updateTargetText(targetId, currentValue)
                                    val cursorUtf8 = currentValue.toByteArray(Charsets.UTF_8).size
                                    coordinator.beginEdit(targetId, cursorUtf8)
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
