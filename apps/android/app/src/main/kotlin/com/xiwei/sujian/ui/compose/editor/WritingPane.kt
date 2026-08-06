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
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier

import androidx.compose.ui.res.stringResource
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.editor.v2.compose.LocalEditorWindowHost
import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.SessionResetSource
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.ui.EditorViewModel
import com.xiwei.sujian.ui.SaveStatus
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.runtime.LocalSujianAppDependencies

/**
 * 章节正文编辑面板 — 连接 EditorViewModel 与 EditorWindowHost。
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
    val deps = LocalSujianAppDependencies.current
    LaunchedEffect(Unit) {
        viewModel.initialize(deps.workspaceRepository, deps.settingsRepository)
    }

    val coordinator = LocalEditorWindowHost.current
        ?: throw IllegalStateException(
            "WritingPane requires an EditorWindowHost in the CompositionLocal. " +
            "Ensure the host Activity or Fragment provides one via CompositionLocalProvider."
        )

    val dims = LocalSujianDimensions.current

    val targetId = remember(projectId, volumeId, chapterId) {
        "chapter-body:$projectId:$volumeId:$chapterId"
    }

    LaunchedEffect(projectId, volumeId, chapterId) {
        viewModel.switchChapter(projectId, volumeId, chapterId, chapterTitle)
    }

    val uiState by viewModel.uiState.collectAsStateWithLifecycle()

    // 生产动画链：设置状态 → Editor Host → 输入事务 → 动画协调器 → 真实 VSync 渲染。
    // 章节切换或设置变化后，立即把打字/光标动画设置推入共享 Editor Host，
    // 不依赖测试注入时钟，设置改变即时生效。
    LaunchedEffect(uiState.settings, chapterId) {
        val s = uiState.settings
        // #595 三: 走 applyMotionPolicy 原子应用文字、光标、协同、时长和 reduce-motion。
        coordinator.applyMotionPolicy(
            com.xiwei.sujian.editor.v2.motion.EditorMotionPolicy(
                textEnabled = s.typingAnimationEnabled,
                textDurationMillis = s.typingAnimationDurationMs,
                cursorEnabled = s.smoothCursorEnabled,
                cursorDurationMillis = s.smoothCursorDurationMs,
                coordinated = s.coordinatedTextCursorAnimationEnabled,
                reduceMotion = s.reduceMotion,
            )
        )
    }

    // 设置变更通过 CoreSettingsEvents.editorSettingsChanged SharedFlow 推送，
    // ON_RESUME 兜底处理进程恢复场景。
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    DisposableEffect(targetId, lifecycleOwner) {
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            if (event == androidx.lifecycle.Lifecycle.Event.ON_RESUME) {
                viewModel.reloadSettings()
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }
    LaunchedEffect(targetId) {
        com.xiwei.sujian.data.CoreSettingsEvents.editorSettingsChanged.collect {
            viewModel.reloadSettings()
        }
    }


    var lastChapterId by remember { mutableStateOf("") }
    var lastTargetId by remember { mutableStateOf("") }
    // #595 一：删除 localContentGeneration/lastSeenContentGeneration/externalContentHash —
    // 改用 EditorSessionState.text 和 origin 判断本地/外部更新，不依赖 revision 比较。

    val target = remember(targetId) {
        EditableTextTarget(targetId = targetId)
    }

    val currentViewModel by rememberUpdatedState(viewModel)

    target.onTextChanged = { newText ->
        // #595 一：本地输入不再维护 generation — onLocalEdit 已在 EditorWindowHost
        // 中更新 sessionStateFlow.revision，WritingPane 用 revision 判断来源。
        currentViewModel.onContentChanged(newText)
    }
    target.onCommit = { finalText ->
        currentViewModel.onContentChanged(finalText)
    }
    target.onCancel = {}
    target.updateProfile(TextEditorProfile.DocumentBody)
    target.updatePersistent(true)
    target.updateText(uiState.content)

    DisposableEffect(targetId) {
        coordinator.registerTarget(target)
        onDispose {
            coordinator.detachWindowBinding(coordinator.windowId, targetId)
        }
    }

    LaunchedEffect(chapterId) {
        if (chapterId != lastChapterId && lastChapterId.isNotEmpty()) {
            // #592 三：章节切换是业务级关闭 — 显式关闭旧章节持久 session
            // （不得从 DisposableEffect 推断业务对象是否结束）。
            if (lastTargetId.isNotEmpty()) {
                coordinator.closeTarget(lastTargetId, com.xiwei.sujian.editor.v2.coordinator.SessionCloseReason.CHAPTER_SWITCH)
            }
            coordinator.resetPersistentSession(
                targetId,
                uiState.content,
                uiState.content.toByteArray(Charsets.UTF_8).size,
                SessionResetSource.CHAPTER_SWITCH
            )
        }
        lastChapterId = chapterId
        lastTargetId = targetId
    }

    // #595 一：收集会话层唯一 SessionState — 用 revision 判断本地/外部更新。
    val sessionState by coordinator.sessionStateFlow.collectAsStateWithLifecycle()

    LaunchedEffect(uiState.content, chapterId) {
        if (!uiState.loading) {
            // #595 一：用 sessionState.text 和 origin 判断本地/外部更新，不依赖 revision 比较。
            // onLocalEdit 先更新 sessionStateFlow（text/revision/origin=LOCAL_INPUT），
            // 后通知 ViewModel 更新 uiState.content。当 LaunchedEffect 触发时，
            // sessionState.text 已与 uiState.content 一致 → 本地更新，不 reset。
            // 真实外部更新时 sessionState.text 与 uiState.content 不一致 → 外部更新，reset。
            if (sessionState.origin == com.xiwei.sujian.editor.v2.coordinator.EditorSessionOrigin.LOCAL_INPUT
                && sessionState.text == uiState.content
            ) {
                // 本地输入产生的 UI 回显 — sessionState 已更新，
                // 只同步 targetText，不触发 resetPersistentSession。
                coordinator.updateTargetText(targetId, uiState.content)
            } else if (uiState.content != sessionState.text) {
                // 真实外部更新（Repository/Sync/Undo）— 内容与 SessionState 不一致，
                // 确认是新的外部版本后执行 reset 协议。
                val externalUpdate = com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate.ExternalReplace(
                    targetId = targetId,
                    text = uiState.content,
                    revision = (sessionState.revision + 1).coerceAtLeast(1L),
                    source = com.xiwei.sujian.editor.v2.coordinator.ExternalSource.REPOSITORY_LOAD,
                )
                if (coordinator.sessionCoordinator.shouldApplyExternalReplace(externalUpdate)) {
                    coordinator.resetPersistentSession(
                        targetId,
                        uiState.content,
                        uiState.content.toByteArray(Charsets.UTF_8).size,
                        SessionResetSource.EXTERNAL
                    )
                    coordinator.sessionCoordinator.applyExternalReplace(externalUpdate)
                    if (coordinator.activeTargetId != targetId) {
                        coordinator.beginEdit(targetId, uiState.content.toByteArray(Charsets.UTF_8).size)
                    }
                }
            }
        }
    }

    LaunchedEffect(targetId, uiState.loading) {
        if (coordinator.activeTargetId != targetId && !uiState.loading) {
            coordinator.updateTargetText(targetId, uiState.content)
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
            val statusSemanticValue = when (uiState.saveStatus) {
                SaveStatus.Idle -> "idle"
                SaveStatus.Unsaved -> "unsaved"
                SaveStatus.Saving -> "saving"
                SaveStatus.Saved -> "saved"
                SaveStatus.SaveFailed -> "failed"
            }
            val statusText = when (uiState.saveStatus) {
                SaveStatus.Idle -> ""
                SaveStatus.Unsaved -> stringResource(id = R.string.status_unsaved)
                SaveStatus.Saving -> stringResource(id = R.string.status_saving)
                SaveStatus.Saved -> stringResource(id = R.string.status_saved)
                SaveStatus.SaveFailed -> stringResource(id = R.string.status_save_failed)
            }
            Text(
                text = statusText,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier
                    .testTag(SujianSemanticIds.EditorSaveStatus)
                    .semantics { this.stateDescription = statusSemanticValue }
            )
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
            @Suppress("UNUSED_EXPRESSION")
            (coordinator.targetDecorationsVersionFlow.collectAsStateWithLifecycle().value)
            com.xiwei.sujian.editor.v2.compose.WritingEditorSurface(
                coordinator = coordinator,
                targetId = targetId,
                modifier = Modifier.weight(1f).fillMaxWidth(),
            )
        }
    }
}
