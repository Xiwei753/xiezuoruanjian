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
import com.xiwei.sujian.app.presentation.layout.AndroidLayoutSpec
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.WorkspaceLayoutMode
import com.xiwei.sujian.app.presentation.screen.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.requestOpenChapter
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import kotlinx.coroutines.ensureActive
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
    workbenchPlan: AndroidWorkbenchLayoutPlan?,
    chapterTreeCollapsed: Boolean,
    toolPaneCollapsed: Boolean,
    onToggleChapterTree: () -> Unit,
    onToggleToolPane: () -> Unit,
    chrome: SujianChromeSpec,
    onTopLevelSettings: () -> Unit,
    onTopLevelSearch: () -> Unit,
    onTopLevelSync: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val coroutineScope = rememberCoroutineScope()
    val context = LocalContext.current
    val deps = LocalSujianAppDependencies.current
    val projectRepository = deps.projectRepository
    val workspaceLayoutMode = layoutSpec.workspaceLayoutMode
    val isWideLayout = workspaceLayoutMode != WorkspaceLayoutMode.SINGLE_PANE

    // #628 评论 5301021120 02:59:39Z 版：plan + pane 收起状态打包传给工作台，
    // 避免 WideWritingWorkspace 参数超出门禁阈值。
    val workbenchLayoutState =
        WideWorkspaceLayoutState(
            workbenchPlan = workbenchPlan,
            chapterTreeCollapsed = chapterTreeCollapsed,
            toolPaneCollapsed = toolPaneCollapsed,
            onToggleChapterTree = onToggleChapterTree,
            onToggleToolPane = onToggleToolPane,
        )

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

    // #640 A.1/A.8：窄屏预准备的编辑器 target — 在 requestOpenChapter 成功后、awaitPresentationReady 之前设置，
    // 让 SinglePaneEditorLayer 提前进入组合并 layout，但 View.INVISIBLE。
    // 导航到 Editor 后 presentationVisible=true，View 立即可见，无首帧跳动。
    // 返回非 Editor（章节树/作品列表）时清空，释放编辑器层。
    var preparedEditorTarget by remember {
        mutableStateOf<PreparedEditorTarget?>(null)
    }
    var navigationRequestId by remember { mutableStateOf(0L) }
    LaunchedEffect(location) {
        if (location !is WorkspaceLocation.Editor) {
            preparedEditorTarget = null
            navigationRequestId++
        }
    }

    // #640 A.8/A.9：章节切换失败回滚回调 — 提取为共享变量，供窄屏和宽屏共用。
    val onChapterSwitchFailed: (
        (
            oldProjectId: String,
            oldVolumeId: String?,
            oldChapterId: String?,
            oldChapterTitle: String,
        ) -> Unit
    ) =
        { oldProjectId, oldVolumeId, oldChapterId, oldChapterTitle ->
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

            // 4. 用真实标题打开章节
            val requestId = ++navigationRequestId
            val result =
                editorViewModel.requestOpenChapter(
                    edit.projectId,
                    edit.volumeId,
                    edit.chapterId,
                    chapter.title,
                )
            when (result) {
                is ChapterSwitchResult.Success -> {
                    // #640 A.8：先设 preparedEditorTarget（让稳定层提前 layout），
                    // 再 awaitPresentationReady，再校验 target 未过期，最后 navigateToEditor。
                    val target =
                        PreparedEditorTarget(
                            projectId = edit.projectId,
                            volumeId = edit.volumeId,
                            chapterId = edit.chapterId,
                            chapterTitle = chapter.title,
                        )
                    if (isWideLayout) {
                        appState.selectProject(edit.projectId, projectTitle)
                        appState.selectChapter(edit.volumeId, edit.chapterId, chapter.title)
                        workspaceNavState.navigateToEditor(edit.projectId, edit.volumeId, edit.chapterId)
                    } else {
                        preparedEditorTarget = target
                        editorHost?.awaitPresentationReady(target.targetId)
                        kotlinx.coroutines.currentCoroutineContext().ensureActive()
                        if (navigationRequestId == requestId && preparedEditorTarget == target) {
                            appState.selectProject(edit.projectId, projectTitle)
                            appState.selectChapter(edit.volumeId, edit.chapterId, chapter.title)
                            workspaceNavState.navigateToEditor(edit.projectId, edit.volumeId, edit.chapterId)
                        }
                    }
                }
                // #630 评论15 项1：Stale 表示已被更新的打开请求替代，
                // 必须零 UI 副作用直接 return，避免和新请求的导航抢位。
                ChapterSwitchResult.Stale -> return@launch
                // #630 评论15 项1：SaveFailed 时章节切换事务已恢复旧 session/旧 UI，
                // 调用层不能再强制切 project + 清 chapter，保持原状态。
                is ChapterSwitchResult.SaveFailed -> return@launch
                // #630 评论15 项1：只有目标 LoadFailed（章节已删除）才回到该作品章节树。
                is ChapterSwitchResult.LoadFailed -> {
                    appState.selectProject(edit.projectId, projectTitle)
                    appState.clearChapterSelection()
                    workspaceNavState.navigateToChapterTree(edit.projectId)
                }
            }
        }
    }

    val currentProjectId = appState.currentProjectId
    val currentVolumeId = appState.currentVolumeId
    val currentChapterId = appState.currentChapterId
    val currentChapterTitle = appState.currentChapterTitle

    // #628 原则：窗口尺寸→布局决策唯一在 Rust — 通过 layoutSpec.workspaceLayoutMode。
    // 契约缺失（桥失败/空契约）时 fallback 到 SinglePane（与窄窗口基线一致）。
    // #628 验收点 4：作品卡片最小宽度来自 Rust LayoutMetrics.projectCardMinWidthDp。
    // isWideLayout=true 时 contract 必非 null（workspaceLayoutMode 缺失回落 SINGLE_PANE），
    // 因此 projectCardMinWidthDp 必非 null；SinglePane 时不画 grid，传 0f 占位。
    val projectCardMinWidthDp = layoutSpec.contract?.metrics?.projectCardMinWidthDp ?: 0f

    if (isWideLayout) {
        WideLayoutContent(
            appState = appState,
            workspaceNavState = workspaceNavState,
            projectListActions = projectListActions,
            projectWorkspaceActions = projectWorkspaceActions,
            chrome = chrome,
            location = location,
            currentProjectId = currentProjectId,
            currentVolumeId = currentVolumeId,
            currentChapterId = currentChapterId,
            currentChapterTitle = currentChapterTitle,
            projectRepository = projectRepository,
            editorViewModel = editorViewModel,
            projectCardMinWidthDp = projectCardMinWidthDp,
            workbenchLayoutState = workbenchLayoutState,
            onTopLevelSettings = onTopLevelSettings,
            onTopLevelSearch = onTopLevelSearch,
            onTopLevelSync = onTopLevelSync,
            onChapterSwitchFailed = onChapterSwitchFailed,
            onSelectProject = { projectId, projectTitle ->
                appState.selectProject(projectId, projectTitle)
                coroutineScope.launch {
                    workspaceNavState.navigateToChapterTree(projectId)
                }
            },
            onContinueRecentEdit = handleContinueRecentEdit,
            onSelectChapter = { volumeId, chapterId, chapterTitle ->
                if (currentProjectId != null) {
                    coroutineScope.launch {
                        val result =
                            editorViewModel.requestOpenChapter(
                                currentProjectId,
                                volumeId,
                                chapterId,
                                chapterTitle,
                            )
                        when (result) {
                            is ChapterSwitchResult.Success -> {
                                appState.selectChapter(volumeId, chapterId, chapterTitle)
                                workspaceNavState.navigateToEditor(currentProjectId, volumeId, chapterId)
                            }
                            is ChapterSwitchResult.SaveFailed,
                            is ChapterSwitchResult.LoadFailed,
                            ChapterSwitchResult.Stale,
                            -> {
                                // 错误提示已由 ViewModel 事件（toast）发出。
                            }
                        }
                    }
                }
            },
            onBack = {
                coroutineScope.launch { workspaceNavState.guardedBack() }
            },
            modifier = modifier,
        )
    } else {
        // #640 A.1：窄屏稳定 Box — preparedEditorTarget 非空时 SinglePaneEditorLayer 永远在同一 slot，
        // ProjectList/ChapterTree 在上层可见；隐藏 Editor View 仍 layout 但不可绘制/不可触摸；
        // 不用 alpha/GONE/AnimatedVisibility。
        Box(modifier = modifier.fillMaxSize()) {
            // 稳定层：预准备 target 非空时始终在组合中（预热阶段不可见，Editor 时立即可见）
            preparedEditorTarget?.let { target ->
                SinglePaneEditorLayer(
                    target = target,
                    presentationVisible = location is WorkspaceLocation.Editor,
                    onChapterSwitchFailed = onChapterSwitchFailed,
                    modifier = Modifier.fillMaxSize(),
                )
            }

            // 上层：非 Editor 时显示业务页面（Editor 时隐藏，不占 slot）
            if (location !is WorkspaceLocation.Editor) {
                SinglePaneContent(
                    appState = appState,
                    workspaceNavState = workspaceNavState,
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
                    onSelectChapter = { volumeId, chapterId, chapterTitle ->
                        if (currentProjectId != null) {
                            coroutineScope.launch {
                                val requestId = ++navigationRequestId
                                val result =
                                    editorViewModel.requestOpenChapter(
                                        currentProjectId,
                                        volumeId,
                                        chapterId,
                                        chapterTitle,
                                    )
                                when (result) {
                                    is ChapterSwitchResult.Success -> {
                                        // 先设 preparedEditorTarget，让稳定层提前 layout；
                                        // presentation 就绪后只提交仍是最新请求的导航。
                                        val target =
                                            PreparedEditorTarget(
                                                projectId = currentProjectId,
                                                volumeId = volumeId,
                                                chapterId = chapterId,
                                                chapterTitle = chapterTitle,
                                            )
                                        preparedEditorTarget = target
                                        editorHost?.awaitPresentationReady(target.targetId)
                                        kotlinx.coroutines.currentCoroutineContext().ensureActive()
                                        // 校验：协程未取消 + preparedEditorTarget 仍是当前 target + 请求仍是最新
                                        if (navigationRequestId == requestId &&
                                            preparedEditorTarget?.projectId == currentProjectId &&
                                            preparedEditorTarget?.volumeId == volumeId &&
                                            preparedEditorTarget?.chapterId == chapterId
                                        ) {
                                            appState.selectChapter(volumeId, chapterId, chapterTitle)
                                            workspaceNavState.navigateToEditor(currentProjectId, volumeId, chapterId)
                                        }
                                    }
                                    is ChapterSwitchResult.SaveFailed,
                                    is ChapterSwitchResult.LoadFailed,
                                    ChapterSwitchResult.Stale,
                                    -> {
                                        // 错误提示已由 ViewModel 事件（toast）发出；零导航副作用。
                                    }
                                }
                            }
                        }
                    },
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }
}

/**
 * 窄屏（SinglePane）业务内容 — 只画 ProjectList/ChapterTree（Editor 由 [SinglePaneEditorLayer] 稳定承载）。
 *
 * #640 A.1：Editor 分支已删除 — 编辑器现在由稳定 Box 中的 SinglePaneEditorLayer 统一承载，
 * 不再按 location 切换；本函数只负责非 Editor 业务页面。
 */
@Composable
private fun SinglePaneContent(
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
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
    if (location is WorkspaceLocation.ProjectList) {
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
    } else {
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
    }
}

/**
 * 大屏（Workbench）内容 — 根据业务位置画不同布局。
 *
 * - ProjectList 位置 → [ProjectListContent]（grid）；
 * - ChapterTree 位置 → [ChapterTreeContent] + 占位；
 * - Editor 位置 → [WideWritingWorkspace]（左章节树 + 中央编辑器 + 右工具面板）。
 */
@Composable
private fun WideLayoutContent(
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    projectListActions: AndroidWorkspaceActionSpec,
    projectWorkspaceActions: AndroidWorkspaceActionSpec,
    chrome: SujianChromeSpec,
    location: WorkspaceLocation,
    currentProjectId: String?,
    currentVolumeId: String?,
    currentChapterId: String?,
    currentChapterTitle: String,
    projectRepository: com.xiwei.sujian.feature.project.data.ProjectRepository,
    editorViewModel: EditorViewModel,
    projectCardMinWidthDp: Float,
    workbenchLayoutState: WideWorkspaceLayoutState,
    onTopLevelSettings: () -> Unit,
    onTopLevelSearch: () -> Unit,
    onTopLevelSync: () -> Unit,
    onChapterSwitchFailed: (
        (
            oldProjectId: String,
            oldVolumeId: String?,
            oldChapterId: String?,
            oldChapterTitle: String,
        ) -> Unit
    ),
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    onContinueRecentEdit: (edit: RecentEdit) -> Unit,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    onBack: () -> Unit,
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
        is WorkspaceLocation.Editor ->
            if (currentProjectId != null) {
                WideWritingWorkspace(
                    deps =
                        WideWorkspaceDeps(
                            appState = appState,
                            projectRepository = projectRepository,
                            projectWorkspaceActions = projectWorkspaceActions,
                            chrome = chrome,
                        ),
                    documentState =
                        WideWorkspaceDocumentState(
                            currentProjectId = currentProjectId,
                            currentVolumeId = currentVolumeId,
                            currentChapterId = currentChapterId,
                            currentChapterTitle = currentChapterTitle,
                        ),
                    editorViewModel = editorViewModel,
                    layoutState = workbenchLayoutState,
                    callbacks =
                        WideWorkspaceCallbacks(
                            onBack = onBack,
                            onSync = onTopLevelSync,
                            onSearch = onTopLevelSearch,
                            onSettings = onTopLevelSettings,
                            onChapterSwitchFailed = onChapterSwitchFailed,
                        ),
                    modifier = modifier.fillMaxSize(),
                )
            } else {
                // currentProjectId 为 null 但 location 是 Editor — 不可能发生，
                // 但稳妥起见回退到 ProjectList。
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
            }
    }
}
