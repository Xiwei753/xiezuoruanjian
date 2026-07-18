package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.interaction.FocusInteraction
import androidx.compose.foundation.interaction.MutableInteractionSource
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.data.BridgeProvider
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
    val context = LocalContext.current
    var localValue by remember(value) { mutableStateOf(value) }
    var isEditing by remember { mutableStateOf(false) }
    val interactionSource = remember { MutableInteractionSource() }

    val effectiveCoordinator = coordinator ?: LocalAnimatedTextEditorCoordinator.current ?: remember {
        val bridge = BridgeProvider.getAppServiceBridge(context)
        AnimatedTextEditorCoordinator(context, bridge)
    }

    val target = remember(targetId) {
        EditableTextTarget(
            targetId = targetId,
            profile = profile,
            initialText = value,
            isPersistent = profile == TextEditorProfile.DocumentBody,
            onTextChanged = { newText ->
                localValue = newText
                onValueChange(newText)
            },
            onCommit = { finalText ->
                localValue = finalText
                isEditing = false
                onCommit(finalText)
            },
            onCancel = {
                isEditing = false
            },
            onEditingStateChanged = { state ->
                isEditing = state == EditingState.EDITING || state == EditingState.BINDING
            }
        )
    }

    DisposableEffect(targetId) {
        effectiveCoordinator.registerTarget(target)
        onDispose {
            effectiveCoordinator.unregisterTarget(targetId)
        }
    }

    LaunchedEffect(value) {
        if (value != localValue) {
            localValue = value
        }
    }

    LaunchedEffect(interactionSource) {
        interactionSource.interactions.collect { interaction ->
            if (interaction is FocusInteraction.Focus && !isEditing && enabled) {
                val cursorUtf8 = localValue.toByteArray(Charsets.UTF_8).size
                effectiveCoordinator.beginEdit(targetId, cursorUtf8)
            }
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
                effectiveCoordinator.updateTargetGeometry(targetId, rect.toAndroidRect())
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
            interactionSource = interactionSource,
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
