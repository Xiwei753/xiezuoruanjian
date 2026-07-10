package com.xiwei.sujian.ui.compose.starmap

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapGraphEdge
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapLayoutData
import com.xiwei.sujian.model.StarMapLayoutNodeData
import com.xiwei.sujian.model.StarMapMeta
import com.xiwei.sujian.model.StarMapNodeKind
import com.xiwei.sujian.model.StarMapViewportData
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
fun StarMapScreen(
    modifier: Modifier = Modifier
) {
    var selectedStarmapId by remember { mutableStateOf<String?>(null) }

    if (selectedStarmapId != null) {
        StarMapEditorScreen(
            starmapId = selectedStarmapId!!,
            onBack = { selectedStarmapId = null },
            modifier = modifier
        )
    } else {
        StarMapListScreen(
            onSelectStarmap = { starmapId ->
                selectedStarmapId = starmapId
            },
            modifier = modifier
        )
    }
}

@Composable
private fun StarMapListScreen(
    onSelectStarmap: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val coroutineScope = rememberCoroutineScope()
    var starMaps by remember { mutableStateOf<List<StarMapMeta>>(emptyList()) }
    var isLoading by remember { mutableStateOf(true) }
    var showCreateDialog by remember { mutableStateOf(false) }

    suspend fun loadStarMaps() {
        val maps = withContext(Dispatchers.IO) {
            try {
                val bridge = BridgeProvider.getStarmapBridge(context)
                when (val result = bridge.listStarmaps()) {
                    is com.xiwei.sujian.data.BridgeResult.Success -> result.data
                    else -> emptyList()
                }
            } catch (_: Exception) {
                emptyList()
            }
        }
        starMaps = maps
        isLoading = false
    }

    LaunchedEffect(Unit) {
        loadStarMaps()
    }

    Box(modifier = modifier.fillMaxSize()) {
        if (isLoading) {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("加载中...", style = MaterialTheme.typography.bodyLarge)
            }
        } else if (starMaps.isEmpty()) {
            Column(
                modifier = Modifier.fillMaxSize().padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text("暂无星图", style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(8.dp))
                Text("点击右下角按钮创建新星图", style = MaterialTheme.typography.bodyMedium)
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                modifier = Modifier.fillMaxSize()
            ) {
                items(starMaps, key = { it.starmapId }) { meta ->
                    Card(
                        onClick = { onSelectStarmap(meta.starmapId) },
                        modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow)
                    ) {
                        Column(modifier = Modifier.padding(16.dp)) {
                            Text(meta.title, style = MaterialTheme.typography.titleMedium)
                            if (meta.description.isNotBlank()) {
                                Text(meta.description, style = MaterialTheme.typography.bodySmall)
                            }
                            Text("${meta.nodeCount} 节点 · ${meta.edgeCount} 连线", style = MaterialTheme.typography.bodySmall)
                        }
                    }
                }
            }
        }

        FloatingActionButton(
            onClick = { showCreateDialog = true },
            modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp)
        ) {
            Icon(Icons.Default.Add, contentDescription = "新建星图")
        }
    }

    if (showCreateDialog) {
        var title by remember { mutableStateOf("") }
        var description by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { showCreateDialog = false },
            title = { Text("新建星图") },
            text = {
                Column {
                    OutlinedTextField(
                        value = title,
                        onValueChange = { title = it },
                        label = { Text("标题") },
                        singleLine = true
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    OutlinedTextField(
                        value = description,
                        onValueChange = { description = it },
                        label = { Text("描述（可选）") },
                        maxLines = 3
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    if (title.isNotBlank()) {
                        val t = title.trim()
                        val d = description.trim()
                        coroutineScope.launch {
                            withContext(Dispatchers.IO) {
                                try {
                                    val bridge = BridgeProvider.getStarmapBridge(context)
                                    bridge.createStarmap(t, d)
                                } catch (_: Exception) { }
                            }
                            loadStarMaps()
                        }
                    }
                    showCreateDialog = false
                }) { Text("创建") }
            },
            dismissButton = {
                TextButton(onClick = { showCreateDialog = false }) { Text("取消") }
            }
        )
    }
}

@Composable
private fun StarMapEditorScreen(
    starmapId: String,
    onBack: () -> Unit,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val coroutineScope = rememberCoroutineScope()
    var starMapData by remember { mutableStateOf<StarMapData?>(null) }
    var isLoading by remember { mutableStateOf(true) }
    var showAddNodeDialog by remember { mutableStateOf(false) }
    var showAddEdgeDialog by remember { mutableStateOf(false) }
    var selectedNodeId by remember { mutableStateOf<String?>(null) }
    var viewportSaveJob by remember { mutableStateOf<Job?>(null) }

    suspend fun loadStarMap() {
        val data = withContext(Dispatchers.IO) {
            try {
                val bridge = BridgeProvider.getStarmapBridge(context)
                when (val result = bridge.getStarmapGraph(starmapId)) {
                    is com.xiwei.sujian.data.BridgeResult.Success -> {
                        val graphData = result.data
                        val viewport = when (val vp = bridge.getStarmapViewport(starmapId)) {
                            is com.xiwei.sujian.data.BridgeResult.Success -> vp.data
                            else -> StarMapViewportData()
                        }
                        val edgeRenders = when (val er = bridge.computeEdgeRenders(graphData)) {
                            is com.xiwei.sujian.data.BridgeResult.Success -> er.data
                            else -> emptyList()
                        }
                        graphData.copy(edgeRenders = edgeRenders, viewport = viewport)
                    }
                    else -> null
                }
            } catch (_: Exception) { null }
        }
        starMapData = data
        isLoading = false
    }

    LaunchedEffect(starmapId) {
        loadStarMap()
    }

    Column(modifier = modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            IconButton(onClick = onBack) {
                Icon(Icons.Default.ArrowBack, contentDescription = "返回")
            }
            Text(starMapData?.graph?.title ?: "星图", style = MaterialTheme.typography.titleMedium)
            Spacer(modifier = Modifier.weight(1f))
            Text(
                "${starMapData?.graph?.nodes?.size ?: 0} 节点 · ${starMapData?.graph?.edges?.size ?: 0} 连线",
                style = MaterialTheme.typography.bodySmall
            )
            Spacer(modifier = Modifier.width(8.dp))
            IconButton(onClick = { showAddEdgeDialog = true }) {
                Icon(Icons.Default.Add, contentDescription = "添加连线")
            }
            IconButton(onClick = { showAddNodeDialog = true }) {
                Icon(Icons.Default.Add, contentDescription = "添加节点")
            }
        }

        if (isLoading) {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("加载中...", style = MaterialTheme.typography.bodyLarge)
            }
        } else if (starMapData != null) {
            StarMapCanvas(
                data = starMapData!!,
                onNodeDrag = { nodeId, x, y ->
                    coroutineScope.launch {
                        withContext(Dispatchers.IO) {
                            try {
                                val bridge = BridgeProvider.getStarmapBridge(context)
                                val updatedNodes = starMapData!!.layout.nodes.map {
                                    if (it.nodeId == nodeId) it.copy(x = x, y = y) else it
                                }
                                bridge.saveStarmapLayout(
                                    starmapId,
                                    starMapData!!.layout.copy(nodes = updatedNodes)
                                )
                            } catch (_: Exception) { }
                        }
                    }
                },
                onViewportChange = { viewport ->
                    viewportSaveJob?.cancel()
                    viewportSaveJob = coroutineScope.launch {
                        delay(500)
                        withContext(Dispatchers.IO) {
                            try {
                                val bridge = BridgeProvider.getStarmapBridge(context)
                                bridge.saveStarmapViewport(starmapId, viewport)
                            } catch (_: Exception) { }
                        }
                    }
                },
                onNodeTap = { nodeId ->
                    selectedNodeId = nodeId
                },
                modifier = Modifier.weight(1f).fillMaxWidth()
            )
        } else {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("加载失败", style = MaterialTheme.typography.bodyLarge)
            }
        }
    }

    selectedNodeId?.let { nodeId ->
        val graphNode = starMapData?.graph?.nodes?.find { it.id == nodeId }
        if (graphNode != null) {
            NodeEditPanel(
                node = graphNode,
                onUpdate = { newTitle, newKind ->
                    coroutineScope.launch {
                        withContext(Dispatchers.IO) {
                            try {
                                val bridge = BridgeProvider.getStarmapBridge(context)
                                bridge.updateStarmapNode(starmapId, nodeId, title = newTitle, kind = newKind)
                            } catch (_: Exception) { }
                        }
                        selectedNodeId = null
                        loadStarMap()
                    }
                },
                onDelete = {
                    coroutineScope.launch {
                        withContext(Dispatchers.IO) {
                            try {
                                val bridge = BridgeProvider.getStarmapBridge(context)
                                bridge.deleteStarmapNode(starmapId, nodeId)
                            } catch (_: Exception) { }
                        }
                        selectedNodeId = null
                        loadStarMap()
                    }
                },
                onDismiss = { selectedNodeId = null }
            )
        }
    }

    if (showAddNodeDialog) {
        var nodeTitle by remember { mutableStateOf("") }
        var nodeKind by remember { mutableStateOf(StarMapNodeKind.Note) }
        AlertDialog(
            onDismissRequest = { showAddNodeDialog = false },
            title = { Text("添加节点") },
            text = {
                Column {
                    OutlinedTextField(
                        value = nodeTitle,
                        onValueChange = { nodeTitle = it },
                        label = { Text("标题") },
                        singleLine = true
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        StarMapNodeKind.entries.take(6).forEach { kind ->
                            TextButton(
                                onClick = { nodeKind = kind },
                                modifier = Modifier
                            ) {
                                Text(
                                    kind.name,
                                    color = if (nodeKind == kind) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    if (nodeTitle.isNotBlank()) {
                        val t = nodeTitle.trim()
                        coroutineScope.launch {
                            withContext(Dispatchers.IO) {
                                try {
                                    val bridge = BridgeProvider.getStarmapBridge(context)
                                    val nodeId = java.util.UUID.randomUUID().toString()
                                    val node = StarMapGraphNode(
                                        id = nodeId,
                                        title = t,
                                        kind = nodeKind
                                    )
                                    bridge.addStarmapNode(starmapId, node)
                                } catch (_: Exception) { }
                            }
                            loadStarMap()
                        }
                    }
                    showAddNodeDialog = false
                }) { Text("添加") }
            },
            dismissButton = {
                TextButton(onClick = { showAddNodeDialog = false }) { Text("取消") }
            }
        )
    }

    if (showAddEdgeDialog && starMapData != null) {
        val nodes = starMapData!!.graph.nodes
        var fromNodeId by remember { mutableStateOf(nodes.firstOrNull()?.id ?: "") }
        var toNodeId by remember { mutableStateOf(nodes.drop(1).firstOrNull()?.id ?: "") }

        AlertDialog(
            onDismissRequest = { showAddEdgeDialog = false },
            title = { Text("添加连线") },
            text = {
                Column {
                    Text("起始节点", style = MaterialTheme.typography.bodySmall)
                    LazyColumn(modifier = Modifier.height(120.dp)) {
                        items(nodes) { node ->
                            TextButton(
                                onClick = { fromNodeId = node.id },
                                modifier = Modifier.fillMaxWidth()
                            ) {
                                Text(
                                    node.title,
                                    color = if (fromNodeId == node.id) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
                                )
                            }
                        }
                    }
                    Spacer(modifier = Modifier.height(8.dp))
                    Text("目标节点", style = MaterialTheme.typography.bodySmall)
                    LazyColumn(modifier = Modifier.height(120.dp)) {
                        items(nodes) { node ->
                            TextButton(
                                onClick = { toNodeId = node.id },
                                modifier = Modifier.fillMaxWidth()
                            ) {
                                Text(
                                    node.title,
                                    color = if (toNodeId == node.id) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        if (fromNodeId.isNotBlank() && toNodeId.isNotBlank() && fromNodeId != toNodeId) {
                            coroutineScope.launch {
                                withContext(Dispatchers.IO) {
                                    try {
                                        val bridge = BridgeProvider.getStarmapBridge(context)
                                        bridge.addStarmapEdge(starmapId, fromNodeId, toNodeId)
                                    } catch (_: Exception) { }
                                }
                                loadStarMap()
                            }
                        }
                        showAddEdgeDialog = false
                    },
                    enabled = fromNodeId.isNotBlank() && toNodeId.isNotBlank() && fromNodeId != toNodeId
                ) { Text("添加") }
            },
            dismissButton = {
                TextButton(onClick = { showAddEdgeDialog = false }) { Text("取消") }
            }
        )
    }
}

@Composable
private fun NodeEditPanel(
    node: StarMapGraphNode,
    onUpdate: (title: String, kind: StarMapNodeKind) -> Unit,
    onDelete: () -> Unit,
    onDismiss: () -> Unit
) {
    var editTitle by remember { mutableStateOf(node.title) }
    var editKind by remember { mutableStateOf(node.kind) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("编辑节点") },
        text = {
            Column {
                OutlinedTextField(
                    value = editTitle,
                    onValueChange = { editTitle = it },
                    label = { Text("标题") },
                    singleLine = true
                )
                Spacer(modifier = Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    StarMapNodeKind.entries.take(6).forEach { kind ->
                        TextButton(onClick = { editKind = kind }) {
                            Text(
                                kind.name,
                                color = if (editKind == kind) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
                            )
                        }
                    }
                }
                Spacer(modifier = Modifier.height(16.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.End
                ) {
                    IconButton(onClick = onDelete) {
                        Icon(
                            Icons.Default.Delete,
                            contentDescription = "删除节点",
                            tint = MaterialTheme.colorScheme.error
                        )
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                if (editTitle.isNotBlank()) {
                    onUpdate(editTitle.trim(), editKind)
                }
            }) { Text("保存") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("取消") }
        }
    )
}
