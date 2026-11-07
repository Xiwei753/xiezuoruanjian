package com.xiwei.sujian.feature.starmap.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.core.designsystem.component.SujianOverflowMenu
import com.xiwei.sujian.core.designsystem.component.SujianOverflowMenuItem
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.starmap.data.StarMapRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

private const val GlobalLibraryScopeJson = "{\"kind\":\"global\"}"

/** 素材库中的一个可插入目标；本体 ID 与画布实例 ID 不混用。 */
internal data class StarMapLibraryTarget(
    val kind: String,
    val assetId: String,
    val title: String,
    val entryId: String = "",
    val parentFolderId: String? = null,
    val scopeJson: String = GlobalLibraryScopeJson,
    val scopeKind: String = "global",
    val projectId: String? = null,
    val targetKind: String = kind,
) {
    val isStarmap: Boolean get() = isStarMapLibraryKind(kind)

    /** Core 只接受原始 target.kind；UI 展示使用归一化后的 [kind]。 */
    fun targetJson(): String =
        JSONObject()
            .put("kind", targetKind.ifBlank { if (isStarmap) "starMap" else "node" })
            .put("assetId", assetId)
            .toString()
}

internal data class StarMapLibraryFolder(
    val folderId: String,
    val parentFolderId: String?,
    val name: String,
    val scopeJson: String,
)

internal data class StarMapLibraryList(
    val entries: List<StarMapLibraryTarget> = emptyList(),
    val folders: List<StarMapLibraryFolder> = emptyList(),
    val error: String? = null,
)

private data class StarMapLibraryReference(
    val targetKind: String,
    val assetId: String,
    val projectId: String?,
    val starmapId: String,
    val instanceId: String,
)

private data class FolderDialogTarget(
    val scopeJson: String,
    val parentFolderId: String?,
)

private data class FolderMoveRequest(
    val entry: StarMapLibraryTarget,
    val folders: List<StarMapLibraryFolder>,
)

private data class FolderPickerOption(
    val folder: StarMapLibraryFolder,
    val depth: Int,
)

private data class ReferencesDialogState(
    val title: String,
    val references: List<StarMapLibraryReference>,
)

private data class LibraryRowActions(
    val onOpen: (StarMapLibraryTarget) -> Unit,
    val onInsert: (StarMapLibraryTarget) -> Unit,
    val onCopy: (StarMapLibraryTarget) -> Unit,
    val onMoveOwnership: ((StarMapLibraryTarget) -> Unit)?,
    val onMoveToFolder: (StarMapLibraryTarget) -> Unit,
    val onShowReferences: (StarMapLibraryTarget) -> Unit,
    val onCreateSubfolder: (StarMapLibraryFolder) -> Unit,
)

internal fun isStarMapLibraryKind(kind: String): Boolean = kind.equals("starmap", ignoreCase = true)

/**
 * 星图素材库抽屉（#1037）。
 *
 * 全局库和当前作品库同时展示；文件夹按 Core 的 parentFolderId 递归展开。
 * 单击打开素材本体，插入、复制、移动归属与引用位置都在素材菜单中完成，
 * Android 端不保存任何素材本体或归属状态。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun StarMapLibrarySheet(
    repository: StarMapRepository,
    projectId: String?,
    onDismiss: () -> Unit,
    onOpen: (StarMapLibraryTarget) -> Unit,
    onInsert: (StarMapLibraryTarget) -> Unit,
    onOpenReference: (starmapId: String, instanceId: String, kind: String, assetId: String) -> Unit,
) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        StarMapLibraryPanel(
            repository = repository,
            projectId = projectId,
            modifier = Modifier.fillMaxWidth(),
            contentPadding = PaddingValues(start = 20.dp, end = 20.dp, bottom = 24.dp),
            onOpen = onOpen,
            onInsert = onInsert,
            onOpenReference = onOpenReference,
        )
    }
}

@Suppress("LongMethod", "CyclomaticComplexMethod", "CognitiveComplexMethod")
@Composable
private fun StarMapLibraryPanel(
    repository: StarMapRepository,
    projectId: String?,
    modifier: Modifier,
    contentPadding: PaddingValues,
    onOpen: (StarMapLibraryTarget) -> Unit,
    onInsert: (StarMapLibraryTarget) -> Unit,
    onOpenReference: (starmapId: String, instanceId: String, kind: String, assetId: String) -> Unit,
) {
    val projectScope =
        projectId?.takeIf { it.isNotBlank() }
            ?.let { "{\"kind\":\"project\",\"projectId\":${JSONObject.quote(it)}}" }
    val coroutineScope = rememberCoroutineScope()
    var reloadToken by remember { mutableIntStateOf(0) }
    var globalLibrary by remember { mutableStateOf(StarMapLibraryList()) }
    var projectLibrary by remember { mutableStateOf(StarMapLibraryList()) }
    var expandedFolders by remember { mutableStateOf<Set<String>>(emptySet()) }
    var folderDialog by remember { mutableStateOf<FolderDialogTarget?>(null) }
    var folderName by remember { mutableStateOf("") }
    var folderMoveRequest by remember { mutableStateOf<FolderMoveRequest?>(null) }
    var referencesDialog by remember { mutableStateOf<ReferencesDialogState?>(null) }
    var actionError by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(repository, projectScope, reloadToken) {
        globalLibrary = loadLibrary(repository, GlobalLibraryScopeJson)
        projectLibrary = projectScope?.let { loadLibrary(repository, it) } ?: StarMapLibraryList()
    }

    fun runLibraryAction(action: suspend () -> BridgeResult<*>) {
        coroutineScope.launch {
            actionError = null
            when (val result = withContext(Dispatchers.IO) { action() }) {
                is BridgeResult.Success -> reloadToken++
                is BridgeResult.Error -> actionError = result.message.ifBlank { result.code ?: "操作失败" }
                BridgeResult.NotLoaded -> actionError = "Core 尚未加载"
            }
        }
    }

    fun showReferences(target: StarMapLibraryTarget) {
        coroutineScope.launch {
            actionError = null
            when (
                val result =
                    withContext(Dispatchers.IO) {
                        repository.listStarmapAssetReferencesJson(target.targetJson())
                    }
            ) {
                is BridgeResult.Success -> {
                    try {
                        referencesDialog =
                            ReferencesDialogState(
                                title = target.title,
                                references = parseReferences(result.data, target),
                            )
                    } catch (e: Exception) {
                        actionError = e.message ?: "引用位置解析失败"
                    }
                }
                is BridgeResult.Error -> actionError = result.message.ifBlank { result.code ?: "读取引用位置失败" }
                BridgeResult.NotLoaded -> actionError = "Core 尚未加载"
            }
        }
    }

    fun moveEntry(
        target: StarMapLibraryTarget,
        scopeJson: String,
        parentFolderId: String?,
    ) {
        if (target.entryId.isBlank()) {
            actionError = "素材记录缺少 entryId"
            return
        }
        if (parentFolderId != null) {
            expandedFolders = expandedFolders + parentFolderId
        }
        runLibraryAction {
            repository.moveStarmapLibraryEntry(target.entryId, scopeJson, parentFolderId)
        }
    }

    fun createFolder(
        scopeJson: String,
        parentFolderId: String?,
        name: String,
    ) {
        if (parentFolderId != null) {
            expandedFolders = expandedFolders + parentFolderId
        }
        runLibraryAction {
            repository.createStarmapLibraryFolder(scopeJson, parentFolderId, name)
        }
    }

    val openEntry: (StarMapLibraryTarget) -> Unit = { target ->
        if (target.isStarmap) {
            onOpen(target)
        } else {
            // 节点素材没有独立画布；单击先展示它在哪些画布中有实例。
            showReferences(target)
        }
    }
    val toggleFolder: (String) -> Unit = { folderId ->
        expandedFolders =
            if (folderId in expandedFolders) {
                expandedFolders - folderId
            } else {
                expandedFolders + folderId
            }
    }
    val copyEntry: (StarMapLibraryTarget) -> Unit = { target ->
        runLibraryAction {
            repository.copyStarmapLibraryAsset(target.targetJson(), target.scopeJson, null)
        }
    }
    fun libraryFoldersFor(target: StarMapLibraryTarget): List<StarMapLibraryFolder> =
        if (target.scopeKind == "project") projectLibrary.folders else globalLibrary.folders
    val moveToFolder: (StarMapLibraryTarget) -> Unit = { target ->
        folderMoveRequest = FolderMoveRequest(target, libraryFoldersFor(target))
    }

    LazyColumn(
        modifier = modifier,
        contentPadding = contentPadding,
    ) {
        item {
            Text(
                text = "素材库",
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                text = "单击打开素材；插入、复制、移动和引用位置在条目菜单中操作。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 4.dp, bottom = 8.dp),
            )
        }
        actionError?.let { message ->
            item {
                Text(
                    text = message,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(vertical = 6.dp),
                )
            }
        }
        item {
            LibrarySectionHeader(
                title = "全局素材库",
                onCreateFolder = {
                    folderDialog = FolderDialogTarget(GlobalLibraryScopeJson, null)
                    folderName = ""
                },
            )
        }
        libraryItemRows(
            library = globalLibrary,
            expandedFolderIds = expandedFolders,
            onToggleFolder = toggleFolder,
            actions =
                LibraryRowActions(
                    onOpen = openEntry,
                    onInsert = onInsert,
                    onCopy = copyEntry,
                    onMoveOwnership =
                        projectScope?.let { otherScope ->
                            { target -> moveEntry(target, otherScope, null) }
                        },
                    onMoveToFolder = moveToFolder,
                    onShowReferences = ::showReferences,
                    onCreateSubfolder = { folder ->
                        folderDialog = FolderDialogTarget(folder.scopeJson, folder.folderId)
                        folderName = ""
                    },
                ),
        )
        item {
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            LibrarySectionHeader(
                title = "当前作品素材库",
                onCreateFolder =
                    projectScope?.let {
                        {
                            folderDialog = FolderDialogTarget(it, null)
                            folderName = ""
                        }
                    },
            )
            if (projectScope == null) {
                Text(
                    text = "当前入口没有作品归属，仅显示全局素材库。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(vertical = 8.dp),
                )
            }
        }
        libraryItemRows(
            library = projectLibrary,
            expandedFolderIds = expandedFolders,
            onToggleFolder = toggleFolder,
            actions =
                LibraryRowActions(
                    onOpen = openEntry,
                    onInsert = onInsert,
                    onCopy = copyEntry,
                    onMoveOwnership = { target -> moveEntry(target, GlobalLibraryScopeJson, null) },
                    onMoveToFolder = moveToFolder,
                    onShowReferences = ::showReferences,
                    onCreateSubfolder = { folder ->
                        folderDialog = FolderDialogTarget(folder.scopeJson, folder.folderId)
                        folderName = ""
                    },
                ),
        )
    }

    folderDialog?.let { request ->
        AlertDialog(
            onDismissRequest = { folderDialog = null },
            title = { Text(if (request.parentFolderId == null) "新建素材文件夹" else "新建子文件夹") },
            text = {
                OutlinedTextField(
                    value = folderName,
                    onValueChange = { folderName = it },
                    singleLine = true,
                    label = { Text("文件夹名称") },
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        val name = folderName.trim()
                        if (name.isNotEmpty()) {
                            createFolder(request.scopeJson, request.parentFolderId, name)
                        }
                        folderDialog = null
                    },
                    enabled = folderName.isNotBlank(),
                ) { Text("创建") }
            },
            dismissButton = {
                TextButton(onClick = { folderDialog = null }) { Text("取消") }
            },
        )
    }

    folderMoveRequest?.let { request ->
        val options = remember(request.folders) { flattenFolders(request.folders) }
        AlertDialog(
            onDismissRequest = { folderMoveRequest = null },
            title = { Text("移动“${request.entry.title}”到文件夹") },
            text = {
                LazyColumn(modifier = Modifier.heightIn(max = 360.dp)) {
                    item {
                        FolderPickerRow(
                            label = "素材库根目录",
                            depth = 0,
                            selected = request.entry.parentFolderId == null,
                            onClick = {
                                moveEntry(request.entry, request.entry.scopeJson, null)
                                folderMoveRequest = null
                            },
                        )
                    }
                    items(options.size, key = { options[it].folder.folderId }) { index ->
                        val option = options[index]
                        FolderPickerRow(
                            label = option.folder.name,
                            depth = option.depth + 1,
                            selected = request.entry.parentFolderId == option.folder.folderId,
                            onClick = {
                                moveEntry(request.entry, request.entry.scopeJson, option.folder.folderId)
                                folderMoveRequest = null
                            },
                        )
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { folderMoveRequest = null }) { Text("取消") }
            },
        )
    }

    referencesDialog?.let { state ->
        AlertDialog(
            onDismissRequest = { referencesDialog = null },
            title = { Text("引用位置 · ${state.title}") },
            text = {
                if (state.references.isEmpty()) {
                    Text(
                        text = "没有画布引用",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    LazyColumn(modifier = Modifier.heightIn(max = 360.dp)) {
                        items(state.references.size, key = { index ->
                            val reference = state.references[index]
                            "${reference.starmapId}:${reference.instanceId}"
                        }) { index ->
                            val reference = state.references[index]
                            Column(
                                modifier =
                                    Modifier
                                        .fillMaxWidth()
                                        .clickable {
                                            onOpenReference(
                                                reference.starmapId,
                                                reference.instanceId,
                                                reference.targetKind,
                                                reference.assetId,
                                            )
                                            referencesDialog = null
                                        }
                                        .padding(vertical = 8.dp),
                            ) {
                                Text(
                                    text = if (reference.projectId == null) "全局" else "作品 ${reference.projectId}",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                                Text(
                                    text =
                                        if (isStarMapLibraryKind(reference.targetKind)) {
                                            "星图 ${reference.starmapId} / 嵌入 ${reference.instanceId}"
                                        } else {
                                            "星图 ${reference.starmapId} / 节点 ${reference.instanceId}"
                                        },
                                    style = MaterialTheme.typography.bodyMedium,
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { referencesDialog = null }) { Text("关闭") }
            },
        )
    }
}

@Composable
private fun FolderPickerRow(
    label: String,
    depth: Int,
    selected: Boolean,
    onClick: () -> Unit,
) {
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .clickable(onClick = onClick)
                .padding(start = (depth * 16).dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
            color =
                if (selected) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.onSurface
                },
            modifier = Modifier.weight(1f),
        )
        if (selected) {
            Text(
                text = "当前",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

@Suppress("LongMethod")
private fun LazyListScope.libraryItemRows(
    library: StarMapLibraryList,
    expandedFolderIds: Set<String>,
    onToggleFolder: (String) -> Unit,
    actions: LibraryRowActions,
) {
    if (library.error != null) {
        item {
            Text(
                text = library.error,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.padding(vertical = 6.dp),
            )
        }
        return
    }
    if (library.entries.isEmpty() && library.folders.isEmpty()) {
        item {
            Text(
                text = "暂无素材",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(vertical = 6.dp),
            )
        }
        return
    }

    val folderIds = library.folders.mapTo(mutableSetOf()) { it.folderId }
    val foldersByParent =
        library.folders.groupBy { folder -> folder.parentFolderId?.takeIf { it in folderIds } }
    val entriesByParent =
        library.entries.groupBy { entry -> entry.parentFolderId?.takeIf { it in folderIds } }
    val visitedFolders = mutableSetOf<String>()

    fun addLevel(
        parentFolderId: String?,
        depth: Int,
    ) {
        foldersByParent[parentFolderId].orEmpty().forEach { folder ->
            if (!visitedFolders.add(folder.folderId)) return@forEach
            val hasChildren =
                foldersByParent[folder.folderId].orEmpty().isNotEmpty() ||
                    entriesByParent[folder.folderId].orEmpty().isNotEmpty()
            item(key = "folder:${folder.folderId}") {
                LibraryFolderRow(
                    folder = folder,
                    depth = depth,
                    expanded = folder.folderId in expandedFolderIds,
                    hasChildren = hasChildren,
                    onToggle = { onToggleFolder(folder.folderId) },
                    actions = actions,
                )
            }
            if (folder.folderId in expandedFolderIds) {
                addLevel(folder.folderId, depth + 1)
            }
        }
        entriesByParent[parentFolderId].orEmpty().forEach { entry ->
            item(key = "entry:${entry.entryId.ifBlank { "${entry.kind}:${entry.assetId}:$depth" }}") {
                LibraryEntryRow(entry = entry, depth = depth, actions = actions)
            }
        }
    }

    addLevel(null, 0)

    // 目录数据异常（父文件夹缺失/成环）时仍把未显示素材落到根层，避免素材消失。
    library.folders
        .filterNot { it.folderId in visitedFolders }
        .forEach { folder ->
            if (!visitedFolders.add(folder.folderId)) return@forEach
            item(key = "folder:${folder.folderId}") {
                LibraryFolderRow(
                    folder = folder,
                    depth = 0,
                    expanded = false,
                    hasChildren = false,
                    onToggle = { onToggleFolder(folder.folderId) },
                    actions = actions,
                )
            }
        }
}

@Composable
private fun LibraryFolderRow(
    folder: StarMapLibraryFolder,
    depth: Int,
    expanded: Boolean,
    hasChildren: Boolean,
    onToggle: () -> Unit,
    actions: LibraryRowActions,
) {
    var menuExpanded by remember(folder.folderId) { mutableStateOf(false) }
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .heightIn(min = 44.dp)
                .clickable(enabled = hasChildren, onClick = onToggle)
                .padding(start = (4 + depth * 16).dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(
            text = if (!hasChildren) "·" else if (expanded) "▾" else "▸",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(18.dp),
        )
        Text(
            text = "夹",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = folder.name,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        SujianOverflowMenu(
            expanded = menuExpanded,
            onExpandedChange = { menuExpanded = it },
            contentDescription = "文件夹操作",
        ) {
            SujianOverflowMenuItem(
                text = "新建子文件夹",
                onClick = {
                    menuExpanded = false
                    actions.onCreateSubfolder(folder)
                },
            )
        }
    }
}

@Composable
private fun LibraryEntryRow(
    entry: StarMapLibraryTarget,
    depth: Int,
    actions: LibraryRowActions,
) {
    var menuExpanded by remember(entry.entryId) { mutableStateOf(false) }
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .heightIn(min = 44.dp)
                .clickable { actions.onOpen(entry) }
                .padding(start = (4 + depth * 16).dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(
            text = if (entry.isStarmap) "图" else "点",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(18.dp),
        )
        Text(
            text = entry.title,
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        SujianOverflowMenu(
            expanded = menuExpanded,
            onExpandedChange = { menuExpanded = it },
            contentDescription = "素材操作",
        ) {
            SujianOverflowMenuItem(
                text = "插入当前画布",
                onClick = {
                    menuExpanded = false
                    actions.onInsert(entry)
                },
            )
            SujianOverflowMenuItem(
                text = "复制",
                onClick = {
                    menuExpanded = false
                    actions.onCopy(entry)
                },
            )
            actions.onMoveOwnership?.let { moveOwnership ->
                SujianOverflowMenuItem(
                    text = if (entry.scopeKind == "project") "移动到全局库" else "移动到当前作品库",
                    onClick = {
                        menuExpanded = false
                        moveOwnership(entry)
                    },
                )
            }
            SujianOverflowMenuItem(
                text = "移动到文件夹",
                onClick = {
                    menuExpanded = false
                    actions.onMoveToFolder(entry)
                },
            )
            SujianOverflowMenuItem(
                text = "查看引用位置",
                onClick = {
                    menuExpanded = false
                    actions.onShowReferences(entry)
                },
            )
        }
    }
}

@Composable
private fun LibrarySectionHeader(
    title: String,
    onCreateFolder: (() -> Unit)?,
) {
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.weight(1f),
        )
        if (onCreateFolder != null) {
            TextButton(onClick = onCreateFolder) { Text("新建文件夹") }
        }
    }
}

private suspend fun loadLibrary(
    repository: StarMapRepository,
    scopeJson: String,
): StarMapLibraryList =
    withContext(Dispatchers.IO) {
        when (val result = repository.listStarmapLibraryJson(scopeJson)) {
            is BridgeResult.Success -> parseLibrary(result.data, scopeJson)
            is BridgeResult.Error -> StarMapLibraryList(error = result.message)
            BridgeResult.NotLoaded -> StarMapLibraryList(error = "Core 尚未加载")
        }
    }

private fun parseLibrary(
    json: String,
    fallbackScopeJson: String,
): StarMapLibraryList {
    return try {
        val root = JSONObject(json)
        val entries = root.optJSONArray("entries") ?: return StarMapLibraryList(error = "素材库数据缺少 entries")
        val folders = root.optJSONArray("folders") ?: JSONArray()
        val targets = ArrayList<StarMapLibraryTarget>(entries.length())
        for (index in 0 until entries.length()) {
            val view = entries.optJSONObject(index) ?: continue
            val entry = view.optJSONObject("entry") ?: continue
            val target = entry.optJSONObject("target") ?: continue
            val targetKind = target.optString("kind")
            val assetId = target.optString("assetId")
            if (targetKind.isBlank() || assetId.isBlank()) continue
            val scope = entry.optJSONObject("scope")
            val scopeKind = scope?.optionalString("kind") ?: "global"
            targets +=
                StarMapLibraryTarget(
                    kind = if (isStarMapLibraryKind(targetKind)) "starmap" else "node",
                    assetId = assetId,
                    title = view.optString("title").ifBlank { assetId },
                    entryId = entry.optString("entryId"),
                    parentFolderId = entry.optionalString("parentFolderId"),
                    scopeJson = scope?.toString() ?: fallbackScopeJson,
                    scopeKind = scopeKind,
                    projectId = scope?.optionalString("projectId"),
                    targetKind = targetKind,
                )
        }
        val libraryFolders = ArrayList<StarMapLibraryFolder>(folders.length())
        for (index in 0 until folders.length()) {
            val folder = folders.optJSONObject(index) ?: continue
            val folderId = folder.optString("folderId")
            if (folderId.isBlank()) continue
            libraryFolders +=
                StarMapLibraryFolder(
                    folderId = folderId,
                    parentFolderId = folder.optionalString("parentFolderId"),
                    name = folder.optString("name").ifBlank { "文件夹" },
                    scopeJson = folder.optJSONObject("scope")?.toString() ?: fallbackScopeJson,
                )
        }
        StarMapLibraryList(entries = targets, folders = libraryFolders)
    } catch (e: Exception) {
        StarMapLibraryList(error = e.message ?: "素材库解析失败")
    }
}

private fun parseReferences(
    json: String,
    fallback: StarMapLibraryTarget,
): List<StarMapLibraryReference> {
    val root = JSONArray(json)
    val references = ArrayList<StarMapLibraryReference>(root.length())
    for (index in 0 until root.length()) {
        val item = root.optJSONObject(index) ?: continue
        val starmapId = item.optString("starmapId")
        if (starmapId.isBlank()) continue
        val target = item.optJSONObject("target")
        references +=
            StarMapLibraryReference(
                targetKind = target?.optionalString("kind") ?: fallback.targetKind,
                assetId = target?.optionalString("assetId") ?: fallback.assetId,
                projectId = item.optionalString("projectId"),
                starmapId = starmapId,
                instanceId = item.optString("instanceId"),
            )
    }
    return references
}

private fun flattenFolders(folders: List<StarMapLibraryFolder>): List<FolderPickerOption> {
    val folderIds = folders.mapTo(mutableSetOf()) { it.folderId }
    val foldersByParent = folders.groupBy { folder -> folder.parentFolderId?.takeIf { it in folderIds } }
    val visited = mutableSetOf<String>()
    val options = mutableListOf<FolderPickerOption>()

    fun addLevel(parentFolderId: String?, depth: Int) {
        foldersByParent[parentFolderId].orEmpty().forEach { folder ->
            if (!visited.add(folder.folderId)) return@forEach
            options += FolderPickerOption(folder, depth)
            addLevel(folder.folderId, depth + 1)
        }
    }

    addLevel(null, 0)
    folders
        .filterNot { it.folderId in visited }
        .forEach { folder ->
            if (visited.add(folder.folderId)) {
                options += FolderPickerOption(folder, 0)
            }
        }
    return options
}

private fun JSONObject.optionalString(key: String): String? =
    if (!has(key) || isNull(key)) null else optString(key).takeIf { it.isNotBlank() }

/** 宽屏常驻左栏：全局库与当前作品库并列，与窄屏抽屉共享同一份 Core 数据。 */
@Composable
internal fun StarMapLibraryPane(
    repository: StarMapRepository,
    projectId: String?,
    onOpen: (StarMapLibraryTarget) -> Unit,
    onInsert: (StarMapLibraryTarget) -> Unit,
    onOpenReference: (starmapId: String, instanceId: String, kind: String, assetId: String) -> Unit,
) {
    StarMapLibraryPanel(
        repository = repository,
        projectId = projectId,
        modifier =
            Modifier
                .width(248.dp)
                .fillMaxHeight()
                .background(MaterialTheme.colorScheme.surfaceContainerLow),
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 12.dp),
        onOpen = onOpen,
        onInsert = onInsert,
        onOpenReference = onOpenReference,
    )
}
