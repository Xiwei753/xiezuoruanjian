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
    modifier: Modifier = Modifier,
    /**
     * #595 一：章节切换事务失败回调（保存失败/加载失败）。
     * 参数是回滚目标（旧章节）；旧章节不存在时传 null（应清除章节选择/回到章节树）。
     * 由工作区宿主把工作区导航回滚到 activeChapterKey，不能只把 loading 改回 false。
     */
    onChapterSwitchFailed: ((oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit)? = null,
) {
    val viewModel: EditorViewModel = viewModel()
    val deps = LocalSujianAppDependencies.current
    LaunchedEffect(Unit) {
        viewModel.initialize(deps.workspaceRepository, deps.settingsRepository, deps.syncStatusRepository)
    }

    val coordinator = LocalEditorWindowHost.current
        ?: throw IllegalStateException(
            "WritingPane requires an EditorWindowHost in the CompositionLocal. " +
            "Ensure the host Activity or Fragment provides one via CompositionLocalProvider."
        )

    // #595 二：注入 Coordinator 的全局 contentVersion 源到 ViewModel，
    // 使 RepositoryLoaded/SyncMerged 事件与 LocalInput/UndoRestored/ProgrammaticReplace
    // 共享同一递增序列，reducer 的 contentVersion 比较有效。
    LaunchedEffect(coordinator) {
        viewModel.setContentVersionSupplier { coordinator.sessionCoordinator.nextContentVersion() }
    }

    val dims = LocalSujianDimensions.current

    val targetId = remember(projectId, volumeId, chapterId) {
        "chapter-body:$projectId:$volumeId:$chapterId"
    }

    // 章节切换由下方的事务 LaunchedEffect 驱动（switchChapter 在事务内调用），
    // 这里不再 fire-and-forget 调用 — 避免同一个章节切换执行两次保存/加载。

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


    var lastProjectId by remember { mutableStateOf("") }
    var lastVolumeId by remember { mutableStateOf("") }
    var lastChapterId by remember { mutableStateOf("") }
    var lastChapterTitle by remember { mutableStateOf("") }
    var lastTargetId by remember { mutableStateOf("") }
    // #595 一：切换失败标记 — 阻止 beginEdit/外部替换协议用旧章节正文创建新章节
    // session（“新 target 使用旧内容创建 Rust session”）；回滚重组合到旧章节后自然失效。
    var failedSwitchTarget by remember { mutableStateOf<String?>(null) }
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

    // #595 一：章节切换事务 — 保存旧章节 → 加载新章节 → 提交成功才关闭旧章节 session；
    // 保存/加载失败时回滚工作区导航（onChapterSwitchFailed），旧 session 保留以便无损恢复。
    // 旧的 LaunchedEffect(chapterId) 立即 closeTarget(CHAPTER_SWITCH) 已删除：
    // 切换失败时旧 session 必须先保留，不能提前关闭。
    LaunchedEffect(projectId, volumeId, chapterId) {
        val sameChapter = lastChapterId.isNotEmpty() &&
            lastProjectId == projectId && lastVolumeId == volumeId && lastChapterId == chapterId
        if (!sameChapter) {
            if (currentViewModel.isCurrentChapter(projectId, volumeId, chapterId)) {
                // #595 一：宿主已用 requestOpenChapter 预提交该章节（事务成功后才导航）—
                // 直接收口旧 target 并进入编辑，不再重复执行保存/加载事务；
                // 也防止过期 pane 的请求在更新请求之后运行造成串写（旧正文写新章节）。
                if (lastTargetId.isNotEmpty() && lastTargetId != targetId) {
                    coordinator.closeTarget(lastTargetId, com.xiwei.sujian.editor.v2.coordinator.SessionCloseReason.CHAPTER_SWITCH)
                }
                failedSwitchTarget = null
            } else {
                when (val result = viewModel.switchChapter(projectId, volumeId, chapterId, chapterTitle)) {
                    is com.xiwei.sujian.ui.ChapterSwitchResult.Success -> {
                        // #595 一：只有保存+加载都成功，旧章节才是业务级关闭（CHAPTER_SWITCH）。
                        if (lastTargetId.isNotEmpty() && lastTargetId != targetId) {
                            coordinator.closeTarget(lastTargetId, com.xiwei.sujian.editor.v2.coordinator.SessionCloseReason.CHAPTER_SWITCH)
                        }
                        failedSwitchTarget = null
                    }
                    is com.xiwei.sujian.ui.ChapterSwitchResult.SaveFailed,
                    is com.xiwei.sujian.ui.ChapterSwitchResult.LoadFailed -> {
                        // #595 一：保存/加载失败 → 回滚工作区选择到旧章节（activeChapterKey）。
                        // 在回滚完成前禁止 beginEdit/外部替换协议使用旧正文创建新章节 session。
                        failedSwitchTarget = targetId
                        onChapterSwitchFailed?.invoke(
                            lastProjectId.takeIf { it.isNotEmpty() } ?: projectId,
                            lastVolumeId.takeIf { it.isNotEmpty() },
                            lastChapterId.takeIf { it.isNotEmpty() },
                            lastChapterTitle,
                        )
                    }
                    com.xiwei.sujian.ui.ChapterSwitchResult.Stale -> {
                        // #595 一：请求已过期 — 更新的请求正在完成切换，本请求不再动作。
                    }
                }
            }
            lastProjectId = projectId
            lastVolumeId = volumeId
            lastChapterId = chapterId
            lastChapterTitle = chapterTitle
            lastTargetId = targetId
        }
    }

    // #595 一：收集会话层唯一 SessionState — 用 revision 判断本地/外部更新。
    val sessionState by coordinator.sessionStateFlow.collectAsStateWithLifecycle()

    // #595 一：收集 Repository 真实来源事件（真实 fileHash）— 章节加载完成时
    // ViewModel 发出，执行外部替换协议；不再根据字符串差异伪造 revision/source。
    val currentUiState by rememberUpdatedState(uiState)

    // #595 二：外部内容变更（RepositoryLoaded/SyncMerged）— 调用方已通过
    // shouldApplyRepositoryLoad/shouldApplyExternalUpdate 确认版本更新，
    // 此处只执行 Core reset 和 beginEdit，不再重复构造事件做检查。
    fun applyExternalContent(text: String, fileHash: String) {
        coordinator.resetPersistentSession(
            targetId,
            text,
            text.toByteArray(Charsets.UTF_8).size,
            SessionResetSource.EXTERNAL
        )
        if (coordinator.activeTargetId != targetId) {
            coordinator.beginEdit(targetId, text.toByteArray(Charsets.UTF_8).size)
        }
    }

    LaunchedEffect(targetId) {
        // #595 二：按 target 分区的最新事件流（带 replay）— 新 collector 立即
        // 拿到当前最新事件，不再经过单消费者 Channel 被错误页面取走。
        currentViewModel.documentUpdates(targetId).collect { update ->
            if (currentUiState.loading) return@collect
            // #595 二：WritingPane 只消费类型化 EditorDocumentUpdate 事件，
            // 不再观察字符串差异。所有外部更新来源（RepositoryLoaded/SyncMerged/
            // UndoRestored/ProgrammaticReplace）统一经 shouldApplyExternalUpdate
            // 判断版本新旧，确认属于当前章节且版本更新时才执行一次 Core reset。
            when (update) {
                is com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate.RepositoryLoaded -> {
                    if (coordinator.sessionCoordinator.shouldApplyRepositoryLoad(update)) {
                        applyExternalContent(update.text, update.fileHash)
                        coordinator.sessionCoordinator.applyRepositoryLoaded(update)
                    }
                }
                is com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate.SyncMerged -> {
                    if (coordinator.sessionCoordinator.shouldApplyExternalUpdate(update)) {
                        applyExternalContent(update.text, update.fileHash)
                        coordinator.sessionCoordinator.applySyncMerged(update)
                    }
                }
                is com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate.UndoRestored,
                is com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate.ProgrammaticReplace -> {
                    // 撤销/恢复和程序化替换通过 onExternalEdit 同步回调已直接更新 SessionState，
                    // 不走异步 documentUpdates 事件流。此处仅作类型穷尽守卫。
                }
                is com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate.LocalInput -> {
                    // 本地输入通过 onLocalEdit 同步回调已直接更新 SessionState，
                    // 不走异步 documentUpdates 事件流。此处仅作类型穷尽守卫。
                }
            }
        }
    }

    LaunchedEffect(uiState.content, chapterId) {
        // #595 二：WritingPane 只消费类型化 EditorDocumentUpdate 事件，不再观察
        // 字符串差异后自行触发外部 reset。外部正文变更（同步/撤销/程序化替换）
        // 全部由 LaunchedEffect(targetId) 收集 viewModel.documentUpdates 事件驱动，
        // 携带真实 fileHash 和 contentVersion，由 reducer 判断版本新旧。
        // 此处只处理本地输入的 target 正文同步 — onLocalEdit 先更新 sessionStateFlow，
        // 后通知 ViewModel 更新 uiState.content，sessionState.text 已与之一致。
        if (failedSwitchTarget == targetId) return@LaunchedEffect
        if (!uiState.loading &&
            sessionState.origin == com.xiwei.sujian.editor.v2.coordinator.EditorSessionOrigin.LOCAL_INPUT &&
            sessionState.text == uiState.content
        ) {
            coordinator.updateTargetText(targetId, uiState.content)
        }
    }

    LaunchedEffect(targetId, uiState.loading) {
        // #595 一：切换失败的目标禁止 beginEdit — 否则会用旧章节正文创建新章节 session。
        if (failedSwitchTarget == targetId) return@LaunchedEffect
        // #595 一：只有 ViewModel 当前已提交章节才允许 beginEdit —
        // 切换事务提交后、业务选择/导航落地前的一帧内，旧 pane 不得用
        // 新章节正文对旧 target 创建/重置 session。
        if (!currentViewModel.isCurrentChapter(projectId, volumeId, chapterId)) return@LaunchedEffect
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
