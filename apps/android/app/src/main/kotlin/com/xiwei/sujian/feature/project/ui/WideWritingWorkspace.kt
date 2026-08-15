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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.presentation.screen.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.feature.editor.presentation.ChapterSwitchResult
import com.xiwei.sujian.feature.editor.presentation.EditorViewModel
import com.xiwei.sujian.feature.editor.presentation.requestOpenChapter
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
 * 大屏写作工作台（#625 第二段）— Row 布局：
 * - 左：[ChapterTreeContent]（宽度来自 [WideWorkspaceMetrics.listPaneWidthDp]，
 *   用户主动收起，不按设备尺寸自动收起）；
 * - 中：[SujianEditorHost]（复用现有编辑器，不创建第二个）；
 * - 右：[WritingToolPane]（用户主动收起，宽度来自 [WideWorkspaceMetrics.toolPaneWidthDp]）
 *   + [WritingToolRail]（最右图标列，宽度来自 [WideWorkspaceMetrics.toolRailWidthDp]）。
 *
 * 顶部 [WritingWorkspaceToolbar] 由本组件承担（#628 验收点 6：Workbench Writing
 * 由工作台自己拥有顶部工具栏，外层 app shell 不再额外画通用顶栏）。
 *
 * #625 评论：用户主动收起 — 不按设备尺寸/方向自动多档收 pane。
 * 收起状态用 rememberSaveable 持有，跨配置变化保留。
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
    // #625 评论：用户主动收起 — rememberSaveable 跨配置变化保留收起状态。
    var chapterTreeCollapsed by rememberSaveable { mutableStateOf(false) }
    var toolPaneCollapsed by rememberSaveable { mutableStateOf(false) }
    val coroutineScope = rememberCoroutineScope()

    Column(modifier = modifier.fillMaxSize()) {
        WritingWorkspaceToolbar(
            chapterTitle =
                documentState.currentChapterTitle.ifEmpty {
                    stringResource(id = R.string.writing_workspace_chapter_placeholder)
                },
            chrome = deps.chrome,
            callbacks =
                WritingToolbarCallbacks(
                    onBack = callbacks.onBack,
                    onSync = callbacks.onSync,
                    onSearch = callbacks.onSearch,
                    onSettings = callbacks.onSettings,
                ),
        )

        Row(modifier = Modifier.fillMaxSize()) {
            // 左：章节树（用户主动收起）。listPaneWidthDp 缺失时不画（防御性，isWideLayout 时应有值）。
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

            // 中：正文编辑器（复用 SujianEditorHost，不创建第二个）
            EditorPane(
                documentState = documentState,
                onChapterSwitchFailed = callbacks.onChapterSwitchFailed,
                modifier = Modifier.weight(1f).fillMaxHeight(),
            )

            // 右：工具面板（用户主动收起，宽度来自 metrics.toolPaneWidthDp）
            // + 工具栏图标列（宽度来自 metrics.toolRailWidthDp）。
            if (!toolPaneCollapsed) {
                WritingToolPane(
                    modifier = Modifier.fillMaxHeight().width(metrics.toolPaneWidthDp.dp),
                )
            }
            WritingToolRail(
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
