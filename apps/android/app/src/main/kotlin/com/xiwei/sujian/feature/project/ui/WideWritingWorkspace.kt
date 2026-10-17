package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.navigation.SujianWorkbenchDimensions
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.editor.ui.SujianEditorHost
import com.xiwei.sujian.feature.project.data.ProjectRepository

/**
 * 大屏写作工作台依赖 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WideWorkspaceDeps(
    val appState: SujianAppState,
    val projectRepository: ProjectRepository,
    val projectWorkspaceActions: WorkspaceActions,
    val chrome: SujianChromeSpec?,
)

/**
 * 大屏写作工作台当前文档状态 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WideWorkspaceDocumentState(
    val currentProjectId: String,
    val currentVolumeId: String?,
    val currentChapterId: String?,
    val currentChapterTitle: String,
)

/**
 * 大屏写作工作台回调 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WideWorkspaceCallbacks(
    val onBack: () -> Unit,
    val onSync: () -> Unit,
    val onSearch: () -> Unit,
    val onSettings: () -> Unit,
    val onChapterSwitch: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    val onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
)

/**
 * 工作台布局输入 — pane 收起状态仍是 Android 局部 UI 状态；
 * 窗口尺寸→宽/窄布局决策由顶层导航按本端窗口尺寸完成，不在此处判断。
 */
internal data class WideWorkspaceLayoutState(
    val chapterTreeCollapsed: Boolean,
    val toolPaneCollapsed: Boolean,
    val onToggleChapterTree: () -> Unit,
    val onToggleToolPane: () -> Unit,
)

/**
 * #641 评论 问题5：大屏写作工作台 — Compose-native 布局。
 *
 * 布局完全由本端 Compose 原语决定（不再消费 Core 返回的 bounds / 角色放置）：
 * - 顶栏：一行左（返回/撤销/重做/收起章节栏）中（正文工具 slot）右（同步/搜索/设置）；
 * - 内容：一行 左章节树 + 中央编辑器 + 右工具面板 + 最右工具 rail；
 * - 章节树 / 工具面板由用户主动收起（[WideWorkspaceLayoutState]），不按设备尺寸自动收；
 * - 中央 [SujianEditorHost] 始终在同一 call site，切换 pane 可见性不重建编辑器。
 */
@Composable
internal fun WideWritingWorkspace(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    layoutState: WideWorkspaceLayoutState,
    callbacks: WideWorkspaceCallbacks,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxSize()) {
        WideWorkspaceToolbar(
            chrome = deps.chrome,
            chapterTreeCollapsed = layoutState.chapterTreeCollapsed,
            layoutState = layoutState,
            callbacks = callbacks,
        )
        WideWorkspaceContentRow(
            deps = deps,
            documentState = documentState,
            layoutState = layoutState,
            callbacks = callbacks,
            modifier = Modifier.weight(1f),
        )
    }
}

/** 顶栏三组容器 — 左 / 中 / 右按各自宽度放置。 */
@Composable
private fun WideWorkspaceToolbar(
    chrome: SujianChromeSpec?,
    chapterTreeCollapsed: Boolean,
    layoutState: WideWorkspaceLayoutState,
    callbacks: WideWorkspaceCallbacks,
) {
    val editorWindowHost = LocalEditorWindowHost.current
    val appDeps = LocalSujianAppDependencies.current
    val syncState by appDeps.syncStatusRepository.state.collectAsState()
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .height(SujianWorkbenchDimensions.toolbarHeightDp.dp),
    ) {
        WritingToolbarLeadingGroup(
            showBack = chrome?.showBack ?: false,
            chapterTreeCollapsed = chapterTreeCollapsed,
            callbacks =
                WritingToolbarLeadingCallbacks(
                    onBack = callbacks.onBack,
                    onUndo = { editorWindowHost?.performUndo() },
                    onRedo = { editorWindowHost?.performRedo() },
                    onToggleChapterTree = layoutState.onToggleChapterTree,
                ),
            modifier =
                Modifier
                    .width(SujianWorkbenchDimensions.toolbarLeadingWidthDp.dp)
                    .fillMaxHeight(),
        )
        WritingToolbarCenterSlot(
            modifier =
                Modifier
                    .weight(1f)
                    .fillMaxHeight(),
        )
        WritingToolbarTrailingGroup(
            actions = chrome?.actions ?: emptyList(),
            syncState = syncState,
            callbacks =
                WritingToolbarTrailingCallbacks(
                    onSync = callbacks.onSync,
                    onSearch = callbacks.onSearch,
                    onSettings = callbacks.onSettings,
                ),
            modifier =
                Modifier
                    .width(SujianWorkbenchDimensions.toolbarTrailingWidthDp.dp)
                    .fillMaxHeight(),
        )
    }
}

/** 内容行：章节导航（可收起） + 中央编辑器 + 工具面板（可收起） + 工具 rail。 */
@Composable
private fun WideWorkspaceContentRow(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    layoutState: WideWorkspaceLayoutState,
    callbacks: WideWorkspaceCallbacks,
    modifier: Modifier = Modifier,
) {
    val chapterTreeCollapsed = layoutState.chapterTreeCollapsed
    val toolPaneCollapsed = layoutState.toolPaneCollapsed
    // #625 评论项5：当前选中工具 — rememberSaveable 持有。星图/AI 归 #373/#506。
    var selectedToolId by rememberSaveable { mutableStateOf<String?>(null) }
    val tools = remember { emptyList<WritingToolItem>() }
    val toolPaneContent: (@Composable () -> Unit)? = null

    Row(modifier = modifier.fillMaxWidth()) {
        // 章节导航 — 用户主动收起时不画。
        if (!chapterTreeCollapsed) {
            ChapterTreeContent(
                projectId = documentState.currentProjectId,
                projectRepository = deps.projectRepository,
                workspaceActions = deps.projectWorkspaceActions,
                onSelectChapter = callbacks.onChapterSwitch,
                onError = deps.appState::reportWorkspaceError,
                modifier =
                    Modifier
                        .width(SujianWorkbenchDimensions.listPaneWidthDp.dp)
                        .fillMaxHeight(),
            )
        }
        // 中央编辑器 — 始终在同一 call site，切换 pane 可见性不重建 AndroidView。
        SujianEditorHost(
            projectId = documentState.currentProjectId,
            volumeId = documentState.currentVolumeId ?: "",
            chapterId = documentState.currentChapterId ?: "",
            chapterTitle = documentState.currentChapterTitle,
            modifier =
                Modifier
                    .weight(1f)
                    .fillMaxHeight(),
            onChapterSwitchFailed = callbacks.onChapterSwitchFailed,
        )
        // 工具面板（用户主动收起时不画）。
        if (!toolPaneCollapsed) {
            WritingToolPane(
                content = toolPaneContent,
                modifier =
                    Modifier
                        .width(SujianWorkbenchDimensions.toolPaneWidthDp.dp)
                        .fillMaxHeight(),
            )
        }
        // 工具栏图标列（始终画）。
        WritingToolRail(
            tools = tools,
            selectedToolId = selectedToolId,
            onSelect = { id -> selectedToolId = id },
            onTogglePane = layoutState.onToggleToolPane,
            paneCollapsed = toolPaneCollapsed,
            modifier =
                Modifier
                    .width(SujianWorkbenchDimensions.toolRailWidthDp.dp)
                    .fillMaxHeight(),
        )
    }
}
