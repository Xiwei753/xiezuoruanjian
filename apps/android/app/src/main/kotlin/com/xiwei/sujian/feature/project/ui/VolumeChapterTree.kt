package com.xiwei.sujian.feature.project.ui

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
import androidx.compose.runtime.LaunchedEffect
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
import com.xiwei.sujian.app.presentation.screen.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.WorkspaceActionKind
import com.xiwei.sujian.app.presentation.screen.WorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.WorkspaceActionTarget
import com.xiwei.sujian.core.designsystem.component.SujianDialog
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.component.SujianListItem
import com.xiwei.sujian.core.designsystem.component.SujianOverflowMenu
import com.xiwei.sujian.core.designsystem.component.SujianOverflowMenuItem
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
}

sealed class VolumeChapterListItem {
    data class VolumeItem(val volume: VolumeUiModel) : VolumeChapterListItem()

    data class ChapterItem(val chapter: ChapterUiModel, val volumeId: String) : VolumeChapterListItem()

    data class EmptyChapterHint(val volumeId: String) : VolumeChapterListItem()
}

/**
 * #625：把 [WorkspaceActionSpec.kind] 映射到菜单项文字资源 id。
 * 不属于卷/章节 Context 契约的 kind 返回 null，调用方跳过渲染。
 */
private fun workspaceActionLabelRes(action: WorkspaceActionSpec): Int? =
    when (action.kind) {
        WorkspaceActionKind.Rename -> R.string.action_rename
        WorkspaceActionKind.MoveEarlier -> R.string.action_move_up
        WorkspaceActionKind.MoveLater -> R.string.action_move_down
        WorkspaceActionKind.Delete -> R.string.action_delete
        else -> null
    }

/**
 * #625：把 Volume Context 菜单项点击映射到业务回调 — 提取以控制行长度与嵌套深度。
 *
 * MoveEarlier/MoveLater 立即执行并收菜单；Delete 按 Core 契约 [WorkspaceActionSpec.requiresConfirmation]
 * 决定是否进入确认弹窗；Rename 进入重命名输入弹窗。
 */
private fun handleVolumeAction(
    action: WorkspaceActionSpec,
    volume: VolumeUiModel,
    viewModel: ProjectViewModel,
    setDialogState: (WorkspaceDialogState) -> Unit,
) {
    when (action.kind) {
        WorkspaceActionKind.Rename -> setDialogState(WorkspaceDialogState.RenameVolume(volume))
        WorkspaceActionKind.Delete ->
            if (action.requiresConfirmation) {
                setDialogState(WorkspaceDialogState.DeleteVolume(volume))
            } else {
                viewModel.deleteVolume(volume.id)
                setDialogState(WorkspaceDialogState.None)
            }
        WorkspaceActionKind.MoveEarlier -> {
            viewModel.moveVolumeUp(volume.id)
            setDialogState(WorkspaceDialogState.None)
        }
        WorkspaceActionKind.MoveLater -> {
            viewModel.moveVolumeDown(volume.id)
            setDialogState(WorkspaceDialogState.None)
        }
        else -> {}
    }
}

/**
 * #625：把 Chapter Context 菜单项点击映射到业务回调 — 提取以控制行长度与嵌套深度。
 *
 * 行为与 [handleVolumeAction] 对称，作用于章节。
 */
private fun handleChapterAction(
    action: WorkspaceActionSpec,
    volumeId: String,
    chapter: ChapterUiModel,
    viewModel: ProjectViewModel,
    setDialogState: (WorkspaceDialogState) -> Unit,
) {
    when (action.kind) {
        WorkspaceActionKind.Rename -> setDialogState(WorkspaceDialogState.RenameChapter(volumeId, chapter))
        WorkspaceActionKind.Delete ->
            if (action.requiresConfirmation) {
                setDialogState(WorkspaceDialogState.DeleteChapter(volumeId, chapter))
            } else {
                viewModel.deleteChapter(volumeId, chapter.id)
                setDialogState(WorkspaceDialogState.None)
            }
        WorkspaceActionKind.MoveEarlier -> {
            viewModel.moveChapterUp(volumeId, chapter.id)
            setDialogState(WorkspaceDialogState.None)
        }
        WorkspaceActionKind.MoveLater -> {
            viewModel.moveChapterDown(volumeId, chapter.id)
            setDialogState(WorkspaceDialogState.None)
        }
        else -> {}
    }
}

@Composable
internal fun ChapterTreeContent(
    projectId: String,
    projectRepository: com.xiwei.sujian.feature.project.data.ProjectRepository,
    workspaceActions: AndroidWorkspaceActionSpec,
    onSelectChapter: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    modifier: Modifier = Modifier,
    onError: (String) -> Unit = {},
) {
    val viewModel: ProjectViewModel = viewModel()
    // #617 评论一：初始化从组合阶段移入副作用 — 避免随重组反复进入初始化判断；
    // 作品切换时以 projectId 为 key 重新触发，按 key 重新初始化。
    LaunchedEffect(viewModel, projectId, projectRepository) {
        viewModel.initialize(projectId, projectRepository)
    }
    // #617 评论九：收集 ViewModel 错误事件，转交全局 Snackbar（onError）—
    // 不在 feature 里另起 Toast/Snackbar 系统，复用 #614 已有的 WorkspaceUiEvent 链。
    LaunchedEffect(viewModel) {
        viewModel.uiEvents.collect { event ->
            when (event) {
                is ProjectTreeUiEvent.Error -> onError(event.message)
            }
        }
    }
    val uiState by viewModel.uiState.collectAsState()

    var dialogState by remember { mutableStateOf<WorkspaceDialogState>(WorkspaceDialogState.None) }

    val flatItems =
        remember(uiState.volumes, uiState.expandedVolumeIds) {
            val items = mutableListOf<VolumeChapterListItem>()
            for (volume in uiState.volumes) {
                items.add(VolumeChapterListItem.VolumeItem(volume))
                // #617 评论八：展开状态只从 expandedVolumeIds 派生 — UI 模型不携带
                // isExpanded，刷新链写回与用户切换不会互相覆盖。
                if (volume.id in uiState.expandedVolumeIds) {
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
        // #610 评论四：新建卷按钮按 Core ListHeader 契约存在与否渲染。
        val hasCreateVolume =
            workspaceActions.listHeaderActions.any { it.kind == WorkspaceActionKind.CreateVolume }
        Row(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(id = R.string.volume_chapter_title),
                style = MaterialTheme.typography.titleMedium,
            )
            if (hasCreateVolume) {
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
            // #617 评论九：首次加载失败且没有卷时显示 loadError，不要落进空态 —
            // 把"读取失败"伪装成"暂无卷"会让用户误以为作品真的是空的。
            val loadError = uiState.loadError
            if (loadError != null) {
                Text(
                    loadError,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(16.dp),
                )
            } else {
                Text(
                    stringResource(id = R.string.volume_chapter_empty),
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(16.dp),
                )
            }
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
                                // #617 评论八：展开标志显式传入，箭头只认 expandedVolumeIds。
                                isExpanded = item.volume.id in uiState.expandedVolumeIds,
                                actions =
                                    VolumeRowActions(
                                        workspaceActions = workspaceActions,
                                        onToggleExpand = { viewModel.toggleVolumeExpand(item.volume.id) },
                                        onCreateChapter = {
                                            dialogState =
                                                WorkspaceDialogState.CreateChapter(item.volume.id, item.volume.title)
                                        },
                                        // #625：菜单项点击映射到业务回调（提取为 handleVolumeAction）。
                                        onAction = { action ->
                                            handleVolumeAction(action, item.volume, viewModel) { dialogState = it }
                                        },
                                    ),
                            )
                        }
                        is VolumeChapterListItem.ChapterItem -> {
                            ChapterRow(
                                chapter = item.chapter,
                                isSelected = item.chapter.id == uiState.selectedChapterId,
                                actions =
                                    ChapterRowActions(
                                        workspaceActions = workspaceActions,
                                        onSelect = {
                                            viewModel.selectChapter(item.chapter.id)
                                            onSelectChapter(item.volumeId, item.chapter.id, item.chapter.title)
                                        },
                                        onAction = { action ->
                                            handleChapterAction(
                                                action,
                                                item.volumeId,
                                                item.chapter,
                                                viewModel,
                                            ) { dialogState = it }
                                        },
                                    ),
                                volumeId = item.volumeId,
                            )
                        }
                        is VolumeChapterListItem.EmptyChapterHint -> {
                            // #610 评论四：空态必须真正消费 CreateChapter + Volume + EmptyState 槽位，
                            // 不能只有"没有章节"的文字而契约声明了动作。
                            EmptyChapterHint(
                                volumeId = item.volumeId,
                                hasCreateChapter =
                                    workspaceActions.emptyStateActions(WorkspaceActionTarget.Volume)
                                        .any { it.kind == WorkspaceActionKind.CreateChapter },
                                onCreateChapter = {
                                    val volume = uiState.volumes.find { it.id == item.volumeId }
                                    dialogState =
                                        WorkspaceDialogState.CreateChapter(
                                            item.volumeId,
                                            volume?.title.orEmpty(),
                                        )
                                },
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

/** VolumeRow 的 spec 与行内回调 — 打包传递，避免函数参数超出门禁阈值（与 ChapterRowActions 同模式）。 */
internal class VolumeRowActions(
    val workspaceActions: AndroidWorkspaceActionSpec,
    val onToggleExpand: () -> Unit,
    val onCreateChapter: () -> Unit,
    val onAction: (WorkspaceActionSpec) -> Unit,
)

@Composable
internal fun VolumeRow(
    volume: VolumeUiModel,
    isExpanded: Boolean,
    actions: VolumeRowActions,
    modifier: Modifier = Modifier,
) {
    // #610 评论四：行内按钮只按 Core 契约渲染：
    // - CreateChapter + Volume + ItemTrailing → 新建章节图标；
    // - Context(Volume) 非空 → 更多菜单图标。
    val hasCreateChapter =
        actions.workspaceActions.itemTrailingActions(WorkspaceActionTarget.Volume)
            .any { it.kind == WorkspaceActionKind.CreateChapter }
    val hasContextActions = actions.workspaceActions.contextActions(WorkspaceActionTarget.Volume).isNotEmpty()
    // #625：菜单展开状态留在行本地 — 多行各自独立，不会互相覆盖。
    var menuExpanded by remember { mutableStateOf(false) }
    SujianListItem(
        headline = volume.title,
        leadingIcon = if (isExpanded) SujianIcons.KeyboardArrowDown else SujianIcons.KeyboardArrowRight,
        onClick = actions.onToggleExpand,
        semanticId = SujianSemanticIds.volume(volume.id),
        trailingContent = {
            Row {
                if (hasCreateChapter) {
                    SujianIconButton(
                        onClick = actions.onCreateChapter,
                        icon = SujianIcons.Add,
                        contentDescription = stringResource(id = R.string.action_new_chapter),
                        semanticId = SujianSemanticIds.createChapter(volume.id),
                    )
                }
                // #625：⋮ 触发器与 DropdownMenu 在同一组合位置一起画，菜单锚定到按钮左下角；
                // 菜单项按 Core Context(Volume) spec 渲染，顺序来自 Core order。
                if (hasContextActions) {
                    SujianOverflowMenu(
                        expanded = menuExpanded,
                        onExpandedChange = { menuExpanded = it },
                        contentDescription = stringResource(id = R.string.action_more),
                    ) {
                        val volumeActions = actions.workspaceActions.contextActions(WorkspaceActionTarget.Volume)
                        volumeActions.forEach { action ->
                            val labelRes = workspaceActionLabelRes(action)
                            if (labelRes != null) {
                                SujianOverflowMenuItem(
                                    text = stringResource(labelRes),
                                    onClick = {
                                        menuExpanded = false
                                        actions.onAction(action)
                                    },
                                )
                            }
                        }
                    }
                }
            }
        },
        modifier = modifier,
    )
}

/** ChapterRow 的 spec 与行内回调 — 打包传递，避免函数参数超出门禁阈值。 */
internal class ChapterRowActions(
    val workspaceActions: AndroidWorkspaceActionSpec,
    val onSelect: () -> Unit,
    val onAction: (WorkspaceActionSpec) -> Unit,
)

@Composable
internal fun ChapterRow(
    chapter: ChapterUiModel,
    isSelected: Boolean,
    actions: ChapterRowActions,
    modifier: Modifier = Modifier,
    volumeId: String = "",
) {
    // #610 评论四：更多菜单按钮按 Core Context(Chapter) 契约存在与否渲染。
    val hasContextActions = actions.workspaceActions.contextActions(WorkspaceActionTarget.Chapter).isNotEmpty()
    // #625：菜单展开状态留在行本地 — 多行各自独立，不会互相覆盖。
    var menuExpanded by remember { mutableStateOf(false) }
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
        onClick = actions.onSelect,
        semanticId = if (volumeId.isNotEmpty()) SujianSemanticIds.chapter(volumeId, chapter.id) else null,
        trailingContent = {
            // #625：⋮ 触发器与 DropdownMenu 在同一组合位置一起画，菜单锚定到按钮左下角；
            // 菜单项按 Core Context(Chapter) spec 渲染，顺序来自 Core order。
            if (hasContextActions) {
                SujianOverflowMenu(
                    expanded = menuExpanded,
                    onExpandedChange = { menuExpanded = it },
                    contentDescription = stringResource(id = R.string.action_more),
                ) {
                    val chapterActions = actions.workspaceActions.contextActions(WorkspaceActionTarget.Chapter)
                    chapterActions.forEach { action ->
                        val labelRes = workspaceActionLabelRes(action)
                        if (labelRes != null) {
                            SujianOverflowMenuItem(
                                text = stringResource(labelRes),
                                onClick = {
                                    menuExpanded = false
                                    actions.onAction(action)
                                },
                            )
                        }
                    }
                }
            }
        },
        modifier = modifier,
    )
}

/**
 * 空卷提示行（#610 评论四）：真实消费 CreateChapter + Volume + EmptyState 槽位 —
 * 契约声明了"空态新建章节"动作，这里就必须画出来，不能只有文字。
 */
@Composable
private fun EmptyChapterHint(
    volumeId: String,
    hasCreateChapter: Boolean,
    onCreateChapter: () -> Unit,
) {
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(start = 48.dp, top = 4.dp, bottom = 4.dp, end = 8.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            stringResource(id = R.string.chapter_list_empty),
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.weight(1f),
        )
        if (hasCreateChapter) {
            SujianIconButton(
                onClick = onCreateChapter,
                icon = SujianIcons.Add,
                contentDescription = stringResource(id = R.string.action_new_chapter),
                semanticId = SujianSemanticIds.createChapterInEmpty(volumeId),
            )
        }
    }
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
