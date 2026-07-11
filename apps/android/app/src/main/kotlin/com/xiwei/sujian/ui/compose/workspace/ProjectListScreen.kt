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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
                        Card(
                            modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp).clickable {
                                onSelectProject(edit.projectId, project?.title ?: "")
                            }
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

        FloatingActionButton(
            onClick = { showCreateDialog = true },
            modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp)
        ) {
            Icon(Icons.Default.Add, contentDescription = stringResource(id = R.string.action_new_project))
        }
    }

    if (showCreateDialog) {
        var title by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { showCreateDialog = false },
            title = { Text(stringResource(id = R.string.dialog_new_project_title)) },
            text = {
                OutlinedTextField(
                    value = title,
                    onValueChange = { title = it },
                    label = { Text(stringResource(id = R.string.hint_project_title_new)) },
                    singleLine = true
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    if (title.isNotBlank()) {
                        appState.createProject(title.trim())
                        showCreateDialog = false
                    }
                }) { Text(stringResource(id = R.string.action_create)) }
            },
            dismissButton = {
                TextButton(onClick = { showCreateDialog = false }) { Text(stringResource(id = R.string.action_cancel)) }
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
    Card(
        modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp).clickable(onClick = onSelect),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow)
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
            IconButton(onClick = onMoreActions) {
                Icon(Icons.Default.MoreVert, contentDescription = stringResource(id = R.string.action_more))
            }
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
        AlertDialog(
            onDismissRequest = { showRename = false },
            title = { Text(stringResource(id = R.string.action_rename)) },
            text = {
                OutlinedTextField(
                    value = newTitle,
                    onValueChange = { newTitle = it },
                    label = { Text(stringResource(id = R.string.hint_new_title)) },
                    singleLine = true
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    if (newTitle.isNotBlank() && newTitle != project.title) {
                        onRename(newTitle.trim())
                    }
                    showRename = false
                }) { Text(stringResource(id = R.string.action_ok)) }
            },
            dismissButton = {
                TextButton(onClick = { showRename = false }) { Text(stringResource(id = R.string.action_cancel)) }
            }
        )
    } else {
        AlertDialog(
            onDismissRequest = onDismiss,
            title = { Text(project.title) },
            text = {
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
            },
            confirmButton = {},
            dismissButton = {
                TextButton(onClick = onDismiss) { Text(stringResource(id = R.string.action_cancel)) }
            }
        )
    }
}
