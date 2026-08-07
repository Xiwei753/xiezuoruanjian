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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.editor.v2.compose.LocalEditorWindowHost
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.ExternalContentDecision
import com.xiwei.sujian.editor.v2.coordinator.SessionResetSource
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.editor.v2.coordinator.WindowBindingState
import com.xiwei.sujian.editor.v2.coordinator.applyExternalContentFact
import com.xiwei.sujian.editor.v2.coordinator.shouldApplyExternalContent
import com.xiwei.sujian.runtime.LocalSujianAppDependencies
import com.xiwei.sujian.ui.EditorSettingsState
import com.xiwei.sujian.ui.EditorViewModel
import com.xiwei.sujian.ui.SaveStatus
import com.xiwei.sujian.ui.applyExternalContentToUi
import com.xiwei.sujian.ui.confirmEditorAttached
import com.xiwei.sujian.ui.isCurrentChapter
import com.xiwei.sujian.ui.notifySyncMergeConflict
import com.xiwei.sujian.ui.onContentChanged
import com.xiwei.sujian.ui.reloadSettings

/**
 * 正文编辑窗格 — 「正文」一级内容。
 *
 * 会话生命周期：
 * - DisposableEffect 注册/注销 target
 * - LaunchedEffect(chapterId) 处理章节切换收口（closeTarget 旧章节）
 * - LaunchedEffect(targetId, uiState.loading, sessionState.bindingState) 附着编辑器
 * - LaunchedEffect(targetId) 收集版本化文档事实（Repository 加载 / 同步合并）
 *
 * #595 一：输入窗口防护 — 只有 ViewModel 当前已提交章节（isCurrentChapter）
 * 才显示编辑器；切换事务提交后、导航落地前，旧 pane 不显示 View、
 * 不安装输入回调，旧章节最后一次输入不可能写进新章节。
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
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
) {
    val context = LocalContext.current
    val deps = LocalSujianAppDependencies.current
    val coordinator =
        LocalEditorWindowHost.current
            ?: throw IllegalStateException(
                "WritingPane requires an EditorWindowHost in the CompositionLocal. " +
                    "Ensure the host Activity or Fragment provides one via CompositionLocalProvider.",
            )
    val viewModel = rememberWritingPaneViewModel(context, deps, coordinator)
    val dims = LocalSujianDimensions.current

    val targetId =
        remember(projectId, volumeId, chapterId) {
            "chapter-body:$projectId:$volumeId:$chapterId"
        }

    val uiState by viewModel.uiState.collectAsStateWithLifecycle()

    // 生产动画链：设置状态 → Editor Host → 输入事务 → 动画协调器 → 真实 VSync 渲染。
    rememberMotionPolicySync(coordinator, uiState.settings, chapterId)
    rememberSettingsReload(viewModel, targetId)

    // #595 一：章节切换收口 — 宿主已用 requestOpenChapter 预提交章节
    // （保存/加载/session 预准备成功后才导航）；pane 只负责旧 target 的窗口解绑
    // （DisposableEffect onDispose 的 detachWindowBinding）。
    // #595 一：章节切换不业务关闭旧章节的持久 session — 快速连续点击章节时
    // 原章节的 Undo/Redo 历史保留（persistent session 留在 store 的 Detached
    // 状态）；返回章节列表/作品列表时由 ProjectWorkspaceScreen 以
    // WORKSPACE_NAVIGATION 关闭。
    // 深链/恢复路径（currentSession 不是本 pane 章节）仍走 switchChapter 事务。
    val chapter = ChapterRef(projectId, volumeId, chapterId, chapterTitle)
    val chapterState =
        rememberChapterSwitchSync(
            viewModel,
            chapter,
            targetId,
            onChapterSwitchFailed,
        )

    val sessionState by coordinator.sessionStateFlow.collectAsStateWithLifecycle()
    val currentUiState by rememberUpdatedState(uiState)
    val currentViewModel by rememberUpdatedState(viewModel)

    val target =
        rememberWritingPaneTarget(
            currentViewModel = currentViewModel,
            coordinator = coordinator,
            targetId = targetId,
            content = uiState.content,
        )

    // #595 二：按 target 分区的最新文档事实流（带 replay）— 新 collector 立即
    // 拿到当前文档事实，同 sourceVersion 重放由 reducer 幂等忽略。
    rememberExternalContentFlow(
        viewModel,
        coordinator,
        targetId,
        currentUiState,
        chapterState.failedSwitchTarget,
    )

    rememberEditorAttachSync(
        currentViewModel = currentViewModel,
        coordinator = coordinator,
        targetId = targetId,
        inputs = EditorAttachInputs(uiState, sessionState, chapterState, chapter),
    )

    WritingPaneColumn(
        modifier = modifier,
        chapterTitle = chapterTitle,
        uiState = uiState,
        showEditor = !uiState.loading && currentViewModel.isCurrentChapter(projectId, volumeId, chapterId),
        coordinator = coordinator,
        targetId = targetId,
    )
}

/** target 创建与输入回调绑定（输入回调经 rememberUpdatedState 始终指向最新 VM）。 */
@Composable
private fun rememberWritingPaneTarget(
    currentViewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost,
    targetId: String,
    content: String,
): EditableTextTarget {
    val target =
        remember(targetId) {
            EditableTextTarget(targetId = targetId)
        }
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
    target.updateText(content)

    DisposableEffect(targetId) {
        coordinator.registerTarget(target)
        onDispose {
            coordinator.detachWindowBinding(coordinator.windowId, targetId)
        }
    }
    return target
}

/** 编辑器附着所需的可观察状态（正文/会话/切换标记/章节身份）。 */
private data class EditorAttachInputs(
    val uiState: com.xiwei.sujian.ui.EditorUiState,
    val sessionState: com.xiwei.sujian.editor.v2.coordinator.EditorSessionState,
    val chapterState: ChapterSwitchSyncState,
    val chapter: ChapterRef,
)

/** 本地输入正文同步 + 编辑器附着（beginEdit / confirmEditorAttached）。 */
@Composable
private fun rememberEditorAttachSync(
    currentViewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost,
    targetId: String,
    inputs: EditorAttachInputs,
) {
    rememberTargetTextSync(currentViewModel, coordinator, targetId, inputs)
    rememberEditorAttach(currentViewModel, coordinator, targetId, inputs)
}

/** #595 二：本地输入的 target 正文同步 — onLocalEdit 先更新 sessionStateFlow，
 * 后通知 ViewModel 更新 uiState.content，sessionState.text 已与之一致。 */
@Composable
private fun rememberTargetTextSync(
    currentViewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost,
    targetId: String,
    inputs: EditorAttachInputs,
) {
    LaunchedEffect(inputs.uiState.content, inputs.chapter.chapterId) {
        if (inputs.chapterState.failedSwitchTarget == targetId) return@LaunchedEffect
        if (!inputs.uiState.loading &&
            inputs.sessionState.origin == com.xiwei.sujian.editor.v2.coordinator.EditorSessionOrigin.LOCAL_INPUT &&
            inputs.sessionState.text == inputs.uiState.content
        ) {
            coordinator.updateTargetText(targetId, inputs.uiState.content)
        }
    }
}

/** #595 一：编辑器附着 — 切换失败/未提交章节禁止 beginEdit；附着后解除输入冻结。 */
@Composable
private fun rememberEditorAttach(
    currentViewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost,
    targetId: String,
    inputs: EditorAttachInputs,
) {
    LaunchedEffect(targetId, inputs.uiState.loading, inputs.sessionState.bindingState) {
        // #595 一：切换失败的目标禁止 beginEdit。
        if (inputs.chapterState.failedSwitchTarget == targetId) return@LaunchedEffect
        // #595 一：只有 ViewModel 当前已提交章节才允许 beginEdit —
        // 切换事务提交后、业务选择/导航落地前的一帧内，旧 pane 不得用
        // 新章节正文对旧 target 创建/重置 session。
        if (!currentViewModel.isCurrentChapter(
                inputs.chapter.projectId,
                inputs.chapter.volumeId,
                inputs.chapter.chapterId,
            )
        ) {
            return@LaunchedEffect
        }
        val binding = inputs.sessionState.bindingState
        val alreadyAttached = binding is WindowBindingState.Attached && binding.targetId == targetId
        if (!alreadyAttached && !inputs.uiState.loading) {
            coordinator.updateTargetText(targetId, inputs.uiState.content)
            coordinator.beginEdit(targetId, inputs.uiState.content.toByteArray(Charsets.UTF_8).size)
        }
        // #595 一：新章节编辑器附着（或尝试附着）后解除输入冻结 —
        // 提交→导航窗口期内旧 pane 无法写入新章节；附着后输入恢复正常。
        currentViewModel.confirmEditorAttached(targetId)
    }
}

/** #595 一：显式 Factory 注入进程级容器依赖 + 会话层协调器 — 不再退回
 * WorkspaceRepository(getApplication()) 创建第二份容器。 */
@Composable
private fun rememberWritingPaneViewModel(
    context: android.content.Context,
    deps: com.xiwei.sujian.runtime.SujianAppDependencies,
    coordinator: com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost,
): EditorViewModel {
    val viewModel: EditorViewModel =
        viewModel(
            factory =
                EditorViewModel.Factory(
                    context.applicationContext as android.app.Application,
                    deps,
                    coordinator.sessionCoordinator,
                ),
        )
    LaunchedEffect(Unit) {
        viewModel.initialize(
            deps.workspaceRepository,
            deps.settingsRepository,
            deps.syncStatusRepository,
            coordinator.sessionCoordinator,
        )
    }
    return viewModel
}

/** 生产动画链：设置状态 → Editor Host → 输入事务 → 动画协调器 → 真实 VSync 渲染。 */
@Composable
private fun rememberMotionPolicySync(
    coordinator: com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost,
    settings: EditorSettingsState,
    chapterId: String,
) {
    LaunchedEffect(settings, chapterId) {
        // #595 三: 走 applyMotionPolicy 原子应用文字、光标、协同、时长和 reduce-motion。
        coordinator.applyMotionPolicy(
            com.xiwei.sujian.editor.v2.motion.EditorMotionPolicy(
                textEnabled = settings.typingAnimationEnabled,
                textDurationMillis = settings.typingAnimationDurationMs,
                cursorEnabled = settings.smoothCursorEnabled,
                cursorDurationMillis = settings.smoothCursorDurationMs,
                coordinated = settings.coordinatedTextCursorAnimationEnabled,
                reduceMotion = settings.reduceMotion,
            ),
        )
    }
}

/** 设置变更通过 CoreSettingsEvents.editorSettingsChanged SharedFlow 推送，
 * ON_RESUME 兜底处理进程恢复场景。 */
@Composable
private fun rememberSettingsReload(
    viewModel: EditorViewModel,
    targetId: String,
) {
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    DisposableEffect(targetId, lifecycleOwner) {
        val observer =
            androidx.lifecycle.LifecycleEventObserver { _, event ->
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
}

/** 章节切换收口：深链/恢复路径（currentSession 不是本 pane 章节）走 switchChapter 事务。 */
@Composable
private fun rememberChapterSwitchSync(
    viewModel: EditorViewModel,
    chapter: ChapterRef,
    targetId: String,
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )?,
): ChapterSwitchSyncState {
    var lastProjectId by remember { mutableStateOf("") }
    var lastVolumeId by remember { mutableStateOf("") }
    var lastChapterId by remember { mutableStateOf("") }
    var lastChapterTitle by remember { mutableStateOf("") }
    // #595 一：切换失败标记 — 阻止 beginEdit/外部替换协议用旧章节正文创建新章节
    // session（"新 target 使用旧内容创建 Rust session"）；回滚重组合到旧章节后自然失效。
    var failedSwitchTarget by remember { mutableStateOf<String?>(null) }
    val currentViewModel by rememberUpdatedState(viewModel)

    LaunchedEffect(chapter.projectId, chapter.volumeId, chapter.chapterId) {
        val sameChapter =
            lastChapterId.isNotEmpty() &&
                lastProjectId == chapter.projectId &&
                lastVolumeId == chapter.volumeId &&
                lastChapterId == chapter.chapterId
        if (!sameChapter) {
            if (currentViewModel.isCurrentChapter(chapter.projectId, chapter.volumeId, chapter.chapterId)) {
                // #595 一：宿主已预提交该章节 — 直接进入编辑（旧 target 由
                // DisposableEffect onDispose 解绑，session 保留）。
                failedSwitchTarget = null
            } else {
                when (
                    val result =
                        viewModel.switchChapter(
                            chapter.projectId,
                            chapter.volumeId,
                            chapter.chapterId,
                            chapter.title,
                        )
                ) {
                    is com.xiwei.sujian.ui.ChapterSwitchResult.Success -> {
                        // #595 一：只有保存+加载+session 预准备都成功，旧章节才
                        // 由事务提交（commitPreparedSession 保留其持久 session）；
                        // 窗口解绑由 DisposableEffect onDispose 完成。
                        failedSwitchTarget = null
                    }
                    is com.xiwei.sujian.ui.ChapterSwitchResult.SaveFailed,
                    is com.xiwei.sujian.ui.ChapterSwitchResult.LoadFailed,
                    -> {
                        // #595 一：保存/加载失败 → 回滚工作区选择到旧章节（activeChapterKey）。
                        failedSwitchTarget = targetId
                        onChapterSwitchFailed?.invoke(
                            lastProjectId.takeIf { it.isNotEmpty() } ?: chapter.projectId,
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
            lastProjectId = chapter.projectId
            lastVolumeId = chapter.volumeId
            lastChapterId = chapter.chapterId
            lastChapterTitle = chapter.title
        }
    }
    return ChapterSwitchSyncState(failedSwitchTarget)
}

/** 章节引用 — 当前 pane 的章节身份（用于切换同步与附着判断）。 */
private data class ChapterRef(val projectId: String, val volumeId: String, val chapterId: String, val title: String)

/** 章节切换同步所需的可观察状态（失败目标标记）。 */
private data class ChapterSwitchSyncState(val failedSwitchTarget: String?)

/**
 * #595 一/二：外部文档事实（RepositoryLoaded/SyncMerged）— 调用方已通过
 * shouldApplyExternalContent 确认版本更新与本地 dirty 状态，此处只执行
 * Core reset 和 UI 同步，不再重复构造事件做检查。
 */
@Composable
private fun rememberExternalContentFlow(
    viewModel: EditorViewModel,
    coordinator: com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost,
    targetId: String,
    currentUiState: com.xiwei.sujian.ui.EditorUiState,
    failedSwitchTarget: String?,
) {
    val currentViewModel by rememberUpdatedState(viewModel)
    val currentCoordinator by rememberUpdatedState(coordinator)
    val latestUiState by rememberUpdatedState(currentUiState)

    LaunchedEffect(targetId) {
        currentViewModel.documentUpdates(targetId).collect { fact ->
            if (latestUiState.loading) return@collect
            handleExternalDocumentFact(currentCoordinator, currentViewModel, targetId, fact)
        }
    }
}

/**
 * #595 一/二：外部文档事实决策执行 — 调用方已通过 shouldApplyExternalContent
 * 确认版本更新与本地 dirty 状态，此处只执行 Core reset 和 UI 同步。
 */
private fun handleExternalDocumentFact(
    coordinator: com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost,
    viewModel: EditorViewModel,
    targetId: String,
    fact: com.xiwei.sujian.editor.v2.coordinator.TargetDocumentFact,
) {
    when (val decision = coordinator.sessionCoordinator.shouldApplyExternalContent(fact)) {
        ExternalContentDecision.Apply -> {
            val resetResult =
                coordinator.resetPersistentSession(
                    targetId,
                    fact.text,
                    fact.text.toByteArray(Charsets.UTF_8).size,
                    SessionResetSource.EXTERNAL,
                )
            if (resetResult is com.xiwei.sujian.editor.v2.coordinator.ExternalResetResult.Success &&
                coordinator.activeTargetId != targetId
            ) {
                coordinator.beginEdit(targetId, fact.text.toByteArray(Charsets.UTF_8).size)
            }
            if (resetResult is com.xiwei.sujian.editor.v2.coordinator.ExternalResetResult.Success) {
                // #595 五：仅 Core reset 成功才一次性提交会话事实与 UI —
                // reset 失败时保持旧正文与旧版本，不得推进任何状态（旧实现
                // 无条件推进导致 Rust session/SessionStore/ViewModel 三份分裂）。
                coordinator.sessionCoordinator.applyExternalContentFact(fact)
                if (fact.origin == com.xiwei.sujian.editor.v2.coordinator.DocumentFactOrigin.SYNC_MERGED) {
                    // #595 三：同步合并同时更新 ViewModel 正文/hash/保存状态/字数 —
                    // 磁盘、Rust session、ViewModel 三方保持一致。
                    viewModel.applyExternalContentToUi(targetId, fact.text, fact.sourceVersion.contentHash)
                }
            }
        }
        ExternalContentDecision.IgnoreSameContent -> {
            // 正文已一致 — 只记录版本事实（幂等）。
            coordinator.sessionCoordinator.applyExternalContentFact(fact)
        }
        ExternalContentDecision.IgnoreDirtyConflict -> {
            // #595 二/三：本地未保存编辑存在 — 禁止直接 reset，
            // 发布类型化冲突（不覆盖用户输入）。
            if (fact.origin == com.xiwei.sujian.editor.v2.coordinator.DocumentFactOrigin.SYNC_MERGED) {
                viewModel.notifySyncMergeConflict()
            }
        }
        ExternalContentDecision.IgnoreReplay,
        ExternalContentDecision.IgnoreOlder,
        ExternalContentDecision.IgnoreEmptyVersion,
        -> {
            // 重放/更旧/无版本锚点 — 忽略。
        }
        ExternalContentDecision.IgnoreUncomparableConflict -> {
            // #595 五：不同版本但不可比较（无共同 revision 锚点/父链）—
            // 不得盲目覆盖；进入重新读取/三方合并/冲突路径（类型化通知）。
            if (fact.origin == com.xiwei.sujian.editor.v2.coordinator.DocumentFactOrigin.SYNC_MERGED) {
                viewModel.notifySyncMergeConflict()
            }
        }
    }
}

@Composable
private fun WritingPaneColumn(
    modifier: Modifier,
    chapterTitle: String,
    uiState: com.xiwei.sujian.ui.EditorUiState,
    showEditor: Boolean,
    coordinator: com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost,
    targetId: String,
) {
    val dims = LocalSujianDimensions.current
    Column(modifier = modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = dims.space16, vertical = dims.space4),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                chapterTitle,
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.weight(1f),
            )
            val statusSemanticValue =
                when (uiState.saveStatus) {
                    SaveStatus.Idle -> "idle"
                    SaveStatus.Unsaved -> "unsaved"
                    SaveStatus.Saving -> "saving"
                    SaveStatus.Saved -> "saved"
                    SaveStatus.SaveFailed -> "failed"
                }
            val statusText =
                when (uiState.saveStatus) {
                    SaveStatus.Idle -> ""
                    SaveStatus.Unsaved -> stringResource(id = R.string.status_unsaved)
                    SaveStatus.Saving -> stringResource(id = R.string.status_saving)
                    SaveStatus.Saved -> stringResource(id = R.string.status_saved)
                    SaveStatus.SaveFailed -> stringResource(id = R.string.status_save_failed)
                }
            Text(
                text = statusText,
                style = MaterialTheme.typography.bodySmall,
                modifier =
                    Modifier
                        .testTag(SujianSemanticIds.EditorSaveStatus)
                        .semantics { this.stateDescription = statusSemanticValue },
            )
        }

        if (uiState.wordCount > 0) {
            Text(
                stringResource(R.string.word_count_format, uiState.wordCount),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = dims.space16, vertical = dims.space2),
            )
        }

        // #595 一：只有 ViewModel 当前已提交章节才显示编辑器 —
        // 切换事务提交后、导航落地前，旧 pane 不显示 View（View 不在组合中，
        // 已安装的输入回调随 onRelease 清除），旧章节最后一次输入不可能写进新章节。
        if (!showEditor) {
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
