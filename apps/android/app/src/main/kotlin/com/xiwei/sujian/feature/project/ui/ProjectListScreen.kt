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
fun ProjectListContent(
    appState: WorkspaceAppState,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    modifier: Modifier = Modifier,
    showFab: Boolean = true,
) {
    var showCreateDialog by remember { mutableStateOf(false) }
    var showMenuForProject by remember { mutableStateOf<Project?>(null) }
    val dims = LocalSujianDimensions.current

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
                        onMoreActions = { showMenuForProject = project },
                        modifier = Modifier.testTag(SujianSemanticIds.project(project.id)),
                    )
                }
            }
        }

        if (showFab) {
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
            onRename = { newTitle ->
                appState.renameProject(project.id, newTitle)
                showMenuForProject = null
            },
            onDelete = {
                appState.deleteProject(project.id)
                showMenuForProject = null
            },
            onDismiss = { showMenuForProject = null },
        )
    }
}

@Composable
fun ProjectListScreen(
    appState: WorkspaceAppState,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    ProjectListContent(
        appState = appState,
        onSelectProject = onSelectProject,
        showFab = true,
        modifier = modifier,
    )
}

@Composable
private fun ProjectCard(
    project: Project,
    onSelect: () -> Unit,
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
            SujianIconButton(
                onClick = onMoreActions,
                icon = SujianIcons.MoreVert,
                contentDescription = stringResource(id = R.string.action_more),
            )
        }
    }
}

@Composable
private fun ProjectMenuDialog(
    project: Project,
    onRename: (String) -> Unit,
    onDelete: () -> Unit,
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
                    SujianListItem(
                        headline = stringResource(id = R.string.action_rename),
                        onClick = { showRename = true },
                    )
                    SujianListItem(
                        headline = stringResource(id = R.string.action_delete),
                        onClick = onDelete,
                    )
                }
            },
        )
    }
}
