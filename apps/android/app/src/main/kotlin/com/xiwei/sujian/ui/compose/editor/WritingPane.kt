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
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow

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
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import androidx.compose.ui.viewinterop.AndroidView

/**
 * 章节正文编辑面板 — 连接 EditorViewModel 与 AnimatedTextEditorCoordinator。
 *
 * 核心职责：
 * - 将 ViewModel 的 content 状态同步到 Coordinator 的持久会话
 * - 将 Coordinator 的输入法编辑结果（onTextChanged/onCommit）回传 ViewModel
 * - 章节切换时通过 resetPersistentSession 重置会话（而非 cancelForSession），
 *   保留输入法连接避免闪烁
 * - 外部内容变更（同步/撤销）通过 contentHash 检测并重置会话
 *
 * 会话生命周期：
 * - DisposableEffect 注册/注销 target
 * - LaunchedEffect(chapterId) 处理章节切换
 * - LaunchedEffect(uiState.content) 处理外部内容变更
 * - LaunchedEffect(targetId) 首次激活编辑会话
 */
@Composable
fun WritingPane(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
    modifier: Modifier = Modifier
) {
    val viewModel: EditorViewModel = viewModel()

    val coordinator = LocalAnimatedTextEditorCoordinator.current
        ?: throw IllegalStateException(
            "WritingPane requires an AnimatedTextEditorCoordinator in the CompositionLocal. " +
            "Ensure the host Activity or Fragment provides one via CompositionLocalProvider."
        )

    val dims = LocalSujianDimensions.current

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
    var externalContentHash by remember { mutableLongStateOf(0L) }

    val target = remember(targetId) {
        EditableTextTarget(targetId = targetId)
    }

    val currentViewModel by rememberUpdatedState(viewModel)

    target.onTextChanged = { newText ->
        localContentGeneration++
        lastSeenContentGeneration = localContentGeneration
        currentViewModel.onContentChanged(newText)
    }
    target.onCommit = { finalText ->
        localContentGeneration++
        lastSeenContentGeneration = localContentGeneration
        currentViewModel.onContentChanged(finalText)
    }
    target.onCancel = {}
    target.updateProfile(TextEditorProfile.DocumentBody)
    target.updatePersistent(true)
    target.updateText(uiState.content)

    DisposableEffect(targetId) {
        coordinator.registerTarget(target)
        onDispose {
            coordinator.unregisterTarget(targetId)
        }
    }

    LaunchedEffect(chapterId) {
        if (chapterId != lastChapterId && lastChapterId.isNotEmpty()) {
            if (coordinator.activeTargetId == targetId) {
                coordinator.cancelActiveEdit()
            }
            coordinator.resetPersistentSession(
                targetId,
                uiState.content,
                uiState.content.toByteArray(Charsets.UTF_8).size,
                SessionResetSource.CHAPTER_SWITCH
            )
            localContentGeneration = 0L
            lastSeenContentGeneration = 0L
            externalContentHash = 0L
        }
        lastChapterId = chapterId
    }

    LaunchedEffect(uiState.content, chapterId) {
        if (!uiState.loading) {
            if (localContentGeneration != lastSeenContentGeneration) {
                lastSeenContentGeneration = localContentGeneration
                coordinator.updateTargetText(targetId, uiState.content)
            } else {
                val contentHash = uiState.content.hashCode().toLong()
                if (externalContentHash != contentHash) {
                    externalContentHash = contentHash
                    val kernelText = coordinator.getTargetText(targetId)
                    if (kernelText != uiState.content) {
                        coordinator.resetPersistentSession(
                            targetId,
                            uiState.content,
                            uiState.content.toByteArray(Charsets.UTF_8).size,
                            SessionResetSource.EXTERNAL
                        )
                        if (coordinator.activeTargetId != targetId) {
                            coordinator.beginEdit(targetId, uiState.content.toByteArray(Charsets.UTF_8).size)
                        }
                    }
                }
            }
        }
    }

    LaunchedEffect(targetId) {
        if (coordinator.activeTargetId != targetId && !uiState.loading) {
            coordinator.beginEdit(targetId, uiState.content.toByteArray(Charsets.UTF_8).size)
        }
    }

    Column(modifier = modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = dims.space16, vertical = dims.space4),
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
                modifier = Modifier.padding(horizontal = dims.space16, vertical = dims.space2)
            )
        }

        if (uiState.loading) {
            Box(modifier = Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        } else {
            val isActiveTarget = coordinator.activeTargetId == targetId
            @Suppress("UNUSED_EXPRESSION")
            (coordinator.targetDecorationsVersion)
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
                        if (!isActiveTarget) {
                            val projection = coordinator.getTargetProjection(targetId)
                            if (projection != null) {
                                projection.setWidth(size.width.toFloat())
                            }
                        }
                    }
            ) {
                if (!isActiveTarget) {
                    val projection = coordinator.getTargetProjection(targetId)
                    if (projection != null && projection.getText().isNotEmpty()) {
                        ReadonlyChapterPreview(projection = projection)
                    }
                }
            }
        }
    }
}
