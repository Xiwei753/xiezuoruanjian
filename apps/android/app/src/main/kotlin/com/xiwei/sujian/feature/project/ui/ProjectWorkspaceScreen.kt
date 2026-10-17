package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.requestOpenChapter
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * #641 评论 问题5：大屏 Workbench 真实布局链的展示状态打包 —
 * 由导航套件层统一解析（pane 收起 + chrome）后下发。
 *
 * - [chapterTreeCollapsed]/[toolPaneCollapsed] + 收起回调：pane 收起状态（Android 局部 UI 状态）；
 * - [chrome]：顶栏决策（showBack/actions），用于工具栏左/右组。
 */
internal data class WorkbenchPresentationState(
    val chapterTreeCollapsed: Boolean,
    val toolPaneCollapsed: Boolean,
    val onToggleChapterTree: () -> Unit,
    val onToggleToolPane: () -> Unit,
    val chrome: SujianChromeSpec?,
)

/**
 * 写作工作区 — 「作品」一级入口的唯一内容。
 *
 * 布局模式由 Android 顶层导航按本端窗口尺寸决定，并通过 [isWideLayout] 下发
 * （本组件不再自行判断窗口尺寸，也不向 Core 请求布局模式）：
 * - **窄屏**（SinglePane）：只画当前业务位置（稳定 Box 承载编辑器，无动画）；
 * - **大屏**（Workbench）：
 *   - ProjectList 位置 → [ProjectListContent]（grid）；
 *   - ChapterTree 位置 → [ChapterTreeContent] + 占位；
 *   - Editor 位置 → [WideWritingWorkspace]（左章节树 + 中央编辑器 + 右工具面板）。
 *
 * 不再用 [androidx.compose.material3.adaptive.layout.ListDetailPaneScaffold] 三窗格硬塞所有情况。
 *
 * [workspaceNavState] 由导航套件层创建并注入（#597：返回历史始终同一份）——
 * 顶栏返回、系统返回、预测返回和页面返回共用同一个 navigator；
 * 本组件不再自行创建第二份工作区导航状态。
 *
 * 工作区动作 spec 是 Android 本端常量（[WorkspaceActions]）：
 * - [WorkspaceActions.ProjectList] 用于作品列表（新建作品主操作/作品菜单）；
 * - [WorkspaceActions.ProjectWorkspace] 用于章节树（卷章创建/删除/重命名/排序）。
 */
@Composable
internal fun ProjectWorkspaceScreen(
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    isWideLayout: Boolean,
    projectCardMinWidthDp: Float,
    workbenchPresentation: WorkbenchPresentationState?,
    editorCallbacks: EditorWorkspaceCallbacks? = null,
    modifier: Modifier = Modifier,
) {
    val coroutineScope = rememberCoroutineScope()
    val context = LocalContext.current
    val deps = LocalSujianAppDependencies.current
    val projectRepository = deps.projectRepository

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
            // #641：flush 必须在 closeTarget 之前 — 先把屏幕最终内容（含 IME composition
            // 上屏）提交给 Core，再关闭 Rust session。closeTarget 后 session 失效，
            // flush 再调用 kernelBridge.replace 会失败。
            editorViewModel.finishWorkspaceClose(targetId)
            editorHost?.closeTarget(
                targetId,
                com.xiwei.sujian.feature.editor.projection.SessionCloseReason.WORKSPACE_NAVIGATION,
            )
        }
        when (location) {
            is WorkspaceLocation.ProjectList -> {
                if (appState.currentProjectId != null) {
                    appState.clearProjectSelection()
                    com.xiwei.sujian.core.interop.diagnostics.AppDiagnosticsEvents.workspaceBack("project_list")
                }
            }
            is WorkspaceLocation.ChapterTree -> {
                if (appState.currentChapterId != null) {
                    appState.clearChapterSelection()
                    com.xiwei.sujian.core.interop.diagnostics.AppDiagnosticsEvents.workspaceBack("chapter_tree")
                }
            }
            is WorkspaceLocation.Editor -> { }
        }
    }

    // requestOpenChapter 串行化（navigationRequestId/openChapterJob）
    var navigationRequestId by remember { mutableStateOf(0L) }
    var openChapterJob by remember { mutableStateOf<Job?>(null) }
    val openChapter: (
        projectId: String,
        projectTitle: String,
        volumeId: String,
        chapterId: String,
        chapterTitle: String,
        onLoadFailed: (() -> Unit)?,
    ) -> Unit =
        remember(coroutineScope, editorViewModel, editorHost, appState, workspaceNavState) {
            { projectId, projectTitle, volumeId, chapterId, chapterTitle, onLoadFailed ->
                // 新请求 cancel 旧请求 — 旧 requestOpenChapter 被 cancel。
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
                                // requestOpenChapter 成功后直接选择 project/chapter 并导航到 Editor
                                appState.selectProject(projectId, projectTitle)
                                appState.selectChapter(volumeId, chapterId, chapterTitle)
                                workspaceNavState.navigateToEditor(projectId, volumeId, chapterId)
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

    // 章节切换失败回滚回调 — remember 稳定，供窄屏/宽屏共用。
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

    if (isWideLayout) {
        WideLayoutContent(
            appState = appState,
            location = location,
            currentProjectId = currentProjectId,
            projectRepository = projectRepository,
            projectCardMinWidthDp = projectCardMinWidthDp,
            workbenchPresentation = workbenchPresentation,
            onSelectProject = { projectId, projectTitle ->
                appState.selectProject(projectId, projectTitle)
                coroutineScope.launch {
                    workspaceNavState.navigateToChapterTree(projectId)
                }
            },
            onContinueRecentEdit = handleContinueRecentEdit,
            onSelectChapter = { volumeId, chapterId, chapterTitle ->
                appState.currentProjectId?.let { projectId ->
                    val projectTitle =
                        appState.projectSummaries.firstOrNull { it.id == projectId }?.title.orEmpty()
                    openChapter(projectId, projectTitle, volumeId, chapterId, chapterTitle, null)
                }
            },
            onBack = { coroutineScope.launch { workspaceNavState.guardedBack() } },
            onChapterSwitchFailed = onChapterSwitchFailed,
            editorCallbacks = editorCallbacks,
            modifier = modifier,
        )
    } else {
        // #641 评论 问题6：SinglePaneContent.Editor 分支用 CompactWritingWorkspace，
        // 恢复完整写作顶栏（返回/搜索/同步/设置）。
        SinglePaneContent(
            appState = appState,
            location = location,
            projectCardMinWidthDp = projectCardMinWidthDp,
            onSelectProject = { projectId, projectTitle ->
                appState.selectProject(projectId, projectTitle)
                coroutineScope.launch {
                    workspaceNavState.navigateToChapterTree(projectId)
                }
            },
            onContinueRecentEdit = handleContinueRecentEdit,
            onSelectChapter = { volumeId, chapterId, chapterTitle ->
                appState.currentProjectId?.let { projectId ->
                    val projectTitle =
                        appState.projectSummaries.firstOrNull { it.id == projectId }?.title.orEmpty()
                    openChapter(projectId, projectTitle, volumeId, chapterId, chapterTitle, null)
                }
            },
            onBack = { coroutineScope.launch { workspaceNavState.guardedBack() } },
            onChapterSwitchFailed = onChapterSwitchFailed,
            editorCallbacks = editorCallbacks,
            modifier = modifier.fillMaxSize(),
        )
    }
}

/**
 * 窄屏（SinglePane）业务内容 — 画 ProjectList/ChapterTree/Editor。
 */
@Composable
private fun SinglePaneContent(
    appState: SujianAppState,
    location: WorkspaceLocation,
    projectCardMinWidthDp: Float,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    onContinueRecentEdit: (edit: RecentEdit) -> Unit,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    onBack: () -> Unit,
    onChapterSwitchFailed: (
        oldProjectId: String,
        oldVolumeId: String?,
        oldChapterId: String?,
        oldChapterTitle: String,
    ) -> Unit,
    editorCallbacks: EditorWorkspaceCallbacks? = null,
    modifier: Modifier = Modifier,
) {
    when (location) {
        is WorkspaceLocation.ProjectList ->
            ProjectListContent(
                appState = appState,
                workspaceActions = WorkspaceActions.ProjectList,
                onSelectProject = onSelectProject,
                onContinueRecentEdit = onContinueRecentEdit,
                modifier = modifier.fillMaxSize(),
                layoutConfig =
                    ProjectListLayoutConfig(
                        useWideGrid = false,
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
                        workspaceActions = WorkspaceActions.ProjectWorkspace,
                        onSelectChapter = onSelectChapter,
                        onError = appState::reportWorkspaceError,
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }
        is WorkspaceLocation.Editor -> {
            // #641 评论 问题6：Editor 位置绘制 CompactWritingWorkspace —
            // 完整写作顶栏（返回/搜索/同步/设置）+ 唯一 SujianEditorHost。
            // 不再只画 SujianEditorHost，窄屏正文 chrome 不再被拆掉。
            CompactWritingWorkspace(
                projectId = location.projectId,
                volumeId = location.volumeId,
                chapterId = location.chapterId,
                chapterTitle = appState.currentChapterTitle,
                onBack = onBack,
                onSearch = editorCallbacks?.onSearch ?: {},
                onSync = editorCallbacks?.onSync ?: {},
                onSettings = editorCallbacks?.onSettings ?: {},
                onChapterSwitchFailed = onChapterSwitchFailed,
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
}

/**
 * 大屏（Workbench）业务内容 — 只画 ProjectList/ChapterTree/Editor。
 */
@Composable
private fun WideLayoutContent(
    appState: SujianAppState,
    location: WorkspaceLocation,
    currentProjectId: String?,
    projectRepository: com.xiwei.sujian.feature.project.data.ProjectRepository,
    projectCardMinWidthDp: Float,
    workbenchPresentation: WorkbenchPresentationState?,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    onContinueRecentEdit: (edit: RecentEdit) -> Unit,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    onBack: () -> Unit,
    onChapterSwitchFailed: (
        oldProjectId: String,
        oldVolumeId: String?,
        oldChapterId: String?,
        oldChapterTitle: String,
    ) -> Unit,
    editorCallbacks: EditorWorkspaceCallbacks? = null,
    modifier: Modifier = Modifier,
) {
    when (location) {
        is WorkspaceLocation.ProjectList ->
            ProjectListContent(
                appState = appState,
                workspaceActions = WorkspaceActions.ProjectList,
                onSelectProject = onSelectProject,
                onContinueRecentEdit = onContinueRecentEdit,
                modifier = modifier.fillMaxSize(),
                layoutConfig =
                    ProjectListLayoutConfig(
                        useWideGrid = true,
                        projectCardMinWidthDp = projectCardMinWidthDp,
                    ),
            )
        is WorkspaceLocation.ChapterTree ->
            Box(modifier = modifier.fillMaxSize()) {
                if (currentProjectId != null) {
                    ChapterTreeContent(
                        projectId = currentProjectId,
                        projectRepository = projectRepository,
                        workspaceActions = WorkspaceActions.ProjectWorkspace,
                        onSelectChapter = onSelectChapter,
                        onError = appState::reportWorkspaceError,
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }
        is WorkspaceLocation.Editor -> {
            // 大屏 Editor 位置绘制 Compose-native WideWritingWorkspace。
            // pane 收起 + chrome 全部来自 workbenchPresentation。
            val deps =
                remember(appState, projectRepository, workbenchPresentation) {
                    WideWorkspaceDeps(
                        appState = appState,
                        projectRepository = projectRepository,
                        projectWorkspaceActions = WorkspaceActions.ProjectWorkspace,
                        chrome = workbenchPresentation?.chrome,
                    )
                }
            val layoutState =
                remember(workbenchPresentation) {
                    WideWorkspaceLayoutState(
                        chapterTreeCollapsed = workbenchPresentation?.chapterTreeCollapsed ?: false,
                        toolPaneCollapsed = workbenchPresentation?.toolPaneCollapsed ?: false,
                        onToggleChapterTree = workbenchPresentation?.onToggleChapterTree ?: {},
                        onToggleToolPane = workbenchPresentation?.onToggleToolPane ?: {},
                    )
                }
            val callbacks =
                remember(onBack, onSelectChapter, onChapterSwitchFailed, editorCallbacks) {
                    WideWorkspaceCallbacks(
                        onBack = onBack,
                        onSync = editorCallbacks?.onSync ?: {},
                        onSearch = editorCallbacks?.onSearch ?: {},
                        onSettings = editorCallbacks?.onSettings ?: {},
                        onChapterSwitch = onSelectChapter,
                        onChapterSwitchFailed = onChapterSwitchFailed,
                    )
                }
            WideWritingWorkspace(
                deps = deps,
                documentState =
                    WideWorkspaceDocumentState(
                        currentProjectId = location.projectId,
                        currentVolumeId = location.volumeId,
                        currentChapterId = location.chapterId,
                        currentChapterTitle = appState.currentChapterTitle,
                    ),
                layoutState = layoutState,
                callbacks = callbacks,
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
}
