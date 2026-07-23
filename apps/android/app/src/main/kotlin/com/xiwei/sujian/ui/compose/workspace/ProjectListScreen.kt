package com.xiwei.sujian.ui.compose.workspace

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import com.xiwei.sujian.designsystem.icon.SujianIcons
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import com.xiwei.sujian.editor.v2.compose.AnimatedTextField
import androidx.compose.material3.Text
import com.xiwei.sujian.designsystem.component.SujianCard
import com.xiwei.sujian.designsystem.component.SujianDialog
import com.xiwei.sujian.designsystem.component.SujianFab
import com.xiwei.sujian.designsystem.component.SujianIconButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.model.Project
import com.xiwei.sujian.model.RecentEdit
import com.xiwei.sujian.ui.compose.SujianAppState

@Composable
fun ProjectListScreen(
    appState: SujianAppState,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    modifier: Modifier = Modifier
) {
    var showCreateDialog by remember { mutableStateOf(false) }
    var showMenuForProject by remember { mutableStateOf<Project?>(null) }

    Box(modifier = modifier.fillMaxSize()) {
        if (appState.projects.isEmpty()) {
            Column(
                modifier = Modifier.fillMaxSize().padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text(stringResource(id = R.string.project_list_empty), style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(8.dp))
                Text(stringResource(id = R.string.project_list_empty_hint), style = MaterialTheme.typography.bodyMedium)
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                modifier = Modifier.fillMaxSize()
            ) {
                if (appState.recentEdits.isNotEmpty()) {
                    item {
                        Text(stringResource(id = R.string.recent_edits), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(bottom = 8.dp))
                    }
                    items(appState.recentEdits) { edit ->
                        val project = appState.projects.find { it.id == edit.projectId }
                        SujianCard(
                            onClick = { onSelectProject(edit.projectId, project?.title ?: "") },
                            modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
                        ) {
                            Column(modifier = Modifier.padding(16.dp)) {
                                Text(project?.title ?: stringResource(id = R.string.unknown_project), style = MaterialTheme.typography.titleMedium)
                                Text(stringResource(id = R.string.continue_writing_action), style = MaterialTheme.typography.bodySmall)
                            }
                        }
                    }
                    item {
                        Spacer(modifier = Modifier.height(16.dp))
                        Text(stringResource(id = R.string.all_projects), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(bottom = 8.dp))
                    }
                }
                items(appState.projects) { project ->
                    ProjectCard(
                        project = project,
                        onSelect = { onSelectProject(project.id, project.title) },
                        onMoreActions = { showMenuForProject = project }
                    )
                }
            }
        }

        SujianFab(
            onClick = { showCreateDialog = true },
            icon = SujianIcons.Add,
            contentDescription = stringResource(id = R.string.action_new_project),
            modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp)
        )
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
                    singleLine = true
                )
            }
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
            onDismiss = { showMenuForProject = null }
        )
    }
}

@Composable
private fun ProjectCard(
    project: Project,
    onSelect: () -> Unit,
    onMoreActions: () -> Unit
) {
    SujianCard(
        onClick = onSelect,
        modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
    ) {
        Row(
            modifier = Modifier.padding(16.dp).fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
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
    onDismiss: () -> Unit
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
                    singleLine = true
                )
            }
        )
    } else {
        SujianDialog(
            onDismissRequest = onDismiss,
            title = project.title,
            confirmText = "",
            onConfirm = {},
            body = {
                Column {
                    DropdownMenuItem(
                        text = { Text(stringResource(id = R.string.action_rename)) },
                        onClick = { showRename = true }
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(id = R.string.action_delete)) },
                        onClick = onDelete
                    )
                }
            }
        )
    }
}
