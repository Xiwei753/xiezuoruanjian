package com.xiwei.sujian.feature.starmap.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.width
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.starmap.data.StarMapRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** 素材库中的一个可插入目标；本体 ID 与画布实例 ID 不混用。 */
internal data class StarMapLibraryTarget(
    val kind: String,
    val assetId: String,
    val title: String,
)

internal data class StarMapLibraryList(
    val entries: List<StarMapLibraryTarget> = emptyList(),
    val error: String? = null,
)

/**
 * 星图素材库抽屉（#1037）。
 *
 * 全局库和当前作品库同时展示；点击条目只请求插入当前画布，
 * 不在 Android 端保存任何素材本体或归属状态。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun StarMapLibrarySheet(
    repository: StarMapRepository,
    projectId: String?,
    onDismiss: () -> Unit,
    onInsert: (StarMapLibraryTarget) -> Unit,
) {
    var reloadToken by remember { mutableIntStateOf(0) }
    var globalLibrary by remember { mutableStateOf(StarMapLibraryList()) }
    var projectLibrary by remember { mutableStateOf(StarMapLibraryList()) }
    var folderDialogScope by remember { mutableStateOf<String?>(null) }
    var folderName by remember { mutableStateOf("") }

    val projectScope = projectId?.takeIf { it.isNotBlank() }
        ?.let { "{\"kind\":\"project\",\"projectId\":${JSONObject.quote(it)}}" }

    LaunchedEffect(repository, projectScope, reloadToken) {
        globalLibrary = loadLibrary(repository, "{\"kind\":\"global\"}")
        projectLibrary = projectScope?.let { loadLibrary(repository, it) } ?: StarMapLibraryList()
    }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 20.dp, vertical = 12.dp),
        ) {
            Text(
                text = "素材库",
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                text = "点击插入当前画布；素材本体在多个作品和星图间共享。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 4.dp, bottom = 8.dp),
            )
            LazyColumn(contentPadding = PaddingValues(bottom = 24.dp)) {
                item {
                    LibrarySectionHeader(
                        title = "全局素材库",
                        onCreateFolder = {
                            folderDialogScope = "{\"kind\":\"global\"}"
                            folderName = ""
                        },
                    )
                }
                libraryItemRows(globalLibrary, onInsert)
                item {
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                    LibrarySectionHeader(
                        title = "当前作品素材库",
                        onCreateFolder = if (projectScope != null) {
                            {
                                folderDialogScope = projectScope
                                folderName = ""
                            }
                        } else null,
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
                libraryItemRows(projectLibrary, onInsert)
            }
        }
    }

    val scopeForDialog = folderDialogScope
    if (scopeForDialog != null) {
        AlertDialog(
            onDismissRequest = { folderDialogScope = null },
            title = { Text("新建素材文件夹") },
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
                            createFolder(repository, scopeForDialog, name) {
                                reloadToken++
                            }
                        }
                        folderDialogScope = null
                    },
                ) { Text("创建") }
            },
            dismissButton = {
                TextButton(onClick = { folderDialogScope = null }) { Text("取消") }
            },
        )
    }
}

private fun androidx.compose.foundation.lazy.LazyListScope.libraryItemRows(
    library: StarMapLibraryList,
    onInsert: (StarMapLibraryTarget) -> Unit,
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
    if (library.entries.isEmpty()) {
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
    items(library.entries, key = { it.kind + ":" + it.assetId }) { target ->
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(44.dp)
                .clickable { onInsert(target) }
                .padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = if (target.kind == "starmap") "图" else "点",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = target.title,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun LibrarySectionHeader(title: String, onCreateFolder: (() -> Unit)?) {
    Row(
        modifier = Modifier
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

private suspend fun loadLibrary(repository: StarMapRepository, scopeJson: String): StarMapLibraryList =
    withContext(Dispatchers.IO) {
        when (val result = repository.listStarmapLibraryJson(scopeJson)) {
            is BridgeResult.Success -> parseLibrary(result.data)
            is BridgeResult.Error -> StarMapLibraryList(error = result.message)
            BridgeResult.NotLoaded -> StarMapLibraryList(error = "Core 尚未加载")
        }
    }

private fun parseLibrary(json: String): StarMapLibraryList {
    return try {
        val root = JSONObject(json)
        val entries = root.optJSONArray("entries") ?: return StarMapLibraryList(error = "素材库数据缺少 entries")
        val targets = ArrayList<StarMapLibraryTarget>(entries.length())
        for (index in 0 until entries.length()) {
            val view = entries.optJSONObject(index) ?: continue
            val entry = view.optJSONObject("entry") ?: continue
            val target = entry.optJSONObject("target") ?: continue
            val kind = target.optString("kind")
            val assetId = target.optString("assetId")
            if (kind.isBlank() || assetId.isBlank()) continue
            targets += StarMapLibraryTarget(
                kind = kind,
                assetId = assetId,
                title = view.optString("title").ifBlank { assetId },
            )
        }
        StarMapLibraryList(entries = targets)
    } catch (e: Exception) {
        StarMapLibraryList(error = e.message ?: "素材库解析失败")
    }
}

private fun createFolder(
    repository: StarMapRepository,
    scopeJson: String,
    name: String,
    onCreated: () -> Unit,
) {
    repository.createStarmapLibraryFolder(scopeJson, null, name)
    onCreated()
}

/** 宽屏常驻左栏：全局库与当前作品库并列，与窄屏抽屉共享同一份 Core 数据。 */
@Composable
internal fun StarMapLibraryPane(
    repository: StarMapRepository,
    projectId: String?,
    onInsert: (StarMapLibraryTarget) -> Unit,
) {
    var reloadToken by remember { mutableIntStateOf(0) }
    var globalLibrary by remember { mutableStateOf(StarMapLibraryList()) }
    var projectLibrary by remember { mutableStateOf(StarMapLibraryList()) }
    val projectScope = projectId?.takeIf { it.isNotBlank() }
        ?.let { "{\"kind\":\"project\",\"projectId\":${JSONObject.quote(it)}}" }

    LaunchedEffect(repository, projectScope, reloadToken) {
        globalLibrary = loadLibrary(repository, "{\"kind\":\"global\"}")
        projectLibrary = projectScope?.let { loadLibrary(repository, it) } ?: StarMapLibraryList()
    }

    LazyColumn(
        modifier = Modifier
            .width(248.dp)
            .fillMaxHeight()
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .padding(horizontal = 12.dp),
        contentPadding = PaddingValues(vertical = 12.dp),
    ) {
        item {
            Text(
                text = "素材库",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(bottom = 4.dp),
            )
        }
        item { LibrarySectionHeader("全局素材库", null) }
        libraryItemRows(globalLibrary, onInsert)
        item {
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            LibrarySectionHeader("当前作品素材库", null)
            if (projectScope == null) {
                Text(
                    text = "当前入口没有作品归属。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        libraryItemRows(projectLibrary, onInsert)
    }
}
