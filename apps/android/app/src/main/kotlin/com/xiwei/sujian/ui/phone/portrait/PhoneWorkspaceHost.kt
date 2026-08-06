package com.xiwei.sujian.ui.phone.portrait

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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.ui.compose.SujianSessionAppState
import com.xiwei.sujian.ui.compose.editor.SujianEditorHost
import com.xiwei.sujian.ui.compose.workspace.ChapterTreeContent
import com.xiwei.sujian.ui.compose.workspace.ProjectListContent
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun PhoneWorkspaceHost(
    workspaceNavState: PhoneWorkspaceNavigationState,
    sessionViewModel: WorkspaceSessionViewModel,
    workspaceRepository: WorkspaceRepository,
    editorTopSafeArea: Dp,
    modifier: Modifier = Modifier,
) {
    val coroutineScope = rememberCoroutineScope()
    val sessionAppState = remember { SujianSessionAppState(sessionViewModel) }

    // #592 三：workspace 导航离开正文时业务级关闭章节 session。
    // 配置变化不会改变 workspace route（navigator 历史由 rememberSaveable 保留），
    // 因此不会触发关闭；只有用户真正返回章节列表/作品列表或切换章节才关闭。
    var lastWorkspaceLocation by remember {
        mutableStateOf<WorkspaceLocation?>(null)
    }
    // 窗口层宿主在组合中读取；LaunchedEffect 内不能读取 CompositionLocal。
    val editorHost = com.xiwei.sujian.editor.v2.compose.LocalEditorWindowHost.current

    // 会话业务选择与导航位置的对账：导航位置回退到章节列表时清 chapter 选择，
    // 回退到作品列表时清 project 选择；进入正文不清除任何选择。
    LaunchedEffect(workspaceNavState.currentLocation) {
        val location = workspaceNavState.currentLocation
        val previous = lastWorkspaceLocation
        lastWorkspaceLocation = location
        val previousEditor = previous as? WorkspaceLocation.Editor
        if (previousEditor != null && location !is WorkspaceLocation.Editor) {
            editorHost?.closeTarget(
                "chapter-body:${previousEditor.projectId}:${previousEditor.volumeId}:${previousEditor.chapterId}",
                com.xiwei.sujian.editor.v2.coordinator.SessionCloseReason.WORKSPACE_NAVIGATION,
            )
        }
        when (location) {
            is WorkspaceLocation.ProjectList -> {
                if (sessionViewModel.currentProjectId != null) {
                    sessionViewModel.clearProjectSelection()
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("project_list")
                }
            }
            is WorkspaceLocation.ChapterTree -> {
                if (sessionViewModel.currentChapterId != null) {
                    sessionViewModel.clearChapterSelection()
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("chapter_tree")
                }
            }
            is WorkspaceLocation.Editor -> { }
        }
    }

    val currentProjectId = sessionViewModel.currentProjectId
    val currentVolumeId = sessionViewModel.currentVolumeId
    val currentChapterId = sessionViewModel.currentChapterId
    val currentChapterTitle = sessionViewModel.currentChapterTitle

    val isEditor = workspaceNavState.currentLocation is WorkspaceLocation.Editor

    ListDetailPaneScaffold(
        modifier = modifier,
        directive = workspaceNavState.navigator.scaffoldDirective,
        scaffoldState = workspaceNavState.navigator.scaffoldState,
        listPane = {
            AnimatedPane {
                ProjectListContent(
                    appState = sessionAppState,
                    onSelectProject = { projectId, projectTitle ->
                        sessionViewModel.selectProject(projectId, projectTitle)
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
                        workspaceRepository = workspaceRepository,
                        onSelectChapter = { volumeId, chapterId, chapterTitle ->
                            sessionViewModel.selectChapter(volumeId, chapterId, chapterTitle)
                            coroutineScope.launch {
                                workspaceNavState.navigateToEditor(currentProjectId, volumeId, chapterId)
                            }
                        },
                        onBackToProjects = {
                            coroutineScope.launch {
                                workspaceNavState.back()
                            }
                        },
                    )
                }
            }
        },
        extraPane = {
            AnimatedPane {
                // 正文背景层延伸绘制到状态栏和顶栏下方；文字/光标/手势区域保留明确顶部安全区。
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(MaterialTheme.colorScheme.background),
                ) {
                    if (currentProjectId != null && currentChapterId != null && currentVolumeId != null) {
                        SujianEditorHost(
                            projectId = currentProjectId,
                            volumeId = currentVolumeId,
                            chapterId = currentChapterId,
                            chapterTitle = currentChapterTitle,
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(top = if (isEditor) editorTopSafeArea else 0.dp),
                            // #595 一：章节切换保存/加载失败 → 回滚选择到旧章节；
                            // 无旧章节（首次进入失败）→ 清除章节选择并返回章节树。
                            onChapterSwitchFailed = { oldProjectId, oldVolumeId, oldChapterId, oldChapterTitle ->
                                if (oldVolumeId != null && oldChapterId != null) {
                                    sessionViewModel.selectChapter(oldVolumeId, oldChapterId, oldChapterTitle)
                                    coroutineScope.launch {
                                        workspaceNavState.navigateToEditor(oldProjectId, oldVolumeId, oldChapterId)
                                    }
                                } else {
                                    sessionViewModel.clearChapterSelection()
                                    coroutineScope.launch {
                                        workspaceNavState.back()
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
