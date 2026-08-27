package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.presentation.layout.AndroidLayoutSpec
import com.xiwei.sujian.app.presentation.layout.WorkspaceLayoutMode
import com.xiwei.sujian.app.presentation.screen.AndroidWorkspaceActionSpec
import com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.requestOpenChapter
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * 写作工作区 — 「作品」一级入口的唯一内容。
 *
 * #625 第二段 / #628 验收点 1：根据 [AndroidLayoutSpec.contract.workspaceLayoutMode]
 * （**不自己判断窗口尺寸**）+ [WorkspaceLocation] 决定画什么：
 * - **窄屏**（SinglePane）：只画当前业务位置（稳定 Box + SinglePaneEditorLayer 承载编辑器，无动画）；
 * - **大屏**（Workbench）：
 *   - ProjectList 位置 → [ProjectListContent]（grid）；
 *   - ChapterTree 位置 → [ChapterTreeContent] + 占位；
 *   - Editor 位置 → [WideWritingWorkspace]（左章节树 + 中央编辑器 + 右工具面板）。
 *
 * 不再用 [androidx.compose.material3.adaptive.layout.ListDetailPaneScaffold] 三窗格硬塞所有情况。
 *
 * #628 原则：窗口尺寸→布局决策唯一在 Rust — 判断窄屏/大屏必须通过
 * `layoutSpec.contract.workspaceLayoutMode`，不得引入 600/840/1200/1600 或 WindowWidthSizeClass 断点。
 *
 * [workspaceNavState] 由导航套件层创建并注入（#597：返回历史始终同一份）——
 * 顶栏返回、系统返回、预测返回和页面返回共用同一个 navigator；
 * 本组件不再自行创建第二份工作区导航状态。
 *
 * #618 一：两份动作 spec 都由容器创建时解析的 PresentationPolicyCatalog 固定提供，
 * 不再依赖父层组合帧观察到的 navigator 位置：
 * - [projectListActions] 固定按 PROJECT_LIST 契约（新建作品主操作/作品菜单）；
 * - [projectWorkspaceActions] 固定按 PROJECT_WORKSPACE 契约（卷章创建/删除/重命名/排序）。
 */
@Composable
internal fun ProjectWorkspaceScreen(
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    projectListActions: AndroidWorkspaceActionSpec,
    projectWorkspaceActions: AndroidWorkspaceActionSpec,
    layoutSpec: AndroidLayoutSpec,
    onPreparedEditorTargetChanged: (PreparedEditorTarget?) -> Unit,
    onEditorCallbacksChanged: (EditorPresentationCallbacks) -> Unit,
    modifier: Modifier = Modifier,
) {
    val coroutineScope = rememberCoroutineScope()
    val context = LocalContext.current
    val deps = LocalSujianAppDependencies.current
    val projectRepository = deps.projectRepository
    val workspaceLayoutMode = layoutSpec.workspaceLayoutMode
    val isWideLayout = workspaceLayoutMode != WorkspaceLayoutMode.SINGLE_PANE

    // #595 一：章节切换事务入口 — 显式 Factory 注入进程级容器依赖 + 会话层协调器。
    // 与 WritingPane 内 viewModel(factory=...) 解析到同一 Activity 级实例。
    val editorHost = LocalEditorWindowHost.current
    val editorViewModel: EditorViewModel =
        viewModel(
            factory =
                EditorViewModel.Factory(
                    context.applicationContext as android.app.Application,
                    deps,
                    editorHost?.sessionCoordinator,
                ),
        )
    // #595 一：尽早初始化 — 章节树里的 requestOpenChapter 需要在 WritingPane
    // 组合前就具备 Repository 与 session 协调器（提交前预准备 Rust session）。
    LaunchedEffect(Unit) {
        editorViewModel.initialize(
            deps.projectRepository,
            deps.settingsRepository,
            deps.syncRepository,
            deps.syncStatusRepository,
            editorHost?.sessionCoordinator,
        )
    }

    // #592 三：workspace 导航离开正文时业务级关闭章节 session。
    var lastWorkspaceLocation by remember {
        mutableStateOf<WorkspaceLocation?>(null)
    }
    // 会话业务选择与导航位置的对账：导航位置回退到章节列表时清 chapter 选择，
    // 回退到作品列表时清 project 选择；进入正文不清除任何选择。
    // #624 评论12 第1项：离开正文的保存已由 guardedBack（ActiveDocumentGate
    // flush）在导航提交前完成 — 这里不再补保存。
    val location = workspaceNavState.currentLocation
    LaunchedEffect(workspaceNavState.currentLocation) {
        val previous = lastWorkspaceLocation
        lastWorkspaceLocation = location
        val previousEditor = previous as? WorkspaceLocation.Editor
        if (previousEditor != null && location !is WorkspaceLocation.Editor) {
            val targetId =
                "chapter-body:${previousEditor.projectId}:${previousEditor.volumeId}:${previousEditor.chapterId}"
            editorHost?.closeTarget(
                targetId,
                com.xiwei.sujian.feature.editor.session.SessionCloseReason.WORKSPACE_NAVIGATION,
            )
            editorViewModel.finishWorkspaceClose(targetId)
        }
        when (location) {
            is WorkspaceLocation.ProjectList -> {
                if (appState.currentProjectId != null) {
                    appState.clearProjectSelection()
                    com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.workspaceBack("project_list")
                }
            }
            is WorkspaceLocation.ChapterTree -> {
                if (appState.currentChapterId != null) {
                    appState.clearChapterSelection()
                    com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.workspaceBack("chapter_tree")
                }
            }
            is WorkspaceLocation.Editor -> { }
        }
    }

    // #640 A：preparedEditorTarget state 已上移到 SujianNavigationSuite（唯一 Compose 状态源）。
    // 本组件只保留 requestOpenChapter 串行化（navigationRequestId/openChapterJob），
    // 成功后通过 onPreparedEditorTargetChanged 提交 target；suite 接管 awaitPresentationReady + navigate。
    var navigationRequestId by remember { mutableStateOf(0L) }
    var openChapterJob by remember { mutableStateOf<Job?>(null) }
    val onPreparedEditorTargetChangedRef = rememberUpdatedState(onPreparedEditorTargetChanged)
    val openChapter: (
        projectId: String,
        projectTitle: String,
        volumeId: String,
        chapterId: String,
        chapterTitle: String,
        onLoadFailed: (() -> Unit)?,
    ) -> Unit =
        remember(coroutineScope, editorViewModel, editorHost) {
            { projectId, projectTitle, volumeId, chapterId, chapterTitle, onLoadFailed ->
                // #640 B：新请求 cancel 旧请求 — 旧 requestOpenChapter 被 cancel。
                openChapterJob?.cancel()
                openChapterJob =
                    coroutineScope.launch {
                        val requestId = ++navigationRequestId
                        val result =
                            editorViewModel.requestOpenChapter(
                                projectId,
                                volumeId,
                                chapterId,
                                chapterTitle,
                            )
                        when (result) {
                            is ChapterSwitchResult.Success -> {
                                val target =
                                    PreparedEditorTarget(
                                        projectId = projectId,
                                        projectTitle = projectTitle,
                                        volumeId = volumeId,
                                        chapterId = chapterId,
                                        chapterTitle = chapterTitle,
                                    )
                                // #640 A：只提交 target；suite 接管 awaitPresentationReady + navigate。
                                onPreparedEditorTargetChangedRef.value(target)
                            }
                            is ChapterSwitchResult.SaveFailed,
                            ChapterSwitchResult.Stale,
                            -> {
                                // 错误提示已由 ViewModel 事件（toast）发出。
                            }
                            is ChapterSwitchResult.LoadFailed -> {
                                if (navigationRequestId == requestId) {
                                    onLoadFailed?.invoke()
                                }
                            }
                        }
                    }
            }
        }

    // #640 A.8/A.9：章节切换失败回滚回调 — remember 稳定，供窄屏/宽屏/host 共用。
    val onChapterSwitchFailed: (
        oldProjectId: String,
        oldVolumeId: String?,
        oldChapterId: String?,
        oldChapterTitle: String,
    ) -> Unit =
        remember(appState, coroutineScope, workspaceNavState) {
            { oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String ->
                if (oldVolumeId != null && oldChapterId != null) {
                    appState.selectChapter(oldVolumeId, oldChapterId, oldChapterTitle)
                    coroutineScope.launch {
                        workspaceNavState.navigateToEditor(oldProjectId, oldVolumeId, oldChapterId)
                    }
                } else {
                    appState.clearChapterSelection()
                    coroutineScope.launch {
                        workspaceNavState.guardedBack()
                    }
                }
            }
        }

    // #640 A：宽屏章节切换回调 — remember 稳定，上传给 host 供 WideWritingWorkspace 章节树使用。
    val onChapterSwitch: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit =
        remember(appState, openChapter) {
            { volumeId: String, chapterId: String, chapterTitle: String ->
                appState.currentProjectId?.let { projectId ->
                    val projectTitle =
                        appState.projectSummaries.firstOrNull { it.id == projectId }?.title.orEmpty()
                    openChapter(projectId, projectTitle, volumeId, chapterId, chapterTitle, null)
                }
            }
        }

    // #640 A：EditorPresentationCallbacks — remember 稳定后 SideEffect 上传给 suite，
    // 供 EditorPresentationHost 宽屏 WideWritingWorkspace 使用。不新建第二 ViewModel/状态源。
    val editorCallbacks =
        remember(onChapterSwitch, onChapterSwitchFailed) {
            EditorPresentationCallbacks(
                onChapterSwitch = onChapterSwitch,
                onChapterSwitchFailed = onChapterSwitchFailed,
            )
        }
    SideEffect {
        onEditorCallbacksChanged(editorCallbacks)
    }

    // #630 评论12 项2 + 评论13 项1 + 评论15 项1：「继续写作」— 先在 IO 调度器解析真实章节标题，
    // 再传给 requestOpenChapter，避免空标题提交进 EditorViewModel 事务。
    val handleContinueRecentEdit: (RecentEdit) -> Unit = { edit ->
        coroutineScope.launch {
            // 1. 作品标题来自内存，不阻塞主线程
            val projectTitle =
                appState.projectSummaries.firstOrNull { it.id == edit.projectId }?.title.orEmpty()

            // 2. 章节列表只读一次（IO 调度器），不重复读取
            val chapter =
                withContext(kotlinx.coroutines.Dispatchers.IO) {
                    projectRepository.getChapters(edit.projectId, edit.volumeId)
                        ?.firstOrNull { it.id == edit.chapterId }
                }

            // 3. 章节不存在时回退到作品章节树（不进入编辑器）
            if (chapter == null) {
                appState.selectProject(edit.projectId, projectTitle)
                appState.clearChapterSelection()
                workspaceNavState.navigateToChapterTree(edit.projectId)
                return@launch
            }

            // 4. 用真实标题走统一章节打开流程
            openChapter(edit.projectId, projectTitle, edit.volumeId, edit.chapterId, chapter.title) {
                appState.selectProject(edit.projectId, projectTitle)
                appState.clearChapterSelection()
                workspaceNavState.navigateToChapterTree(edit.projectId)
            }
        }
    }

    val currentProjectId = appState.currentProjectId

    // #628 原则：窗口尺寸→布局决策唯一在 Rust — 通过 layoutSpec.workspaceLayoutMode。
    // 契约缺失（桥失败/空契约）时 fallback 到 SinglePane（与窄窗口基线一致）。
    // #628 验收点 4：作品卡片最小宽度来自 Rust LayoutMetrics.projectCardMinWidthDp。
    // isWideLayout=true 时 contract 必非 null（workspaceLayoutMode 缺失回落 SINGLE_PANE），
    // 因此 projectCardMinWidthDp 必非 null；SinglePane 时不画 grid，传 0f 占位。
    val projectCardMinWidthDp = layoutSpec.contract?.metrics?.projectCardMinWidthDp ?: 0f

    if (isWideLayout) {
        // #640 A：宽屏只画 ProjectList/ChapterTree；Editor 由 EditorPresentationHost（suite sibling）接管。
        WideLayoutContent(
            appState = appState,
            projectListActions = projectListActions,
            projectWorkspaceActions = projectWorkspaceActions,
            location = location,
            currentProjectId = currentProjectId,
            projectRepository = projectRepository,
            projectCardMinWidthDp = projectCardMinWidthDp,
            onSelectProject = { projectId, projectTitle ->
                appState.selectProject(projectId, projectTitle)
                coroutineScope.launch {
                    workspaceNavState.navigateToChapterTree(projectId)
                }
            },
            onContinueRecentEdit = handleContinueRecentEdit,
            onSelectChapter = onChapterSwitch,
            modifier = modifier,
        )
    } else {
        // #640 A：窄屏只画 ProjectList/ChapterTree；Editor 由 EditorPresentationHost（suite sibling）接管。
        SinglePaneContent(
            appState = appState,
            projectListActions = projectListActions,
            projectWorkspaceActions = projectWorkspaceActions,
            location = location,
            workspaceLayoutMode = workspaceLayoutMode,
            projectCardMinWidthDp = projectCardMinWidthDp,
            onSelectProject = { projectId, projectTitle ->
                appState.selectProject(projectId, projectTitle)
                coroutineScope.launch {
                    workspaceNavState.navigateToChapterTree(projectId)
                }
            },
            onContinueRecentEdit = handleContinueRecentEdit,
            onSelectChapter = onChapterSwitch,
            modifier = modifier.fillMaxSize(),
        )
    }
}

/**
 * 窄屏（SinglePane）业务内容 — 只画 ProjectList/ChapterTree。
 *
 * #640 A：Editor 由 EditorPresentationHost（suite sibling）接管，本函数 Editor 位置留空。
 */
@Composable
private fun SinglePaneContent(
    appState: SujianAppState,
    projectListActions: AndroidWorkspaceActionSpec,
    projectWorkspaceActions: AndroidWorkspaceActionSpec,
    location: WorkspaceLocation,
    workspaceLayoutMode: WorkspaceLayoutMode,
    projectCardMinWidthDp: Float,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    onContinueRecentEdit: (edit: RecentEdit) -> Unit,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    when (location) {
        is WorkspaceLocation.ProjectList ->
            ProjectListContent(
                appState = appState,
                workspaceActions = projectListActions,
                onSelectProject = onSelectProject,
                onContinueRecentEdit = onContinueRecentEdit,
                modifier = modifier.fillMaxSize(),
                layoutConfig =
                    ProjectListLayoutConfig(
                        workspaceLayoutMode = workspaceLayoutMode,
                        projectCardMinWidthDp = projectCardMinWidthDp,
                    ),
            )
        is WorkspaceLocation.ChapterTree ->
            Box(modifier = modifier.fillMaxSize()) {
                val projectId = appState.currentProjectId
                if (projectId != null) {
                    ChapterTreeContent(
                        projectId = projectId,
                        projectRepository = LocalSujianAppDependencies.current.projectRepository,
                        workspaceActions = projectWorkspaceActions,
                        onSelectChapter = onSelectChapter,
                        onError = appState::reportWorkspaceError,
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }
        is WorkspaceLocation.Editor -> {
            // #640 A：Editor 由 EditorPresentationHost（suite sibling）接管，此处留空。
        }
    }
}

/**
 * 大屏（Workbench）业务内容 — 只画 ProjectList/ChapterTree。
 *
 * #640 A：Editor 由 EditorPresentationHost（suite sibling）接管，Editor 位置留空。
 */
@Composable
private fun WideLayoutContent(
    appState: SujianAppState,
    projectListActions: AndroidWorkspaceActionSpec,
    projectWorkspaceActions: AndroidWorkspaceActionSpec,
    location: WorkspaceLocation,
    currentProjectId: String?,
    projectRepository: com.xiwei.sujian.feature.project.data.ProjectRepository,
    projectCardMinWidthDp: Float,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    onContinueRecentEdit: (edit: RecentEdit) -> Unit,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    when (location) {
        is WorkspaceLocation.ProjectList ->
            ProjectListContent(
                appState = appState,
                workspaceActions = projectListActions,
                onSelectProject = onSelectProject,
                onContinueRecentEdit = onContinueRecentEdit,
                modifier = modifier.fillMaxSize(),
                layoutConfig =
                    ProjectListLayoutConfig(
                        workspaceLayoutMode = WorkspaceLayoutMode.WORKBENCH,
                        projectCardMinWidthDp = projectCardMinWidthDp,
                    ),
            )
        is WorkspaceLocation.ChapterTree ->
            Box(modifier = modifier.fillMaxSize()) {
                if (currentProjectId != null) {
                    ChapterTreeContent(
                        projectId = currentProjectId,
                        projectRepository = projectRepository,
                        workspaceActions = projectWorkspaceActions,
                        onSelectChapter = onSelectChapter,
                        onError = appState::reportWorkspaceError,
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }
        is WorkspaceLocation.Editor -> {
            // #640 A：Editor 由 EditorPresentationHost（suite sibling）接管，此处留空。
        }
    }
}
