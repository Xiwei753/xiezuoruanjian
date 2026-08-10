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
import com.xiwei.sujian.R
import com.xiwei.sujian.app.WorkspaceAppState
import com.xiwei.sujian.app.presentation.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.WorkspaceActionKind
import com.xiwei.sujian.app.presentation.WorkspaceActionSpec
import com.xiwei.sujian.app.presentation.WorkspaceActionTarget
import com.xiwei.sujian.core.designsystem.component.SujianCard
import com.xiwei.sujian.core.designsystem.component.SujianDialog
import com.xiwei.sujian.core.designsystem.component.SujianFab
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.component.SujianListItem
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.feature.editor.ui.AnimatedTextField
import com.xiwei.sujian.feature.project.data.model.Project

@Composable
internal fun ProjectListContent(
    appState: WorkspaceAppState,
    workspaceActions: AndroidWorkspaceActionSpec,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    var showCreateDialog by remember { mutableStateOf(false) }
    var showMenuForProject by remember { mutableStateOf<Project?>(null) }
    var confirmDeleteProject by remember { mutableStateOf<Project?>(null) }
    val dims = LocalSujianDimensions.current

    // #610 评论四：动作存在性与位置来自 Core 契约，不再由 Composable 自己决定：
    // - 新建作品是 PrimaryAction（Android compact 画成 FAB）；
    // - 作品菜单只按 Context(Project) spec 渲染，顺序来自 Core order。
    val createProjectAction =
        workspaceActions.primaryActions.firstOrNull { it.kind == WorkspaceActionKind.CreateProject }
    val projectMenuActions = workspaceActions.contextActions(WorkspaceActionTarget.Project)

    Box(modifier = modifier.fillMaxSize()) {
        if (appState.projects.isEmpty()) {
            Column(
                modifier = Modifier.fillMaxSize().padding(dims.space32),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(stringResource(id = R.string.project_list_empty), style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(dims.space8))
                Text(stringResource(id = R.string.project_list_empty_hint), style = MaterialTheme.typography.bodyMedium)
            }
        } else {
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
                    ProjectCard(
                        project = project,
                        onSelect = { onSelectProject(project.id, project.title) },
                        hasMenuActions = projectMenuActions.isNotEmpty(),
                        onMoreActions = { showMenuForProject = project },
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
                AnimatedTextField(
                    targetId = "project-title:new",
                    value = title,
                    onValueChange = { title = it },
                    onCommit = { },
                    label = { Text(stringResource(id = R.string.hint_project_title_new)) },
                    singleLine = true,
                )
            },
        )
    }

    showMenuForProject?.let { project ->
        ProjectMenuDialog(
            project = project,
            actions = projectMenuActions,
            onRename = { newTitle ->
                appState.renameProject(project.id, newTitle)
                showMenuForProject = null
            },
            // #610 评论五：Delete 是否需要确认由 Core 契约 requiresConfirmation 决定，
            // 不再写死进入确认弹窗。
            onDelete = { action ->
                handleProjectDeleteAction(action, project, appState) { confirmDeleteProject = project }
                showMenuForProject = null
            },
            onDismiss = { showMenuForProject = null },
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

@Composable
private fun ProjectCard(
    project: Project,
    onSelect: () -> Unit,
    hasMenuActions: Boolean,
    onMoreActions: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
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
                Text(project.updatedAt.substringBefore("T"), style = MaterialTheme.typography.bodySmall)
            }
            // #610 评论四：菜单按钮只按 Core Context(Project) 契约存在与否渲染。
            if (hasMenuActions) {
                SujianIconButton(
                    onClick = onMoreActions,
                    icon = SujianIcons.MoreVert,
                    contentDescription = stringResource(id = R.string.action_more),
                )
            }
        }
    }
}

@Composable
private fun ProjectMenuDialog(
    project: Project,
    actions: List<WorkspaceActionSpec>,
    onRename: (String) -> Unit,
    onDelete: (WorkspaceActionSpec) -> Unit,
    onDismiss: () -> Unit,
) {
    var showRename by remember { mutableStateOf(false) }

    if (showRename) {
        var newTitle by remember { mutableStateOf(project.title) }
        SujianDialog(
            onDismissRequest = { showRename = false },
            title = stringResource(id = R.string.action_rename),
            confirmText = stringResource(id = R.string.action_ok),
            onConfirm = {
                if (newTitle.isNotBlank() && newTitle != project.title) {
                    onRename(newTitle.trim())
                }
                showRename = false
            },
            dismissText = stringResource(id = R.string.action_cancel),
            onDismiss = { showRename = false },
            body = {
                AnimatedTextField(
                    targetId = "project-title:rename:${project.id}",
                    value = newTitle,
                    onValueChange = { newTitle = it },
                    onCommit = { },
                    label = { Text(stringResource(id = R.string.hint_new_title)) },
                    singleLine = true,
                )
            },
        )
    } else {
        SujianDialog(
            onDismissRequest = onDismiss,
            title = project.title,
            confirmText = "",
            onConfirm = {},
            body = {
                Column {
                    // #610 评论四：菜单项按 Core Context(Project) spec 渲染，
                    // 顺序来自 Core order；Composable 不再自行决定出现哪些动作、排第几个。
                    actions.forEach { action ->
                        when (action.kind) {
                            WorkspaceActionKind.Rename ->
                                SujianListItem(
                                    headline = stringResource(id = R.string.action_rename),
                                    onClick = { showRename = true },
                                )
                            WorkspaceActionKind.Delete ->
                                SujianListItem(
                                    headline = stringResource(id = R.string.action_delete),
                                    onClick = { onDelete(action) },
                                )
                            else -> {
                                // 其它角色不属于作品 Context 契约，不渲染。
                            }
                        }
                    }
                }
            },
        )
    }
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
