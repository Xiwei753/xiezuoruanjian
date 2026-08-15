package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.app.WorkspaceAppState
import com.xiwei.sujian.app.presentation.layout.WorkspaceLayoutMode
import com.xiwei.sujian.app.presentation.screen.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.WorkspaceActionKind
import com.xiwei.sujian.app.presentation.screen.WorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.WorkspaceActionTarget
import com.xiwei.sujian.core.designsystem.component.SujianCard
import com.xiwei.sujian.core.designsystem.component.SujianDialog
import com.xiwei.sujian.core.designsystem.component.SujianFab
import com.xiwei.sujian.core.designsystem.component.SujianOverflowMenu
import com.xiwei.sujian.core.designsystem.component.SujianOverflowMenuItem
import com.xiwei.sujian.core.designsystem.component.SujianTextField
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.core.designsystem.theme.SujianDimensions
import com.xiwei.sujian.feature.project.data.model.ProjectSummary
import androidx.compose.foundation.lazy.grid.items as gridItems

/**
 * #625 项7：作品卡片字数格式化 — 纯函数，接收 i18n 格式串，便于单测。
 *
 * - < 10000 字：用 [wordsFormat] 格式化原数（中文 "%1$d 字" / 英文 "%1$d words"）；
 * - >= 10000 字：用 [wanFormat] 格式化（中文 "%2$.1f 万字" / 英文 "%1$d words"，英文不区分万字）。
 *
 * 两个格式串都接收 (wordCount, wanValue) 两个参数，由调用方从 `stringResource` 取出，
 * 这样 UI 不自己拼中文单位，Core 只返回整数 `totalWordCount`。
 */
internal fun formatProjectWordCount(
    wordCount: Int,
    wordsFormat: String,
    wanFormat: String,
): String {
    val wanValue = wordCount / 10_000.0
    return if (wordCount < 10_000) {
        String.format(wordsFormat, wordCount, wanValue)
    } else {
        String.format(wanFormat, wordCount, wanValue)
    }
}

/**
 * #625 项7：作品卡片字数行 — 用 `stringResource` 取 i18n 格式串，再交 [formatProjectWordCount] 格式化。
 * null 或 0 不显示。
 */
@Composable
private fun ProjectCardWordCount(totalWordCount: Int?) {
    if (totalWordCount != null && totalWordCount > 0) {
        val formatted =
            formatProjectWordCount(
                totalWordCount,
                wordsFormat = stringResource(id = R.string.project_word_count_words),
                wanFormat = stringResource(id = R.string.project_word_count_wan),
            )
        Text(
            stringResource(id = R.string.project_word_count_with_dot, formatted),
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
internal fun ProjectListContent(
    appState: WorkspaceAppState,
    workspaceActions: AndroidWorkspaceActionSpec,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    modifier: Modifier = Modifier,
    /**
     * #625 第二段 / #628 验收点 1：宽屏 grid 切换依据 — 来自 Rust `LayoutContractDto.workspaceLayoutMode`。
     *
     * #628 原则：不自己判断窗口尺寸 — 通过 layoutSpec.contract.workspaceLayoutMode 决定。
     * SinglePane → LazyColumn（单列）；Workbench → LazyVerticalGrid（多列）。
     * 默认 SinglePane（与窄窗口基线一致）。
     */
    workspaceLayoutMode: WorkspaceLayoutMode = WorkspaceLayoutMode.SINGLE_PANE,
    /**
     * #628 验收点 4：作品卡片最小宽度 — 来自 Rust `LayoutMetricsDto.projectCardMinWidthDp`。
     *
     * 仅在 [workspaceLayoutMode] 为 WORKBENCH 时使用（画 grid）。
     * SinglePane 不画 grid，传 0f 即可（调用方约定）。
     */
    projectCardMinWidthDp: Float = 0f,
) {
    var showCreateDialog by remember { mutableStateOf(false) }
    // #625：菜单展开状态留在每张卡片本地（见 [ProjectCard]），
    // 页面级只持有"重命名输入"与"确认删除"两个真正需要居中 Dialog 的目标。
    // #625 项6：列表 UI 唯一数据源是 ProjectSummary（含 title/字数/卷数/章节数），
    // 不再保留 Project + ProjectSummary 双源拼接。
    var renameProject by remember { mutableStateOf<ProjectSummary?>(null) }
    var confirmDeleteProject by remember { mutableStateOf<ProjectSummary?>(null) }
    val dims = LocalSujianDimensions.current

    // #610 评论四：动作存在性与位置来自 Core 契约，不再由 Composable 自己决定：
    // - 新建作品是 PrimaryAction（Android compact 画成 FAB）；
    // - 作品菜单只按 Context(Project) spec 渲染，顺序来自 Core order。
    val createProjectAction =
        workspaceActions.primaryActions.firstOrNull { it.kind == WorkspaceActionKind.CreateProject }
    val projectMenuActions = workspaceActions.contextActions(WorkspaceActionTarget.Project)

    // #625 第二段：作品摘要查找表 — 由 id 索引，避免每张卡片 O(n) 查找。
    val summaryById =
        remember(appState.projectSummaries) {
            appState.projectSummaries.associateBy { it.id }
        }

    // #628 原则：宽屏 grid 切换依据是 workspaceLayoutMode，不自己判断宽度。
    val useWideGrid = workspaceLayoutMode != WorkspaceLayoutMode.SINGLE_PANE

    Box(modifier = modifier.fillMaxSize()) {
        if (appState.projectSummaries.isEmpty()) {
            // #614：首次加载失败显示错误态（loadError 文本 + 同样 hint）；否则显示原"暂无作品"空状态。
            ProjectListEmptyState(
                loadError = appState.loadError,
                dims = dims,
                modifier = Modifier.fillMaxSize(),
            )
        } else if (useWideGrid) {
            // #625 项6 / #628 验收点 4：宽屏 grid — LazyVerticalGrid（多列），数据源 ProjectSummary。
            // recentEdits 区块保持单列横跨（用 header item + full-span items），
            // projects 区块用 grid。当前简化：宽屏直接全部用 grid（recentEdits 较少）。
            // 卡片最小宽度来自 Rust LayoutMetrics.projectCardMinWidthDp。
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = projectCardMinWidthDp.dp),
                contentPadding = PaddingValues(horizontal = dims.space16, vertical = dims.space8),
                horizontalArrangement = Arrangement.spacedBy(dims.space8),
                verticalArrangement = Arrangement.spacedBy(dims.space8),
                modifier = Modifier.fillMaxSize(),
            ) {
                gridItems(appState.projectSummaries, key = { it.id }) { summary ->
                    ProjectCard(
                        summary = summary,
                        onSelect = { onSelectProject(summary.id, summary.title) },
                        menuActions =
                            ProjectCardMenuActions(
                                menuActions = projectMenuActions,
                                onRename = { renameProject = summary },
                                onDelete = { action ->
                                    handleProjectDeleteAction(
                                        action,
                                        summary,
                                        appState,
                                    ) { confirmDeleteProject = summary }
                                },
                            ),
                        modifier = Modifier.testTag(SujianSemanticIds.project(summary.id)),
                    )
                }
            }
        } else {
            // 窄屏：单列 LazyColumn（保留 recentEdits;分区逻辑）。
            LazyColumn(
                contentPadding = PaddingValues(horizontal = dims.space16, vertical = dims.space8),
                modifier = Modifier.fillMaxSize(),
            ) {
                if (appState.recentEdits.isNotEmpty()) {
                    item {
                        Text(
                            stringResource(id = R.string.recent_edits),
                            style = MaterialTheme.typography.titleSmall,
                            modifier = Modifier.padding(bottom = dims.space8),
                        )
                    }
                    items(appState.recentEdits) { edit ->
                        // #625 项6：recentEdits 标题也来自 ProjectSummary 单数据源。
                        val summary = appState.projectSummaries.find { it.id == edit.projectId }
                        SujianCard(
                            onClick = { onSelectProject(edit.projectId, summary?.title ?: "") },
                            modifier = Modifier.fillMaxWidth().padding(bottom = dims.space8),
                        ) {
                            Column(modifier = Modifier.padding(dims.space16)) {
                                Text(
                                    summary?.title ?: stringResource(id = R.string.unknown_project),
                                    style = MaterialTheme.typography.titleMedium,
                                )
                                Text(
                                    stringResource(id = R.string.continue_writing_action),
                                    style = MaterialTheme.typography.bodySmall,
                                )
                            }
                        }
                    }
                    item {
                        Spacer(modifier = Modifier.height(dims.space16))
                        Text(
                            stringResource(id = R.string.all_projects),
                            style = MaterialTheme.typography.titleSmall,
                            modifier = Modifier.padding(bottom = dims.space8),
                        )
                    }
                }
                items(appState.projectSummaries, key = { it.id }) { summary ->
                    ProjectCard(
                        summary = summary,
                        onSelect = { onSelectProject(summary.id, summary.title) },
                        menuActions =
                            ProjectCardMenuActions(
                                menuActions = projectMenuActions,
                                onRename = { renameProject = summary },
                                onDelete = { action ->
                                    // #610 评论五：Delete 是否需要确认由 Core 契约 requiresConfirmation 决定，
                                    // 不再写死进入确认弹窗。
                                    handleProjectDeleteAction(
                                        action,
                                        summary,
                                        appState,
                                    ) { confirmDeleteProject = summary }
                                },
                            ),
                        modifier = Modifier.testTag(SujianSemanticIds.project(summary.id)),
                    )
                }
            }
        }

        // #610 评论四：Core PrimaryAction → Android compact 呈现为 FAB。
        // 契约里有该槽位才画；契约没有（或桥失败）则不画。
        if (createProjectAction != null) {
            SujianFab(
                onClick = { showCreateDialog = true },
                icon = SujianIcons.Add,
                contentDescription = stringResource(id = R.string.action_new_project),
                modifier = Modifier.align(Alignment.BottomEnd).padding(dims.space16),
            )
        }
    }

    if (showCreateDialog) {
        var title by remember { mutableStateOf("") }
        SujianDialog(
            onDismissRequest = { showCreateDialog = false },
            title = stringResource(id = R.string.dialog_new_project_title),
            confirmText = stringResource(id = R.string.action_create),
            onConfirm = {
                if (title.isNotBlank()) {
                    appState.createProject(title.trim())
                    showCreateDialog = false
                }
            },
            dismissText = stringResource(id = R.string.action_cancel),
            onDismiss = { showCreateDialog = false },
            body = {
                SujianTextField(
                    value = title,
                    onValueChange = { title = it },
                    label = { Text(stringResource(id = R.string.hint_project_title_new)) },
                    singleLine = true,
                )
            },
        )
    }

    // #625：重命名输入 Dialog — 菜单项点击后由卡片回调 onRename 把目标抬到页面级，
    // 输入完成后写回 Core 并清空目标。Dialog 主体提取为 [RenameProjectDialog] 以控制认知复杂度。
    renameProject?.let { summary ->
        RenameProjectDialog(
            summary = summary,
            onRename = { newTitle -> appState.renameProject(summary.id, newTitle) },
            onDismiss = { renameProject = null },
        )
    }

    confirmDeleteProject?.let { summary ->
        ConfirmDeleteProjectDialog(
            name = summary.title,
            onConfirm = {
                appState.deleteProject(summary.id)
                confirmDeleteProject = null
            },
            onDismiss = { confirmDeleteProject = null },
        )
    }
}

/**
 * #614：作品列表空状态。loadError 非空时显示错误文本，否则显示"暂无作品"。
 * 提取为独立 Composable 以控制 [ProjectListContent] 的认知复杂度。
 */
@Composable
private fun ProjectListEmptyState(
    loadError: String?,
    dims: SujianDimensions,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.padding(dims.space32),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (loadError != null) {
            Text(loadError, style = MaterialTheme.typography.titleMedium)
        } else {
            Text(stringResource(id = R.string.project_list_empty), style = MaterialTheme.typography.titleMedium)
        }
        Spacer(modifier = Modifier.height(dims.space8))
        Text(stringResource(id = R.string.project_list_empty_hint), style = MaterialTheme.typography.bodyMedium)
    }
}

/**
 * 作品卡片菜单回调 — 打包传递，避免函数参数超出门禁阈值。
 */
internal data class ProjectCardMenuActions(
    val menuActions: List<WorkspaceActionSpec>,
    val onRename: () -> Unit,
    val onDelete: (WorkspaceActionSpec) -> Unit,
)

@Composable
private fun ProjectCard(
    summary: ProjectSummary,
    onSelect: () -> Unit,
    menuActions: ProjectCardMenuActions,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    // #625：菜单展开状态留在卡片本地 — 多张卡片各自独立，不会互相覆盖。
    var menuExpanded by remember { mutableStateOf(false) }
    SujianCard(
        onClick = onSelect,
        modifier = modifier.fillMaxWidth().padding(bottom = dims.space8),
    ) {
        Row(
            modifier = Modifier.padding(dims.space16).fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(summary.title, style = MaterialTheme.typography.titleMedium)
                Row(
                    horizontalArrangement = Arrangement.spacedBy(dims.space8),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(summary.updatedAt.substringBefore("T"), style = MaterialTheme.typography.bodySmall)
                    // #625 项7：作品卡片字数 — i18n 由 [ProjectCardWordCount] 内部用 stringResource 格式化。
                    ProjectCardWordCount(summary.totalWordCount)
                }
            }
            // #610 评论四 + #625：菜单触发器与 DropdownMenu 在同一组合位置一起画，
            // 菜单锚定到 ⋮ 按钮左下角；菜单项按 Core Context(Project) spec 渲染，顺序来自 Core order。
            if (menuActions.menuActions.isNotEmpty()) {
                SujianOverflowMenu(
                    expanded = menuExpanded,
                    onExpandedChange = { menuExpanded = it },
                    contentDescription = stringResource(id = R.string.action_more),
                ) {
                    menuActions.menuActions.forEach { action ->
                        when (action.kind) {
                            WorkspaceActionKind.Rename ->
                                SujianOverflowMenuItem(
                                    text = stringResource(id = R.string.action_rename),
                                    onClick = {
                                        menuExpanded = false
                                        menuActions.onRename()
                                    },
                                )
                            WorkspaceActionKind.Delete ->
                                SujianOverflowMenuItem(
                                    text = stringResource(id = R.string.action_delete),
                                    onClick = {
                                        menuExpanded = false
                                        menuActions.onDelete(action)
                                    },
                                )
                            else -> {
                                // 其它角色不属于作品 Context 契约，不渲染。
                            }
                        }
                    }
                }
            }
        }
    }
}

/**
 * #625：作品重命名输入 Dialog。从 [ProjectListContent] 提取为独立 Composable，
 * 与 [ConfirmDeleteProjectDialog] 同模式，以控制 [ProjectListContent] 的认知复杂度。
 */
@Composable
private fun RenameProjectDialog(
    summary: ProjectSummary,
    onRename: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var newTitle by remember { mutableStateOf(summary.title) }
    SujianDialog(
        onDismissRequest = onDismiss,
        title = stringResource(id = R.string.action_rename),
        confirmText = stringResource(id = R.string.action_ok),
        onConfirm = {
            if (newTitle.isNotBlank() && newTitle != summary.title) {
                onRename(newTitle.trim())
            }
            onDismiss()
        },
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = onDismiss,
        body = {
            SujianTextField(
                value = newTitle,
                onValueChange = { newTitle = it },
                label = { Text(stringResource(id = R.string.hint_new_title)) },
                singleLine = true,
            )
        },
    )
}

@Composable
private fun ConfirmDeleteProjectDialog(
    name: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    SujianDialog(
        onDismissRequest = onDismiss,
        title = stringResource(id = R.string.confirm_delete),
        confirmText = stringResource(id = R.string.action_delete),
        onConfirm = onConfirm,
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = onDismiss,
        dangerous = true,
        body = {
            Text(stringResource(R.string.confirm_delete_message, name))
        },
    )
}

/**
 * #610 评论五：Delete 是否需要确认由 Core 契约 [WorkspaceActionSpec.requiresConfirmation] 决定。
 * 提取为顶层函数以控制 [ProjectListContent] 的认知复杂度。
 */
private fun handleProjectDeleteAction(
    action: WorkspaceActionSpec,
    summary: ProjectSummary,
    appState: WorkspaceAppState,
    onRequestConfirmDelete: () -> Unit,
) {
    if (action.requiresConfirmation) {
        onRequestConfirmDelete()
    } else {
        appState.deleteProject(summary.id)
    }
}
