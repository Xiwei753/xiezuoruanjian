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
import androidx.compose.material.icons.filled.ChevronRight
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
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel

@Composable
fun VolumeChapterTree(
    projectId: String,
    workspaceRepository: com.xiwei.sujian.data.WorkspaceRepository,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    modifier: Modifier = Modifier
) {
    val viewModel: WorkspaceViewModel = viewModel()
    viewModel.initialize(projectId, workspaceRepository)
    val uiState by viewModel.uiState.collectAsState()

    Column(modifier = modifier) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text("卷章", style = MaterialTheme.typography.titleMedium)
            IconButton(onClick = {
                showCreateVolumeDialog(viewModel)
            }) {
                Icon(Icons.Default.Add, contentDescription = "新建卷")
            }
        }

        uiState.projectStats?.let { stats ->
            Text(
                "共 ${stats.volumeCount} 卷 · ${stats.chapterCount} 章 · ${stats.totalWordCount} 字",
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)
            )
        }

        if (uiState.volumes.isEmpty() && !uiState.isLoading) {
            Text(
                "暂无卷章数据",
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(16.dp)
            )
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp)
            ) {
                items(uiState.volumes, key = { it.id }) { volume ->
                    VolumeRow(
                        volume = volume,
                        onToggleExpand = { viewModel.toggleVolumeExpand(volume.id) },
                        onCreateChapter = {
                            showCreateChapterDialog(viewModel, volume.id, volume.title)
                        },
                        onMoreActions = {
                            showVolumeActionsDialog(viewModel, volume)
                        }
                    )
                    if (volume.isExpanded) {
                        if (volume.chapters.isEmpty()) {
                            item(key = "empty_${volume.id}") {
                                Text(
                                    "暂无章节",
                                    style = MaterialTheme.typography.bodySmall,
                                    modifier = Modifier.padding(start = 48.dp, top = 4.dp, bottom = 4.dp)
                                )
                            }
                        } else {
                            items(volume.chapters, key = { "${volume.id}_${it.id}" }) { chapter ->
                                ChapterRow(
                                    chapter = chapter,
                                    isSelected = chapter.id == uiState.selectedChapterId,
                                    onSelect = {
                                        viewModel.selectChapter(chapter.id)
                                        onSelectChapter(volume.id, chapter.id, chapter.title)
                                    },
                                    onMoreActions = {
                                        showChapterActionsDialog(viewModel, volume.id, chapter)
                                    }
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun showCreateVolumeDialog(viewModel: WorkspaceViewModel) {
    var showDialog by remember { mutableStateOf(true) }
    if (showDialog) {
        var title by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { showDialog = false },
            title = { Text("新建卷") },
            text = {
                OutlinedTextField(
                    value = title,
                    onValueChange = { title = it },
                    label = { Text("卷标题") },
                    singleLine = true
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    if (title.isNotBlank()) {
                        viewModel.createVolume(title.trim())
                    }
                    showDialog = false
                }) { Text("创建") }
            },
            dismissButton = {
                TextButton(onClick = { showDialog = false }) { Text("取消") }
            }
        )
    }
}

@Composable
private fun showCreateChapterDialog(viewModel: WorkspaceViewModel, volumeId: String, volumeTitle: String) {
    var showDialog by remember { mutableStateOf(true) }
    if (showDialog) {
        var title by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { showDialog = false },
            title = { Text("在「$volumeTitle」中新建章节") },
            text = {
                OutlinedTextField(
                    value = title,
                    onValueChange = { title = it },
                    label = { Text("章节标题") },
                    singleLine = true
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    if (title.isNotBlank()) {
                        viewModel.createChapter(volumeId, title.trim())
                    }
                    showDialog = false
                }) { Text("创建") }
            },
            dismissButton = {
                TextButton(onClick = { showDialog = false }) { Text("取消") }
            }
        )
    }
}

@Composable
private fun showVolumeActionsDialog(viewModel: WorkspaceViewModel, volume: VolumeUiModel) {
    var showDialog by remember { mutableStateOf(true) }
    var showRename by remember { mutableStateOf(false) }

    if (showRename) {
        var newTitle by remember { mutableStateOf(volume.title) }
        AlertDialog(
            onDismissRequest = { showRename = false; showDialog = false },
            title = { Text("重命名卷") },
            text = {
                OutlinedTextField(
                    value = newTitle,
                    onValueChange = { newTitle = it },
                    label = { Text("新标题") },
                    singleLine = true
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    if (newTitle.isNotBlank()) viewModel.renameVolume(volume.id, newTitle.trim())
                    showRename = false; showDialog = false
                }) { Text("确定") }
            },
            dismissButton = {
                TextButton(onClick = { showRename = false; showDialog = false }) { Text("取消") }
            }
        )
    } else if (showDialog) {
        AlertDialog(
            onDismissRequest = { showDialog = false },
            title = { Text(volume.title) },
            text = {
                Column {
                    androidx.compose.material3.DropdownMenuItem(
                        text = { Text("重命名") },
                        onClick = { showRename = true }
                    )
                    androidx.compose.material3.DropdownMenuItem(
                        text = { Text("删除") },
                        onClick = {
                            viewModel.deleteVolume(volume.id)
                            showDialog = false
                        }
                    )
                }
            },
            confirmButton = {},
            dismissButton = {
                TextButton(onClick = { showDialog = false }) { Text("取消") }
            }
        )
    }
}

@Composable
private fun showChapterActionsDialog(viewModel: WorkspaceViewModel, volumeId: String, chapter: ChapterUiModel) {
    var showDialog by remember { mutableStateOf(true) }
    var showRename by remember { mutableStateOf(false) }

    if (showRename) {
        var newTitle by remember { mutableStateOf(chapter.title) }
        AlertDialog(
            onDismissRequest = { showRename = false; showDialog = false },
            title = { Text("重命名章节") },
            text = {
                OutlinedTextField(
                    value = newTitle,
                    onValueChange = { newTitle = it },
                    label = { Text("新标题") },
                    singleLine = true
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    if (newTitle.isNotBlank()) viewModel.renameChapter(volumeId, chapter.id, newTitle.trim())
                    showRename = false; showDialog = false
                }) { Text("确定") }
            },
            dismissButton = {
                TextButton(onClick = { showRename = false; showDialog = false }) { Text("取消") }
            }
        )
    } else if (showDialog) {
        AlertDialog(
            onDismissRequest = { showDialog = false },
            title = { Text(chapter.title) },
            text = {
                Column {
                    androidx.compose.material3.DropdownMenuItem(
                        text = { Text("重命名") },
                        onClick = { showRename = true }
                    )
                    androidx.compose.material3.DropdownMenuItem(
                        text = { Text("删除") },
                        onClick = {
                            viewModel.deleteChapter(volumeId, chapter.id)
                            showDialog = false
                        }
                    )
                }
            },
            confirmButton = {},
            dismissButton = {
                TextButton(onClick = { showDialog = false }) { Text("取消") }
            }
        )
    }
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
                    Icons.Default.ChevronRight,
                    contentDescription = if (volume.isExpanded) "折叠" else "展开",
                    modifier = Modifier.size(24.dp)
                )
            }
        },
        trailingContent = {
            Row {
                IconButton(onClick = onCreateChapter) {
                    Icon(
                        Icons.Default.Add,
                        contentDescription = "新建章节",
                        modifier = Modifier.size(24.dp)
                    )
                }
                IconButton(onClick = onMoreActions) {
                    Icon(
                        Icons.Default.MoreVert,
                        contentDescription = "更多",
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
            { Text("${chapter.wordCount}字") }
        } else null,
        trailingContent = {
            IconButton(onClick = onMoreActions) {
                Icon(
                    Icons.Default.MoreVert,
                    contentDescription = "更多",
                    modifier = Modifier.size(24.dp)
                )
            }
        },
        modifier = modifier.clickable(onClick = onSelect)
    )
}
