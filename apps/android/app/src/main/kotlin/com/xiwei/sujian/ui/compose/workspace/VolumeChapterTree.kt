package com.xiwei.sujian.ui.compose.workspace

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R

sealed class WorkspaceDialogState {
    data object None : WorkspaceDialogState()
    data class CreateVolume(val dummy: Unit = Unit) : WorkspaceDialogState()
    data class CreateChapter(val volumeId: String, val volumeTitle: String) : WorkspaceDialogState()
    data class RenameVolume(val volume: VolumeUiModel) : WorkspaceDialogState()
    data class RenameChapter(val volumeId: String, val chapter: ChapterUiModel) : WorkspaceDialogState()
    data class DeleteVolume(val volume: VolumeUiModel) : WorkspaceDialogState()
    data class DeleteChapter(val volumeId: String, val chapter: ChapterUiModel) : WorkspaceDialogState()
    data class VolumeActions(val volume: VolumeUiModel) : WorkspaceDialogState()
    data class ChapterActions(val volumeId: String, val chapter: ChapterUiModel) : WorkspaceDialogState()
}

sealed class VolumeChapterListItem {
    data class VolumeItem(val volume: VolumeUiModel) : VolumeChapterListItem()
    data class ChapterItem(val chapter: ChapterUiModel, val volumeId: String) : VolumeChapterListItem()
    data class EmptyChapterHint(val volumeId: String) : VolumeChapterListItem()
}

@Composable
fun VolumeChapterTree(
    projectId: String,
    workspaceRepository: com.xiwei.sujian.data.WorkspaceRepository,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    onBackToProjects: () -> Unit = {},
    modifier: Modifier = Modifier
) {
    val viewModel: WorkspaceViewModel = viewModel()
    viewModel.initialize(projectId, workspaceRepository)
    val uiState by viewModel.uiState.collectAsState()

    var dialogState by remember { mutableStateOf<WorkspaceDialogState>(WorkspaceDialogState.None) }

    val flatItems = remember(uiState.volumes, uiState.expandedVolumeIds) {
        val items = mutableListOf<VolumeChapterListItem>()
        for (volume in uiState.volumes) {
            items.add(VolumeChapterListItem.VolumeItem(volume))
            if (volume.isExpanded) {
                if (volume.chapters.isEmpty()) {
                    items.add(VolumeChapterListItem.EmptyChapterHint(volume.id))
                } else {
                    for (chapter in volume.chapters) {
                        items.add(VolumeChapterListItem.ChapterItem(chapter, volume.id))
                    }
                }
            }
        }
        items
    }

    Column(modifier = modifier) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBackToProjects) {
                Icon(Icons.Default.ArrowBack, contentDescription = stringResource(id = R.string.back_to_project_list))
            }
            Text(stringResource(id = R.string.volume_chapter_title), style = MaterialTheme.typography.titleMedium)
            }
            IconButton(onClick = {
                dialogState = WorkspaceDialogState.CreateVolume()
            }) {
                Icon(Icons.Default.Add, contentDescription = stringResource(id = R.string.action_new_volume_short))
            }
        }

        uiState.projectStats?.let { stats ->
            Text(
                stringResource(R.string.volume_chapter_stats, stats.volumeCount, stats.chapterCount, stats.totalWordCount),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)
            )
        }

        if (flatItems.isEmpty() && !uiState.isLoading) {
            Text(
                stringResource(id = R.string.volume_chapter_empty),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(16.dp)
            )
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp)
            ) {
                items(flatItems, key = { item ->
                    when (item) {
                        is VolumeChapterListItem.VolumeItem -> "vol_${item.volume.id}"
                        is VolumeChapterListItem.ChapterItem -> "ch_${item.volumeId}_${item.chapter.id}"
                        is VolumeChapterListItem.EmptyChapterHint -> "empty_${item.volumeId}"
                    }
                }) { item ->
                    when (item) {
                        is VolumeChapterListItem.VolumeItem -> {
                            VolumeRow(
                                volume = item.volume,
                                onToggleExpand = { viewModel.toggleVolumeExpand(item.volume.id) },
                                onCreateChapter = {
                                    dialogState = WorkspaceDialogState.CreateChapter(item.volume.id, item.volume.title)
                                },
                                onMoreActions = {
                                    dialogState = WorkspaceDialogState.VolumeActions(item.volume)
                                }
                            )
                        }
                        is VolumeChapterListItem.ChapterItem -> {
                            ChapterRow(
                                chapter = item.chapter,
                                isSelected = item.chapter.id == uiState.selectedChapterId,
                                onSelect = {
                                    viewModel.selectChapter(item.chapter.id)
                                    onSelectChapter(item.volumeId, item.chapter.id, item.chapter.title)
                                },
                                onMoreActions = {
                                    dialogState = WorkspaceDialogState.ChapterActions(item.volumeId, item.chapter)
                                }
                            )
                        }
                        is VolumeChapterListItem.EmptyChapterHint -> {
                            Text(
                                stringResource(id = R.string.chapter_list_empty),
                                style = MaterialTheme.typography.bodySmall,
                                modifier = Modifier.padding(start = 48.dp, top = 4.dp, bottom = 4.dp)
                            )
                        }
                    }
                }
            }
        }
    }

    when (val state = dialogState) {
        is WorkspaceDialogState.None -> {}
        is WorkspaceDialogState.CreateVolume -> {
            CreateVolumeDialog(
                onConfirm = { title ->
                    viewModel.createVolume(title)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None }
            )
        }
        is WorkspaceDialogState.CreateChapter -> {
            CreateChapterDialog(
                volumeTitle = state.volumeTitle,
                onConfirm = { title ->
                    viewModel.createChapter(state.volumeId, title)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None }
            )
        }
        is WorkspaceDialogState.VolumeActions -> {
            VolumeActionsDialog(
                volume = state.volume,
                onRename = { dialogState = WorkspaceDialogState.RenameVolume(state.volume) },
                onDelete = {
                    viewModel.deleteVolume(state.volume.id)
                    dialogState = WorkspaceDialogState.None
                },
                onMoveUp = {
                    viewModel.moveVolumeUp(state.volume.id)
                    dialogState = WorkspaceDialogState.None
                },
                onMoveDown = {
                    viewModel.moveVolumeDown(state.volume.id)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None }
            )
        }
        is WorkspaceDialogState.RenameVolume -> {
            RenameDialog(
                title = stringResource(id = R.string.rename_volume),
                initialValue = state.volume.title,
                onConfirm = { newTitle ->
                    viewModel.renameVolume(state.volume.id, newTitle)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None }
            )
        }
        is WorkspaceDialogState.ChapterActions -> {
            ChapterActionsDialog(
                chapter = state.chapter,
                onRename = { dialogState = WorkspaceDialogState.RenameChapter(state.volumeId, state.chapter) },
                onDelete = {
                    viewModel.deleteChapter(state.volumeId, state.chapter.id)
                    dialogState = WorkspaceDialogState.None
                },
                onMoveUp = {
                    viewModel.moveChapterUp(state.volumeId, state.chapter.id)
                    dialogState = WorkspaceDialogState.None
                },
                onMoveDown = {
                    viewModel.moveChapterDown(state.volumeId, state.chapter.id)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None }
            )
        }
        is WorkspaceDialogState.RenameChapter -> {
            RenameDialog(
                title = stringResource(id = R.string.rename_chapter),
                initialValue = state.chapter.title,
                onConfirm = { newTitle ->
                    viewModel.renameChapter(state.volumeId, state.chapter.id, newTitle)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None }
            )
        }
        is WorkspaceDialogState.DeleteVolume -> {
            ConfirmDeleteDialog(
                name = state.volume.title,
                onConfirm = {
                    viewModel.deleteVolume(state.volume.id)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None }
            )
        }
        is WorkspaceDialogState.DeleteChapter -> {
            ConfirmDeleteDialog(
                name = state.chapter.title,
                onConfirm = {
                    viewModel.deleteChapter(state.volumeId, state.chapter.id)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None }
            )
        }
    }
}

@Composable
private fun CreateVolumeDialog(
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit
) {
    var title by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(id = R.string.action_new_volume_short)) },
        text = {
            OutlinedTextField(
                value = title,
                onValueChange = { title = it },
                label = { Text(stringResource(id = R.string.hint_volume_title_short)) },
                singleLine = true
            )
        },
        confirmButton = {
            TextButton(onClick = {
                if (title.isNotBlank()) onConfirm(title.trim())
                else onDismiss()
            }) { Text(stringResource(id = R.string.action_create)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(id = R.string.action_cancel)) }
        }
    )
}

@Composable
private fun CreateChapterDialog(
    volumeTitle: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit
) {
    var title by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.create_chapter_in_volume, volumeTitle)) },
        text = {
            OutlinedTextField(
                value = title,
                onValueChange = { title = it },
                label = { Text(stringResource(id = R.string.hint_chapter_title_short)) },
                singleLine = true
            )
        },
        confirmButton = {
            TextButton(onClick = {
                if (title.isNotBlank()) onConfirm(title.trim())
                else onDismiss()
            }) { Text(stringResource(id = R.string.action_create)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(id = R.string.action_cancel)) }
        }
    )
}

@Composable
private fun VolumeActionsDialog(
    volume: VolumeUiModel,
    onRename: () -> Unit,
    onDelete: () -> Unit,
    onMoveUp: () -> Unit,
    onMoveDown: () -> Unit,
    onDismiss: () -> Unit
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(volume.title) },
        text = {
            Column {
                androidx.compose.material3.DropdownMenuItem(
                    text = { Text(stringResource(id = R.string.action_rename)) },
                    onClick = onRename
                )
                androidx.compose.material3.DropdownMenuItem(
                    text = { Text(stringResource(id = R.string.action_move_up)) },
                    onClick = onMoveUp
                )
                androidx.compose.material3.DropdownMenuItem(
                    text = { Text(stringResource(id = R.string.action_move_down)) },
                    onClick = onMoveDown
                )
                androidx.compose.material3.DropdownMenuItem(
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

@Composable
private fun ChapterActionsDialog(
    chapter: ChapterUiModel,
    onRename: () -> Unit,
    onDelete: () -> Unit,
    onMoveUp: () -> Unit,
    onMoveDown: () -> Unit,
    onDismiss: () -> Unit
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(chapter.title) },
        text = {
            Column {
                androidx.compose.material3.DropdownMenuItem(
                    text = { Text(stringResource(id = R.string.action_rename)) },
                    onClick = onRename
                )
                androidx.compose.material3.DropdownMenuItem(
                    text = { Text(stringResource(id = R.string.action_move_up)) },
                    onClick = onMoveUp
                )
                androidx.compose.material3.DropdownMenuItem(
                    text = { Text(stringResource(id = R.string.action_move_down)) },
                    onClick = onMoveDown
                )
                androidx.compose.material3.DropdownMenuItem(
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

@Composable
private fun RenameDialog(
    title: String,
    initialValue: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit
) {
    var newTitle by remember { mutableStateOf(initialValue) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
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
                if (newTitle.isNotBlank()) onConfirm(newTitle.trim())
                onDismiss()
            }) { Text(stringResource(id = R.string.action_ok)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(id = R.string.action_cancel)) }
        }
    )
}

@Composable
private fun ConfirmDeleteDialog(
    name: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(id = R.string.confirm_delete)) },
        text = { Text(stringResource(R.string.confirm_delete_message, name)) },
        confirmButton = {
            TextButton(onClick = onConfirm) { Text(stringResource(id = R.string.action_delete)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(id = R.string.action_cancel)) }
        }
    )
}

@Composable
fun VolumeRow(
    volume: VolumeUiModel,
    onToggleExpand: () -> Unit,
    onCreateChapter: () -> Unit,
    onMoreActions: () -> Unit,
    modifier: Modifier = Modifier
) {
    ListItem(
        headlineContent = { Text(volume.title) },
        leadingContent = {
            IconButton(onClick = onToggleExpand) {
                Icon(
                    Icons.Default.KeyboardArrowRight,
                    contentDescription = if (volume.isExpanded) stringResource(id = R.string.action_collapse) else stringResource(id = R.string.action_expand),
                    modifier = Modifier.size(24.dp)
                )
            }
        },
        trailingContent = {
            Row {
                IconButton(onClick = onCreateChapter) {
                    Icon(
                        Icons.Default.Add,
                        contentDescription = stringResource(id = R.string.action_new_chapter),
                        modifier = Modifier.size(24.dp)
                    )
                }
                IconButton(onClick = onMoreActions) {
                    Icon(
                        Icons.Default.MoreVert,
                        contentDescription = stringResource(id = R.string.action_more),
                        modifier = Modifier.size(24.dp)
                    )
                }
            }
        },
        modifier = modifier
    )
}

@Composable
fun ChapterRow(
    chapter: ChapterUiModel,
    isSelected: Boolean,
    onSelect: () -> Unit,
    onMoreActions: () -> Unit,
    modifier: Modifier = Modifier
) {
    ListItem(
        headlineContent = {
            Text(
                chapter.title,
                color = if (isSelected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
            )
        },
        supportingContent = if (chapter.wordCount > 0) {
            { Text(stringResource(R.string.word_count_format, chapter.wordCount)) }
        } else null,
        trailingContent = {
            IconButton(onClick = onMoreActions) {
                Icon(
                    Icons.Default.MoreVert,
                    contentDescription = stringResource(id = R.string.action_more),
                    modifier = Modifier.size(24.dp)
                )
            }
        },
        modifier = modifier.clickable(onClick = onSelect)
    )
}
