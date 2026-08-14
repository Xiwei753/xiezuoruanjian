package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.presentation.AndroidWorkspaceActionSpec
import com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.requestOpenChapter
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.editor.ui.SujianEditorHost
import kotlinx.coroutines.launch

/**
 * 写作工作区 — 「作品」一级入口的唯一内容。
 *
 * 作品、卷、章节和正文是同一个工作区内的不同数据和窗格状态：
 * - 当前选择（project/volume/chapter）由 [SujianAppState] 单状态持有，不创建全局 route。
 * - 三窗格由 [ListDetailPaneScaffold] 自适应排列：
 *   - listPane → 作品列表（[ProjectListContent]）
 *   - detailPane → 卷章节树（[ChapterTreeContent]）
 *   - extraPane → 正文编辑器（[SujianEditorHost]）
 * - 窄窗口（手机竖屏）依次显示单窗格；宽窗口（平板/横屏/折叠屏）并排展开多窗格。
 * - 窗口变宽只改变排列，不切换到另一套页面。
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
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
internal fun ProjectWorkspaceScreen(
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    projectListActions: AndroidWorkspaceActionSpec,
    projectWorkspaceActions: AndroidWorkspaceActionSpec,
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

    // #597：工作区 navigator 由导航套件层创建并注入（唯一实例），
    // 会话恢复的初始历史已经由套件层根据业务选择一次性构建。
    val navigator = workspaceNavState.navigator

    // #592 三：workspace 导航离开正文时业务级关闭章节 session。
    // 配置变化不会改变 workspace route（navigator 历史由 rememberSaveable 保留），
    // 因此不会触发关闭；只有用户真正返回章节列表/作品列表或切换章节才关闭。
    var lastWorkspaceLocation by remember {
        mutableStateOf<WorkspaceLocation?>(null)
    }
    // 会话业务选择与导航位置的对账：导航位置回退到章节列表时清 chapter 选择，
    // 回退到作品列表时清 project 选择；进入正文不清除任何选择。
    // #624 评论12 第1项：离开正文的保存已由 guardedBack（ActiveDocumentGate
    // flush）在导航提交前完成 — 这里不再补保存（旧实现在导航完成后才调用
    // saveTargetBeforeClose，保存失败只能阻止 closeTarget，阻止不了导航本身）。
    // 导航成功以后只做两件事：关闭已经成功离开的 target，并同步通知 ViewModel
    // 完成业务关闭（currentSession=null，避免"Rust session 已关闭，ViewModel
    // 仍宣称 A 是当前章节"）。
    LaunchedEffect(workspaceNavState.currentLocation) {
        val location = workspaceNavState.currentLocation
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

    val currentProjectId = appState.currentProjectId
    val currentVolumeId = appState.currentVolumeId
    val currentChapterId = appState.currentChapterId
    val currentChapterTitle = appState.currentChapterTitle

    val isEditor = workspaceNavState.currentLocation is WorkspaceLocation.Editor

    ListDetailPaneScaffold(
        modifier = modifier,
        directive = navigator.scaffoldDirective,
        scaffoldState = navigator.scaffoldState,
        listPane = {
            AnimatedPane {
                ProjectListContent(
                    appState = appState,
                    workspaceActions = projectListActions,
                    onSelectProject = { projectId, projectTitle ->
                        appState.selectProject(projectId, projectTitle)
                        coroutineScope.launch {
                            workspaceNavState.navigateToChapterTree(projectId)
                        }
                    },
                )
            }
        },
        detailPane = {
            AnimatedPane {
                if (currentProjectId != null) {
                    ChapterTreeContent(
                        projectId = currentProjectId,
                        projectRepository = projectRepository,
                        workspaceActions = projectWorkspaceActions,
                        onSelectChapter = { volumeId, chapterId, chapterTitle ->
                            // #595 一：事务成功后才提交业务选择和 Navigator —
                            // 保存/加载失败时 Navigator 完全不变化，不再"先导航再回滚"。
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
                                    -> {
                                        // 数据失败：停在章节树，旧章节 session/状态保留；
                                        // 错误提示已由 ViewModel 事件（toast）发出。
                                    }
                                    ChapterSwitchResult.Stale -> {
                                        // 更新的请求正在完成切换，本请求不再动作。
                                    }
                                }
                            }
                        },
                        // #617 评论九：章节树错误事件转交全局 Snackbar —
                        // 复用 #614 已有的 WorkspaceUiEvent.Error 链，不在 feature 里另起系统。
                        onError = appState::reportWorkspaceError,
                    )
                }
            }
        },
        extraPane = {
            AnimatedPane {
                // 正文背景层延伸绘制到状态栏和顶栏下方；文字/光标/手势区域保留明确顶部安全区。
                Box(
                    modifier =
                        Modifier
                            .fillMaxSize()
                            .background(MaterialTheme.colorScheme.background),
                ) {
                    if (currentProjectId != null && currentChapterId != null && currentVolumeId != null) {
                        SujianEditorHost(
                            projectId = currentProjectId,
                            volumeId = currentVolumeId,
                            chapterId = currentChapterId,
                            chapterTitle = currentChapterTitle,
                            modifier =
                                Modifier
                                    .fillMaxSize()
                                    .padding(top = if (isEditor) 0.dp else 0.dp),
                            // #595 一：章节切换保存/加载失败 → 回滚选择到旧章节；
                            // 无旧章节（首次进入失败）→ 清除章节选择并返回章节树。
                            onChapterSwitchFailed = { oldProjectId, oldVolumeId, oldChapterId, oldChapterTitle ->
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
                            },
                        )
                    } else {
                        Box(
                            modifier = Modifier.fillMaxSize(),
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(stringResource(id = R.string.select_chapter_to_write))
                        }
                    }
                }
            }
        },
    )
}
