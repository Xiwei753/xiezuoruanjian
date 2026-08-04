package com.xiwei.sujian.ui.phone.portrait

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffold
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.ui.compose.SujianSessionAppState
import com.xiwei.sujian.ui.compose.editor.SujianEditorHost
import com.xiwei.sujian.ui.compose.workspace.ChapterTreeContent
import com.xiwei.sujian.ui.compose.workspace.ProjectListContent
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private enum class WorkspacePaneKey {
    ProjectList,
    ChapterTree,
    Editor,
}

private val WorkspacePaneKey.role: androidx.compose.material3.adaptive.layout.ThreePaneScaffoldRole
    get() = when (this) {
        WorkspacePaneKey.ProjectList -> ListDetailPaneScaffoldRole.List
        WorkspacePaneKey.ChapterTree -> ListDetailPaneScaffoldRole.Detail
        WorkspacePaneKey.Editor -> ListDetailPaneScaffoldRole.Extra
    }

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun PhoneWorkspaceHost(
    sessionViewModel: WorkspaceSessionViewModel,
    workspaceRepository: WorkspaceRepository,
    onOpenProject: (String) -> Unit,
    onOpenChapter: (String, String, String) -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val coroutineScope = rememberCoroutineScope()
    val sessionAppState = remember { SujianSessionAppState(sessionViewModel) }

    val initialHistory = remember {
        val chain = mutableListOf(
            ThreePaneScaffoldDestinationItem(WorkspacePaneKey.ProjectList.role, WorkspacePaneKey.ProjectList),
        )
        if (sessionViewModel.currentProjectId != null) {
            chain += ThreePaneScaffoldDestinationItem(WorkspacePaneKey.ChapterTree.role, WorkspacePaneKey.ChapterTree)
            if (sessionViewModel.currentChapterId != null && sessionViewModel.currentVolumeId != null) {
                chain += ThreePaneScaffoldDestinationItem(WorkspacePaneKey.Editor.role, WorkspacePaneKey.Editor)
            }
        }
        chain
    }
    val navigator = rememberListDetailPaneScaffoldNavigator<WorkspacePaneKey>(
        initialDestinationHistory = initialHistory,
    )

    suspend fun backOnePaneAndWriteBack() {
        if (navigator.canNavigateBack()) {
            navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
        }
        when (navigator.currentDestination?.contentKey) {
            WorkspacePaneKey.ProjectList -> {
                if (sessionViewModel.currentProjectId != null) {
                    sessionViewModel.clearProjectSelection()
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("project_list")
                }
            }
            WorkspacePaneKey.ChapterTree -> {
                if (sessionViewModel.currentChapterId != null) {
                    sessionViewModel.clearChapterSelection()
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("chapter_tree")
                }
            }
            null -> {
                if (sessionViewModel.currentProjectId != null) {
                    sessionViewModel.clearProjectSelection()
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("project_list")
                }
            }
            WorkspacePaneKey.Editor -> { }
        }
    }

    val navigateBackWithWriteBack: () -> Unit = {
        coroutineScope.launch {
            backOnePaneAndWriteBack()
        }
    }

    PredictiveBackHandler(enabled = navigator.canNavigateBack()) { progressEvents ->
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack(
            navigator.currentDestination?.contentKey?.name ?: "workspace",
            "start",
        )
        try {
            progressEvents.collect { event ->
                if (event.progress != 0f) {
                    navigator.seekBack(
                        BackNavigationBehavior.PopUntilScaffoldValueChange,
                        com.xiwei.sujian.ui.compose.navigation.predictiveBackStateFraction(event.progress),
                    )
                }
            }
            backOnePaneAndWriteBack()
        } catch (e: CancellationException) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("workspace", "cancel")
            withContext(NonCancellable) {
                navigator.seekBack(BackNavigationBehavior.PopUntilScaffoldValueChange, 0f)
            }
            throw e
        }
    }

    val currentProjectId = sessionViewModel.currentProjectId
    val currentVolumeId = sessionViewModel.currentVolumeId
    val currentChapterId = sessionViewModel.currentChapterId
    val currentChapterTitle = sessionViewModel.currentChapterTitle

    ListDetailPaneScaffold(
        modifier = modifier,
        directive = navigator.scaffoldDirective,
        scaffoldState = navigator.scaffoldState,
        listPane = {
            AnimatedPane {
                ProjectListContent(
                    appState = sessionAppState,
                    onSelectProject = { projectId, projectTitle ->
                        sessionViewModel.selectProject(projectId, projectTitle)
                        onOpenProject(projectId)
                        coroutineScope.launch {
                            navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, WorkspacePaneKey.ChapterTree)
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
                            onOpenChapter(currentProjectId, volumeId, chapterId)
                            coroutineScope.launch {
                                navigator.navigateTo(ListDetailPaneScaffoldRole.Extra, WorkspacePaneKey.Editor)
                            }
                        },
                        onBackToProjects = navigateBackWithWriteBack,
                    )
                }
            }
        },
        extraPane = {
            AnimatedPane {
                if (currentProjectId != null && currentChapterId != null && currentVolumeId != null) {
                    SujianEditorHost(
                        projectId = currentProjectId,
                        volumeId = currentVolumeId,
                        chapterId = currentChapterId,
                        chapterTitle = currentChapterTitle,
                        modifier = Modifier.fillMaxSize(),
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
        },
    )
}
