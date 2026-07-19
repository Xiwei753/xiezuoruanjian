package com.xiwei.sujian.editor.v2.compose

import android.util.Log
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
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile

private const val TAG = "AnimatedInlineText"

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
        LaunchedEffect(targetId) {
            Log.e(TAG, "AnimatedInlineText($targetId) has no AnimatedTextEditorCoordinator. " +
                "Falling back to static Text. Every Activity must provide a coordinator " +
                "via CompositionLocal or the coordinator parameter.")
        }
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

    val currentOnValueChange by rememberUpdatedState(onValueChange)
    val currentOnCommit by rememberUpdatedState(onCommit)
    val currentValue by rememberUpdatedState(value)

    val target = remember(targetId) {
        EditableTextTarget(
            targetId = targetId,
            profile = profile,
            isPersistent = false
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
                        awaitPointerEventScope {
                            while (true) {
                                val event = awaitPointerEvent()
                                if (event.changes.any { it.pressed }) {
                                    event.changes.forEach { it.consume() }
                                    effectiveCoordinator.updateTargetText(targetId, currentValue)
                                    val cursorUtf8 = currentValue.toByteArray(Charsets.UTF_8).size
                                    effectiveCoordinator.beginEdit(targetId, cursorUtf8)
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
                    effectiveCoordinator.updateTargetText(targetId, it.text)
                    effectiveCoordinator.beginEdit(targetId, it.text.toByteArray(Charsets.UTF_8).size)
                    true
                }
            }
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
