package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.clickable
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
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

    if (effectiveCoordinator == null) {
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = modifier
        )
        return
    }

    var localValue by remember(value) { mutableStateOf(value) }
    var isEditing by remember { mutableStateOf(false) }

    val target = remember(targetId) {
        EditableTextTarget(
            targetId = targetId,
            profile = profile,
            initialText = value,
            isPersistent = false,
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
                if (enabled && !isEditing) {
                    Modifier.clickable {
                        val cursorUtf8 = localValue.toByteArray(Charsets.UTF_8).size
                        effectiveCoordinator.beginEdit(targetId, cursorUtf8)
                    }
                } else {
                    Modifier
                }
            )
    ) {
        if (!isEditing) {
            Text(
                text = localValue,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface
            )
        } else {
            Text(
                text = localValue,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0f)
            )
        }
    }
}

private fun Rect.toAndroidRect(): android.graphics.Rect {
    return android.graphics.Rect(
        left.toInt(), top.toInt(), right.toInt(), bottom.toInt()
    )
}
