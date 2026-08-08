package com.xiwei.sujian.feature.home.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianDialog
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.component.SujianListItem
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

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
fun ChapterTreeContent(
    projectId: String,
    projectRepository: com.xiwei.sujian.core.interop.project.ProjectRepository,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    modifier: Modifier = Modifier,
    showHeader: Boolean = true,
    onBackToProjects: () -> Unit = {},
) {
    val viewModel: WorkspaceViewModel = viewModel()
    viewModel.initialize(projectId, projectRepository)
    val uiState by viewModel.uiState.collectAsState()

    var dialogState by remember { mutableStateOf<WorkspaceDialogState>(WorkspaceDialogState.None) }

    val flatItems =
        remember(uiState.volumes, uiState.expandedVolumeIds) {
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
        if (showHeader) {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    SujianIconButton(
                        onClick = onBackToProjects,
                        icon = SujianIcons.ArrowBack,
                        contentDescription = stringResource(id = R.string.back_to_project_list),
                    )
                    Text(
                        stringResource(id = R.string.volume_chapter_title),
                        style = MaterialTheme.typography.titleMedium,
                    )
                }
                SujianIconButton(
                    onClick = {
                        dialogState = WorkspaceDialogState.CreateVolume()
                    },
                    icon = SujianIcons.Add,
                    contentDescription = stringResource(id = R.string.action_new_volume_short),
                    semanticId = SujianSemanticIds.WorkspaceCreateVolume,
                )
            }
        }

        uiState.projectStats?.let { stats ->
            Text(
                stringResource(
                    R.string.volume_chapter_stats,
                    stats.volumeCount,
                    stats.chapterCount,
                    stats.totalWordCount,
                ),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            )
        }

        if (flatItems.isEmpty() && !uiState.isLoading) {
            Text(
                stringResource(id = R.string.volume_chapter_empty),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(16.dp),
            )
        } else {
            LazyColumn(
                modifier = Modifier.testTag(SujianSemanticIds.WorkspaceVolumeList),
                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
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
                                },
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
                                },
                                volumeId = item.volumeId,
                            )
                        }
                        is VolumeChapterListItem.EmptyChapterHint -> {
                            Text(
                                stringResource(id = R.string.chapter_list_empty),
                                style = MaterialTheme.typography.bodySmall,
                                modifier = Modifier.padding(start = 48.dp, top = 4.dp, bottom = 4.dp),
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
                onDismiss = { dialogState = WorkspaceDialogState.None },
            )
        }
        is WorkspaceDialogState.CreateChapter -> {
            CreateChapterDialog(
                volumeTitle = state.volumeTitle,
                onConfirm = { title ->
                    viewModel.createChapter(state.volumeId, title)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None },
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
                onDismiss = { dialogState = WorkspaceDialogState.None },
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
                onDismiss = { dialogState = WorkspaceDialogState.None },
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
                onDismiss = { dialogState = WorkspaceDialogState.None },
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
                onDismiss = { dialogState = WorkspaceDialogState.None },
            )
        }
        is WorkspaceDialogState.DeleteVolume -> {
            ConfirmDeleteDialog(
                name = state.volume.title,
                onConfirm = {
                    viewModel.deleteVolume(state.volume.id)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None },
            )
        }
        is WorkspaceDialogState.DeleteChapter -> {
            ConfirmDeleteDialog(
                name = state.chapter.title,
                onConfirm = {
                    viewModel.deleteChapter(state.volumeId, state.chapter.id)
                    dialogState = WorkspaceDialogState.None
                },
                onDismiss = { dialogState = WorkspaceDialogState.None },
            )
        }
    }
}

@Composable
fun VolumeChapterTree(
    projectId: String,
    projectRepository: com.xiwei.sujian.core.interop.project.ProjectRepository,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    modifier: Modifier = Modifier,
    onBackToProjects: () -> Unit = {},
) {
    ChapterTreeContent(
        projectId = projectId,
        projectRepository = projectRepository,
        onSelectChapter = onSelectChapter,
        showHeader = true,
        onBackToProjects = onBackToProjects,
        modifier = modifier,
    )
}

@Composable
private fun CreateVolumeDialog(
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var title by remember { mutableStateOf("") }
    SujianDialog(
        onDismissRequest = onDismiss,
        title = stringResource(id = R.string.action_new_volume_short),
        confirmText = stringResource(id = R.string.action_create),
        onConfirm = {
            if (title.isNotBlank()) {
                onConfirm(title.trim())
            } else {
                onDismiss()
            }
        },
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = onDismiss,
        body = {
            OutlinedTextField(
                value = title,
                onValueChange = { title = it },
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .testTag("volume-title:new"),
                label = { Text(stringResource(id = R.string.hint_volume_title_short)) },
                singleLine = true,
            )
        },
    )
}

@Composable
private fun CreateChapterDialog(
    volumeTitle: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var title by remember { mutableStateOf("") }
    SujianDialog(
        onDismissRequest = onDismiss,
        title = stringResource(R.string.create_chapter_in_volume, volumeTitle),
        confirmText = stringResource(id = R.string.action_create),
        onConfirm = {
            if (title.isNotBlank()) {
                onConfirm(title.trim())
            } else {
                onDismiss()
            }
        },
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = onDismiss,
        body = {
            OutlinedTextField(
                value = title,
                onValueChange = { title = it },
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .testTag(SujianSemanticIds.ChapterTitleInput),
                label = { Text(stringResource(id = R.string.hint_chapter_title_short)) },
                singleLine = true,
            )
        },
    )
}

@Composable
private fun VolumeActionsDialog(
    volume: VolumeUiModel,
    onRename: () -> Unit,
    onDelete: () -> Unit,
    onMoveUp: () -> Unit,
    onMoveDown: () -> Unit,
    onDismiss: () -> Unit,
) {
    SujianDialog(
        onDismissRequest = onDismiss,
        title = volume.title,
        confirmText = "",
        onConfirm = {},
        body = {
            Column {
                SujianListItem(
                    headline = stringResource(id = R.string.action_rename),
                    onClick = onRename,
                )
                SujianListItem(
                    headline = stringResource(id = R.string.action_move_up),
                    onClick = onMoveUp,
                )
                SujianListItem(
                    headline = stringResource(id = R.string.action_move_down),
                    onClick = onMoveDown,
                )
                SujianListItem(
                    headline = stringResource(id = R.string.action_delete),
                    onClick = onDelete,
                )
            }
        },
    )
}

@Composable
private fun ChapterActionsDialog(
    chapter: ChapterUiModel,
    onRename: () -> Unit,
    onDelete: () -> Unit,
    onMoveUp: () -> Unit,
    onMoveDown: () -> Unit,
    onDismiss: () -> Unit,
) {
    SujianDialog(
        onDismissRequest = onDismiss,
        title = chapter.title,
        confirmText = "",
        onConfirm = {},
        body = {
            Column {
                SujianListItem(
                    headline = stringResource(id = R.string.action_rename),
                    onClick = onRename,
                )
                SujianListItem(
                    headline = stringResource(id = R.string.action_move_up),
                    onClick = onMoveUp,
                )
                SujianListItem(
                    headline = stringResource(id = R.string.action_move_down),
                    onClick = onMoveDown,
                )
                SujianListItem(
                    headline = stringResource(id = R.string.action_delete),
                    onClick = onDelete,
                )
            }
        },
    )
}

@Composable
private fun RenameDialog(
    title: String,
    initialValue: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var newTitle by remember { mutableStateOf(initialValue) }
    SujianDialog(
        onDismissRequest = onDismiss,
        title = title,
        confirmText = stringResource(id = R.string.action_ok),
        onConfirm = {
            if (newTitle.isNotBlank()) onConfirm(newTitle.trim())
            onDismiss()
        },
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = onDismiss,
        body = {
            OutlinedTextField(
                value = newTitle,
                onValueChange = { newTitle = it },
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .testTag("rename:$initialValue"),
                label = { Text(stringResource(id = R.string.hint_new_title)) },
                singleLine = true,
            )
        },
    )
}

@Composable
private fun ConfirmDeleteDialog(
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

@Composable
fun VolumeRow(
    volume: VolumeUiModel,
    onToggleExpand: () -> Unit,
    onCreateChapter: () -> Unit,
    onMoreActions: () -> Unit,
    modifier: Modifier = Modifier,
) {
    SujianListItem(
        headline = volume.title,
        leadingIcon = if (volume.isExpanded) SujianIcons.KeyboardArrowDown else SujianIcons.KeyboardArrowRight,
        onClick = onToggleExpand,
        semanticId = SujianSemanticIds.volume(volume.id),
        trailingContent = {
            Row {
                SujianIconButton(
                    onClick = onCreateChapter,
                    icon = SujianIcons.Add,
                    contentDescription = stringResource(id = R.string.action_new_chapter),
                    semanticId = SujianSemanticIds.createChapter(volume.id),
                )
                SujianIconButton(
                    onClick = onMoreActions,
                    icon = SujianIcons.MoreVert,
                    contentDescription = stringResource(id = R.string.action_more),
                )
            }
        },
        modifier = modifier,
    )
}

@Composable
fun ChapterRow(
    chapter: ChapterUiModel,
    isSelected: Boolean,
    onSelect: () -> Unit,
    onMoreActions: () -> Unit,
    modifier: Modifier = Modifier,
    volumeId: String = "",
) {
    SujianListItem(
        headline = chapter.title,
        supportingText =
            if (chapter.wordCount > 0) {
                stringResource(
                    R.string.word_count_format,
                    chapter.wordCount,
                )
            } else {
                null
            },
        selected = isSelected,
        onClick = onSelect,
        semanticId = if (volumeId.isNotEmpty()) SujianSemanticIds.chapter(volumeId, chapter.id) else null,
        trailingContent = {
            SujianIconButton(
                onClick = onMoreActions,
                icon = SujianIcons.MoreVert,
                contentDescription = stringResource(id = R.string.action_more),
            )
        },
        modifier = modifier,
    )
}
