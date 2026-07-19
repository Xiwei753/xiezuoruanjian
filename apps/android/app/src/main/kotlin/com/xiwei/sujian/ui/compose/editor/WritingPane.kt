package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.SessionResetSource
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.ui.EditorViewModel
import com.xiwei.sujian.ui.SaveStatus

@Composable
fun WritingPane(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val viewModel: EditorViewModel = viewModel()

    val coordinator = LocalAnimatedTextEditorCoordinator.current ?: remember {
        val bridge = BridgeProvider.getAppServiceBridge(context)
        AnimatedTextEditorCoordinator(context, bridge)
    }

    val targetId = remember(projectId, volumeId, chapterId) {
        "chapter-body:$projectId:$volumeId:$chapterId"
    }

    LaunchedEffect(projectId, volumeId, chapterId) {
        viewModel.switchChapter(projectId, volumeId, chapterId, chapterTitle)
    }

    val uiState by viewModel.uiState.collectAsState()

    var localContentGeneration by remember { mutableLongStateOf(0L) }
    var lastSeenContentGeneration by remember { mutableLongStateOf(0L) }
    var lastChapterId by remember { mutableStateOf("") }

    LaunchedEffect(uiState.content, chapterId) {
        if (!uiState.loading) {
            if (localContentGeneration != lastSeenContentGeneration) {
                lastSeenContentGeneration = localContentGeneration
                coordinator.updateTargetText(targetId, uiState.content)
            } else {
                if (coordinator.editingState == EditingState.IDLE) {
                    coordinator.updateTargetText(targetId, uiState.content)
                    val cursorUtf8 = uiState.content.toByteArray(Charsets.UTF_8).size
                    coordinator.beginEdit(targetId, cursorUtf8)
                } else if (coordinator.activeTargetId == targetId) {
                    coordinator.resetPersistentSession(
                        targetId,
                        uiState.content,
                        uiState.content.toByteArray(Charsets.UTF_8).size,
                        SessionResetSource.EXTERNAL
                    )
                }
            }
        }
    }

    LaunchedEffect(chapterId) {
        if (chapterId != lastChapterId && lastChapterId.isNotEmpty()) {
            if (coordinator.activeTargetId == targetId) {
                coordinator.cancelActiveEdit()
            }
            localContentGeneration = 0L
            lastSeenContentGeneration = 0L
        }
        lastChapterId = chapterId
    }

    val target = remember(targetId) {
        EditableTextTarget(
            targetId = targetId,
            profile = TextEditorProfile.DocumentBody,
            initialText = "",
            isPersistent = true,
            onTextChanged = null,
            onCommit = null,
            onCancel = {}
        )
    }

    DisposableEffect(targetId) {
        coordinator.registerTarget(target)
        onDispose {
            coordinator.unregisterTarget(targetId)
        }
    }

    LaunchedEffect(targetId) {
        coordinator.updateTargetSpec(
            targetId,
            onTextChanged = { newText ->
                localContentGeneration++
                viewModel.onContentChanged(newText)
            },
            onCommit = { finalText ->
                localContentGeneration++
                viewModel.onContentChanged(finalText)
            },
            onCancel = {}
        )
    }

    Column(modifier = modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                chapterTitle,
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.weight(1f)
            )
            val statusText = when (uiState.saveStatus) {
                SaveStatus.Idle -> ""
                SaveStatus.Unsaved -> stringResource(id = R.string.status_unsaved)
                SaveStatus.Saving -> stringResource(id = R.string.status_saving)
                SaveStatus.Saved -> stringResource(id = R.string.status_saved)
                SaveStatus.SaveFailed -> stringResource(id = R.string.status_save_failed)
            }
            if (statusText.isNotEmpty()) {
                Text(statusText, style = MaterialTheme.typography.bodySmall)
            }
        }

        if (uiState.wordCount > 0) {
            Text(
                stringResource(R.string.word_count_format, uiState.wordCount),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 2.dp)
            )
        }

        if (uiState.loading) {
            Box(modifier = Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        } else {
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .onGloballyPositioned { coordinates ->
                        val position = coordinates.positionInWindow()
                        val size = coordinates.size
                        val rect = Rect(
                            position.x, position.y,
                            position.x + size.width, position.y + size.height
                        )
                        coordinator.updateTargetGeometry(targetId, android.graphics.Rect(
                            rect.left.toInt(), rect.top.toInt(),
                            rect.right.toInt(), rect.bottom.toInt()
                        ))
                    }
            )
        }
    }
}
