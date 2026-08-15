package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.presentation.screen.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.requestOpenChapter
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.editor.ui.SujianEditorHost
import com.xiwei.sujian.feature.project.data.ProjectRepository
import kotlinx.coroutines.launch

/**
 * 大屏写作工作台依赖 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class WideWorkspaceDeps(
    val appState: SujianAppState,
    val projectRepository: ProjectRepository,
    val projectWorkspaceActions: AndroidWorkspaceActionSpec,
    val chrome: SujianChromeSpec,
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
    val onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
)

/**
 * #628 验收点 4：大屏写作工作台结构尺寸 — 来自 Rust LayoutMetrics，
 * 打包传递避免函数参数超出门禁阈值。
 *
 * - [listPaneWidthDp]：左侧章节树宽度（null 时不画章节树，防御性）；
 * - [toolPaneWidthDp]：右侧工具面板宽度；
 * - [toolRailWidthDp]：最右工具栏图标列宽度。
 */
internal data class WideWorkspaceMetrics(
    val listPaneWidthDp: Float?,
    val toolPaneWidthDp: Float,
    val toolRailWidthDp: Float,
)

/**
 * 大屏写作工作台（#625 第二段 / 评论项2-5）— Row 布局：
 * - 左：[ChapterTreeContent]（宽度来自 [WideWorkspaceMetrics.listPaneWidthDp]，
 *   用户主动收起，不按设备尺寸自动收起）；
 * - 中：[SujianEditorHost]（复用现有编辑器，不创建第二个，收起不销毁/重建，始终在组合中）；
 * - 右：[WritingToolPane]（用户主动收起，宽度来自 [WideWorkspaceMetrics.toolPaneWidthDp]）
 *   + [WritingToolRail]（最右图标列，宽度来自 [WideWorkspaceMetrics.toolRailWidthDp]）。
 *
 * 顶部 [WritingWorkspaceToolbar]（三组独立容器工具栏）由本组件承担
 * （#628 验收点 6：Workbench Writing 由工作台自己拥有顶部工具栏，
 * 外层 app shell 不再额外画通用顶栏）。
 *
 * #625 评论项4：用户主动收起 — 不按设备尺寸/方向自动多档收 pane。
 * 收起状态用 rememberSaveable 持有，跨配置变化保留。收起只改变布局占用，
 * 不销毁/重建中央 [SujianEditorHost]（EditorPane 始终在组合中）。
 *
 * #625 评论项5：rail/pane 收成工具壳。当前无真实工具内容（星图/AI 归 #373/#506），
 * 工具列表为空，pane 显示空态，不放伪功能按钮。
 */
@Composable
internal fun WideWritingWorkspace(
    deps: WideWorkspaceDeps,
    documentState: WideWorkspaceDocumentState,
    editorViewModel: EditorViewModel,
    metrics: WideWorkspaceMetrics,
    callbacks: WideWorkspaceCallbacks,
    modifier: Modifier = Modifier,
) {
    // #625 评论项4：用户主动收起 — rememberSaveable 跨配置变化保留收起状态。
    var chapterTreeCollapsed by rememberSaveable { mutableStateOf(false) }
    var toolPaneCollapsed by rememberSaveable { mutableStateOf(false) }
    // #625 评论项5：当前选中工具 — rememberSaveable 持有。星图/AI 归 #373/#506，
    // 当前工具列表为空，selectedToolId 仅维护状态机，不放伪功能。
    var selectedToolId by rememberSaveable { mutableStateOf<String?>(null) }
    val coroutineScope = rememberCoroutineScope()

    // #625 评论项3：撤销/重做从 LocalEditorWindowHost 这条现有窗口链接入，
    // 继续走 View → Pipeline → session 编辑链，不重建 TextEditSessionBridge。
    val editorWindowHost = LocalEditorWindowHost.current
    val onUndo: () -> Unit = { editorWindowHost?.performUndo() }
    val onRedo: () -> Unit = { editorWindowHost?.performRedo() }

    // 同步状态 — 显示真实 SyncIndicatorState，不永远画固定 CloudSync。
    val appDeps = LocalSujianAppDependencies.current
    val syncState by appDeps.syncStatusRepository.state.collectAsState()

    // #625 评论项5：工具项列表 — 当前为空（星图/AI 归 #373/#506），不放伪功能按钮。
    val tools = remember { emptyList<WritingToolItem>() }
    // 当前选中工具的 content slot — 无真实工具内容时为 null，pane 显示空态。
    val toolPaneContent: (@Composable () -> Unit)? = null

    Column(modifier = modifier.fillMaxSize()) {
        WritingWorkspaceToolbar(
            chrome = deps.chrome,
            syncState = syncState,
            callbacks =
                WritingToolbarCallbacks(
                    onBack = callbacks.onBack,
                    onSync = callbacks.onSync,
                    onSearch = callbacks.onSearch,
                    onSettings = callbacks.onSettings,
                ),
            actions =
                WritingToolbarActions(
                    onUndo = onUndo,
                    onRedo = onRedo,
                    onToggleChapterTree = { chapterTreeCollapsed = !chapterTreeCollapsed },
                    chapterTreeCollapsed = chapterTreeCollapsed,
                ),
        )

        Row(modifier = Modifier.fillMaxSize()) {
            // 左：章节树（用户主动收起）。listPaneWidthDp 缺失时不画（防御性，isWideLayout 时应有值）。
            // 收起后不画 ChapterTreeContent，展开按钮仍在 toolbar 左组。
            if (!chapterTreeCollapsed && metrics.listPaneWidthDp != null) {
                ChapterTreeContent(
                    projectId = documentState.currentProjectId,
                    projectRepository = deps.projectRepository,
                    workspaceActions = deps.projectWorkspaceActions,
                    onSelectChapter = { volumeId, chapterId, chapterTitle ->
                        ChapterSelectContext(
                            coroutineScope = coroutineScope,
                            editorViewModel = editorViewModel,
                            appState = deps.appState,
                            projectId = documentState.currentProjectId,
                        ).handleChapterSelect(
                            volumeId = volumeId,
                            chapterId = chapterId,
                            chapterTitle = chapterTitle,
                        )
                    },
                    onError = deps.appState::reportWorkspaceError,
                    modifier =
                        Modifier
                            .fillMaxHeight()
                            .width(metrics.listPaneWidthDp.dp),
                )
            }

            // 中：正文编辑器（复用 SujianEditorHost，不创建第二个；收起不销毁/重建，始终在组合中）
            EditorPane(
                documentState = documentState,
                onChapterSwitchFailed = callbacks.onChapterSwitchFailed,
                modifier = Modifier.weight(1f).fillMaxHeight(),
            )

            // 右：工具面板（用户主动收起，宽度来自 metrics.toolPaneWidthDp）
            // + 工具栏图标列（宽度来自 metrics.toolRailWidthDp）。
            if (!toolPaneCollapsed) {
                WritingToolPane(
                    content = toolPaneContent,
                    modifier = Modifier.fillMaxHeight().width(metrics.toolPaneWidthDp.dp),
                )
            }
            WritingToolRail(
                tools = tools,
                selectedToolId = selectedToolId,
                onSelect = { id -> selectedToolId = id },
                onTogglePane = { toolPaneCollapsed = !toolPaneCollapsed },
                paneCollapsed = toolPaneCollapsed,
                modifier = Modifier.fillMaxHeight().width(metrics.toolRailWidthDp.dp),
            )
        }
    }
}

/**
 * 章节选择事务上下文 — 打包传递，避免函数参数超出门禁阈值。
 */
private data class ChapterSelectContext(
    val coroutineScope: kotlinx.coroutines.CoroutineScope,
    val editorViewModel: EditorViewModel,
    val appState: SujianAppState,
    val projectId: String,
)

/**
 * 章节选择事务 — 提取以降低 [WideWritingWorkspace] 认知复杂度。
 * 复用 ProjectWorkspaceScreen 的章节切换事务入口 — 事务成功后才提交业务选择。
 */
private fun ChapterSelectContext.handleChapterSelect(
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
) {
    coroutineScope.launch {
        val result =
            editorViewModel.requestOpenChapter(
                projectId,
                volumeId,
                chapterId,
                chapterTitle,
            )
        when (result) {
            is ChapterSwitchResult.Success -> {
                appState.selectChapter(volumeId, chapterId, chapterTitle)
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

/**
 * 正文编辑器面板 — 提取以降低 [WideWritingWorkspace] 认知复杂度。
 * 复用 [SujianEditorHost]，不创建第二个编辑器。
 */
@Composable
private fun EditorPane(
    documentState: WideWorkspaceDocumentState,
    modifier: Modifier = Modifier,
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
) {
    Box(
        modifier =
            modifier
                .background(MaterialTheme.colorScheme.background),
    ) {
        if (documentState.currentChapterId != null && documentState.currentVolumeId != null) {
            SujianEditorHost(
                projectId = documentState.currentProjectId,
                volumeId = documentState.currentVolumeId,
                chapterId = documentState.currentChapterId,
                chapterTitle = documentState.currentChapterTitle,
                modifier = Modifier.fillMaxSize(),
                onChapterSwitchFailed = onChapterSwitchFailed,
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
