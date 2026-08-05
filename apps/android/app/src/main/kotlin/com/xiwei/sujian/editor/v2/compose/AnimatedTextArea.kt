package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.gestures.detectTapGestures
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
import androidx.compose.ui.platform.testTag
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
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost
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
    coordinator: EditorWindowHost? = null
) {
    val effectiveCoordinator = coordinator ?: LocalEditorWindowHost.current
        ?: throw IllegalStateException(
            "AnimatedTextArea($targetId) requires an EditorWindowHost. " +
            "Every Activity must provide one via CompositionLocal or the coordinator parameter."
        )
    val dims = LocalSujianDimensions.current

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
        coordinator = effectiveCoordinator,
        minLineHeight = dims.bodyLineHeight
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
    coordinator: EditorWindowHost,
    minLineHeight: Dp
) {
    var localValue by remember { mutableStateOf(value) }
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
    target.updatePersistent(currentProfile == TextEditorProfile.DocumentBody)
    target.updateText(currentValue)

    LaunchedEffect(value) {
        if (value != localValue) {
            localValue = value
            selectionRange = TextRange(value.length)
            coordinator.updateTargetText(targetId, value)
        }
    }

    DisposableEffect(targetId) {
        coordinator.registerTarget(target)
        onDispose {
            coordinator.detachTarget(targetId)
        }
    }

    Box(
        modifier = modifier
            .testTag(targetId)
            .fillMaxWidth()
            .heightIn(min = minLineHeight * minLines)
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
                        detectTapGestures {
                            coordinator.updateTargetText(targetId, currentValue)
                            val cursorUtf8 = currentValue.toByteArray(Charsets.UTF_8).size
                            coordinator.beginEdit(targetId, cursorUtf8)
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
                            coordinator.updateTargetText(targetId, newText)
                            coordinator.beginEdit(targetId, newText.toByteArray(Charsets.UTF_8).size)
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
                            coordinator.updateTargetText(targetId, newText)
                            coordinator.beginEdit(targetId, utf8Offset)
                            refOnValueChanged(newText)
                            true
                        }
                        setSelection { selStart, selEnd, _ ->
                            val safeStart = TextOffsetUtils.safeCharIndex(localValue, selStart)
                            val safeEnd = TextOffsetUtils.safeCharIndex(localValue, selEnd)
                            selectionRange = TextRange(safeStart, safeEnd)
                            val utf8Start = TextOffsetUtils.utf8OffsetForCharIndex(localValue, selStart)
                            coordinator.updateTargetText(targetId, localValue)
                            coordinator.beginEdit(targetId, utf8Start)
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
