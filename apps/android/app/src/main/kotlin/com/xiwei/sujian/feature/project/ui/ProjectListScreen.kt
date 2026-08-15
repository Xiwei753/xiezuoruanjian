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
import com.xiwei.sujian.app.presentation.layout.WorkspacePaneMode
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
import com.xiwei.sujian.feature.project.data.model.Project
import androidx.compose.foundation.lazy.grid.items as gridItems

/**
 * #625 第二段：作品卡片字数格式化。
 *
 * - < 10000 字：原数（如 "1234 字"）；
 * - >= 10000 字：x.x 万字（如 "1.2 万字"）。
 *
 * 提取为顶层函数以控制 [ProjectCard] 认知复杂度，便于单测。
 */
internal fun formatProjectWordCount(wordCount: Int): String =
    if (wordCount < 10_000) {
        "$wordCount 字"
    } else {
        val wan = wordCount / 10_000.0
        String.format("%.1f 万字", wan)
    }

/**
 * #625 第二段 4c：作品卡片字数行 — 提取以降低 [ProjectCard] 认知复杂度。
 * null 或 0 不显示。
 */
@Composable
private fun ProjectCardWordCount(totalWordCount: Int?) {
    if (totalWordCount != null && totalWordCount > 0) {
        Text(
            "· ${formatProjectWordCount(totalWordCount)}",
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
     * #625 第二段：宽屏 grid 切换依据 — 来自 Rust `LayoutContractDto.workspacePaneMode`。
     *
     * #628 原则：不自己判断窗口尺寸 — 通过 layoutSpec.contract.workspacePaneMode 决定。
     * SinglePane → LazyColumn（单列）；ListDetail / ThreePane → LazyVerticalGrid（多列）。
     * 默认 SinglePane（与窄窗口基线一致）。
     */
    workspacePaneMode: WorkspacePaneMode = WorkspacePaneMode.SINGLE_PANE,
) {
    var showCreateDialog by remember { mutableStateOf(false) }
    // #625：菜单展开状态留在每张卡片本地（见 [ProjectCard]），
    // 页面级只持有"重命名输入"与"确认删除"两个真正需要居中 Dialog 的目标。
    var renameProject by remember { mutableStateOf<Project?>(null) }
    var confirmDeleteProject by remember { mutableStateOf<Project?>(null) }
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

    // #628 原则：宽屏 grid 切换依据是 workspacePaneMode，不自己判断宽度。
    val useWideGrid = workspacePaneMode != WorkspacePaneMode.SINGLE_PANE

    Box(modifier = modifier.fillMaxSize()) {
        if (appState.projects.isEmpty()) {
            // #614：首次加载失败显示错误态（loadError 文本 + 同样 hint）；否则显示原"暂无作品"空状态。
            ProjectListEmptyState(
                loadError = appState.loadError,
                dims = dims,
                modifier = Modifier.fillMaxSize(),
            )
        } else if (useWideGrid) {
            // #625 第二段 4d：宽屏 grid — LazyVerticalGrid（多列）。
            // recentEdits 区块保持单列横跨（用 header item + full-span items），
            // projects 区块用 grid。当前简化：宽屏直接全部用 grid（recentEdits 较少）。
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 180.dp),
                contentPadding = PaddingValues(horizontal = dims.space16, vertical = dims.space8),
                horizontalArrangement = Arrangement.spacedBy(dims.space8),
                verticalArrangement = Arrangement.spacedBy(dims.space8),
                modifier = Modifier.fillMaxSize(),
            ) {
                gridItems(appState.projects, key = { it.id }) { project ->
                    val summary = summaryById[project.id]
                    ProjectCard(
                        project = project,
                        totalWordCount = summary?.totalWordCount,
                        onSelect = { onSelectProject(project.id, project.title) },
                        menuActions =
                            ProjectCardMenuActions(
                                menuActions = projectMenuActions,
                                onRename = { renameProject = project },
                                onDelete = { action ->
                                    handleProjectDeleteAction(
                                        action,
                                        project,
                                        appState,
                                    ) { confirmDeleteProject = project }
                                },
                            ),
                        modifier = Modifier.testTag(SujianSemanticIds.project(project.id)),
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
                        val project = appState.projects.find { it.id == edit.projectId }
                        SujianCard(
                            onClick = { onSelectProject(edit.projectId, project?.title ?: "") },
                            modifier = Modifier.fillMaxWidth().padding(bottom = dims.space8),
                        ) {
                            Column(modifier = Modifier.padding(dims.space16)) {
                                Text(
                                    project?.title ?: stringResource(id = R.string.unknown_project),
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
                items(appState.projects) { project ->
                    val summary = summaryById[project.id]
                    ProjectCard(
                        project = project,
                        totalWordCount = summary?.totalWordCount,
                        onSelect = { onSelectProject(project.id, project.title) },
                        menuActions =
                            ProjectCardMenuActions(
                                menuActions = projectMenuActions,
                                onRename = { renameProject = project },
                                onDelete = { action ->
                                    // #610 评论五：Delete 是否需要确认由 Core 契约 requiresConfirmation 决定，
                                    // 不再写死进入确认弹窗。
                                    handleProjectDeleteAction(
                                        action,
                                        project,
                                        appState,
                                    ) { confirmDeleteProject = project }
                                },
                            ),
                        modifier = Modifier.testTag(SujianSemanticIds.project(project.id)),
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
    renameProject?.let { project ->
        RenameProjectDialog(
            project = project,
            onRename = { newTitle -> appState.renameProject(project.id, newTitle) },
            onDismiss = { renameProject = null },
        )
    }

    confirmDeleteProject?.let { project ->
        ConfirmDeleteProjectDialog(
            name = project.title,
            onConfirm = {
                appState.deleteProject(project.id)
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
    project: Project,
    onSelect: () -> Unit,
    menuActions: ProjectCardMenuActions,
    modifier: Modifier = Modifier,
    /**
     * #625 第二段：作品字数 — 来自 [com.xiwei.sujian.app.WorkspaceAppState.projectSummaries]。
     * null 表示摘要未加载或该作品无摘要，不显示字数行。
     */
    totalWordCount: Int? = null,
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
                Text(project.title, style = MaterialTheme.typography.titleMedium)
                Row(
                    horizontalArrangement = Arrangement.spacedBy(dims.space8),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(project.updatedAt.substringBefore("T"), style = MaterialTheme.typography.bodySmall)
                    // #625 第二段 4c：作品卡片字数 — < 10000 原数，>= 10000 "x.x 万字"。
                    ProjectCardWordCount(totalWordCount)
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
    project: Project,
    onRename: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var newTitle by remember { mutableStateOf(project.title) }
    SujianDialog(
        onDismissRequest = onDismiss,
        title = stringResource(id = R.string.action_rename),
        confirmText = stringResource(id = R.string.action_ok),
        onConfirm = {
            if (newTitle.isNotBlank() && newTitle != project.title) {
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
    project: Project,
    appState: WorkspaceAppState,
    onRequestConfirmDelete: () -> Unit,
) {
    if (action.requiresConfirmation) {
        onRequestConfirmDelete()
    } else {
        appState.deleteProject(project.id)
    }
}
