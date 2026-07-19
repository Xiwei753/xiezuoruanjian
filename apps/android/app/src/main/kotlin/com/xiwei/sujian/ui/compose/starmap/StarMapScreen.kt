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
import com.xiwei.sujian.editor.v2.compose.AnimatedTextField
import com.xiwei.sujian.editor.v2.compose.AnimatedTextArea
import com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
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

    val coordinator = LocalAnimatedTextEditorCoordinator.current ?: remember {
        val bridge = BridgeProvider.getAppServiceBridge(context)
        AnimatedTextEditorCoordinator(context, bridge)
    }

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
                Text(stringResource(id = R.string.loading), style = MaterialTheme.typography.bodyLarge)
            }
        } else if (starMaps.isEmpty()) {
            Column(
                modifier = Modifier.fillMaxSize().padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text(stringResource(id = R.string.starmap_empty), style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(8.dp))
                Text(stringResource(id = R.string.starmap_empty_hint), style = MaterialTheme.typography.bodyMedium)
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
                            Text(stringResource(R.string.starmap_node_edge_count, meta.nodeCount, meta.edgeCount), style = MaterialTheme.typography.bodySmall)
                        }
                    }
                }
            }
        }

        FloatingActionButton(
            onClick = { showCreateDialog = true },
            modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp)
        ) {
            Icon(Icons.Default.Add, contentDescription = stringResource(id = R.string.starmap_create_new))
        }
    }

    if (showCreateDialog) {
        var title by remember { mutableStateOf("") }
        var description by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { showCreateDialog = false },
            title = { Text(stringResource(id = R.string.starmap_create_new)) },
            text = {
                Column {
                    AnimatedTextField(
                        targetId = "starmap-title:new",
                        value = title,
                        onValueChange = { title = it },
                        onCommit = { },
                        label = { Text(stringResource(id = R.string.starmap_hint_title)) },
                        singleLine = true
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    AnimatedTextArea(
                        targetId = "starmap-description:new",
                        value = description,
                        onValueChange = { description = it },
                        onCommit = { },
                        label = { Text(stringResource(id = R.string.starmap_hint_description)) },
                        minLines = 2,
                        maxLines = 3
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    coordinator.commitActiveEdit()
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
                }) { Text(stringResource(id = R.string.action_create)) }
            },
            dismissButton = {
                TextButton(onClick = {
                    coordinator.cancelActiveEdit()
                    showCreateDialog = false
                }) { Text(stringResource(id = R.string.action_cancel)) }
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
    var canvasEditingNodeId by remember { mutableStateOf<String?>(null) }

    val coordinator = LocalAnimatedTextEditorCoordinator.current ?: remember {
        val bridge = BridgeProvider.getAppServiceBridge(context)
        AnimatedTextEditorCoordinator(context, bridge)
    }

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
                Icon(Icons.Default.ArrowBack, contentDescription = stringResource(id = R.string.starmap_back))
            }
            Text(starMapData?.graph?.title ?: stringResource(id = R.string.title_starmap), style = MaterialTheme.typography.titleMedium)
            Spacer(modifier = Modifier.weight(1f))
            Text(
                stringResource(R.string.starmap_node_edge_count, starMapData?.graph?.nodes?.size ?: 0, starMapData?.graph?.edges?.size ?: 0),
                style = MaterialTheme.typography.bodySmall
            )
            Spacer(modifier = Modifier.width(8.dp))
            IconButton(onClick = { showAddEdgeDialog = true }) {
                Icon(Icons.Default.Add, contentDescription = stringResource(id = R.string.starmap_add_edge))
            }
            IconButton(onClick = { showAddNodeDialog = true }) {
                Icon(Icons.Default.Add, contentDescription = stringResource(id = R.string.starmap_add_node))
            }
        }

        if (isLoading) {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(stringResource(id = R.string.loading), style = MaterialTheme.typography.bodyLarge)
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
                    if (canvasEditingNodeId == null) {
                        selectedNodeId = nodeId
                    }
                },
                onNodeDoubleTap = { geometry ->
                    val graphNode = starMapData?.graph?.nodes?.find { it.id == geometry.nodeId }
                    if (graphNode != null) {
                        val targetId = "starmap-node-title:${starmapId}:${geometry.nodeId}"
                        val target = EditableTextTarget(
                            targetId = targetId,
                            profile = TextEditorProfile.CanvasLabel,
                            initialText = graphNode.title,
                            isPersistent = false,
                            onCommit = { finalText ->
                                if (finalText.isNotBlank() && finalText.trim() != graphNode.title) {
                                    coroutineScope.launch {
                                        withContext(Dispatchers.IO) {
                                            try {
                                                val bridge = BridgeProvider.getStarmapBridge(context)
                                                bridge.updateStarmapNode(starmapId, geometry.nodeId, title = finalText.trim(), kind = null)
                                            } catch (_: Exception) { }
                                        }
                                        loadStarMap()
                                    }
                                }
                                canvasEditingNodeId = null
                                coordinator.unregisterTarget(targetId)
                            },
                            onCancel = {
                                canvasEditingNodeId = null
                                coordinator.unregisterTarget(targetId)
                            },
                            onEditingStateChanged = { state ->
                                if (state == EditingState.IDLE || state == EditingState.RELEASED) {
                                    canvasEditingNodeId = null
                                }
                            }
                        )
                        coordinator.registerTarget(target)
                        coordinator.updateTargetGeometry(targetId, geometry.windowRect)
                        coordinator.updateTargetTransform(
                            targetId,
                            com.xiwei.sujian.editor.v2.coordinator.Transform2D(
                                translateX = 0f,
                                translateY = 0f,
                                scaleX = geometry.scale,
                                scaleY = geometry.scale
                            )
                        )
                        if (coordinator.beginEdit(targetId, graphNode.title.toByteArray(Charsets.UTF_8).size)) {
                            canvasEditingNodeId = geometry.nodeId
                        }
                    }
                },
                editingNodeId = canvasEditingNodeId,
                modifier = Modifier.weight(1f).fillMaxWidth()
            )
        } else {
            Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(stringResource(id = R.string.starmap_load_failed), style = MaterialTheme.typography.bodyLarge)
            }
        }
    }

    selectedNodeId?.let { nodeId ->
        val graphNode = starMapData?.graph?.nodes?.find { it.id == nodeId }
        if (graphNode != null) {
            NodeEditPanel(
                node = graphNode,
                coordinator = coordinator,
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
            title = { Text(stringResource(id = R.string.starmap_add_node)) },
            text = {
                Column {
                    AnimatedTextField(
                        targetId = "starmap-node-title:new",
                        value = nodeTitle,
                        onValueChange = { nodeTitle = it },
                        onCommit = { },
                        label = { Text(stringResource(id = R.string.starmap_hint_title)) },
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
                    coordinator.commitActiveEdit()
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
                }) { Text(stringResource(id = R.string.starmap_action_add)) }
            },
            dismissButton = {
                TextButton(onClick = {
                    coordinator.cancelActiveEdit()
                    showAddNodeDialog = false
                }) { Text(stringResource(id = R.string.action_cancel)) }
            }
        )
    }

    if (showAddEdgeDialog && starMapData != null) {
        val nodes = starMapData!!.graph.nodes
        var fromNodeId by remember { mutableStateOf(nodes.firstOrNull()?.id ?: "") }
        var toNodeId by remember { mutableStateOf(nodes.drop(1).firstOrNull()?.id ?: "") }

        AlertDialog(
            onDismissRequest = { showAddEdgeDialog = false },
            title = { Text(stringResource(id = R.string.starmap_add_edge)) },
            text = {
                Column {
                    Text(stringResource(id = R.string.starmap_from_node), style = MaterialTheme.typography.bodySmall)
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
                    Text(stringResource(id = R.string.starmap_to_node), style = MaterialTheme.typography.bodySmall)
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
                ) { Text(stringResource(id = R.string.starmap_action_add)) }
            },
            dismissButton = {
                TextButton(onClick = { showAddEdgeDialog = false }) { Text(stringResource(id = R.string.action_cancel)) }
            }
        )
    }
}

@Composable
private fun NodeEditPanel(
    node: StarMapGraphNode,
    coordinator: AnimatedTextEditorCoordinator,
    onUpdate: (title: String, kind: StarMapNodeKind) -> Unit,
    onDelete: () -> Unit,
    onDismiss: () -> Unit
) {
    var editTitle by remember { mutableStateOf(node.title) }
    var editKind by remember { mutableStateOf(node.kind) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(id = R.string.starmap_edit_node)) },
        text = {
            Column {
                AnimatedTextField(
                    targetId = "starmap-node-title:edit",
                    value = editTitle,
                    onValueChange = { editTitle = it },
                    onCommit = { },
                    label = { Text(stringResource(id = R.string.starmap_hint_title)) },
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
                            contentDescription = stringResource(id = R.string.starmap_delete_node),
                            tint = MaterialTheme.colorScheme.error
                        )
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                coordinator.commitActiveEdit()
                if (editTitle.isNotBlank()) {
                    onUpdate(editTitle.trim(), editKind)
                }
            }) { Text(stringResource(id = R.string.action_save)) }
        },
        dismissButton = {
            TextButton(onClick = {
                coordinator.cancelActiveEdit()
                onDismiss()
            }) { Text(stringResource(id = R.string.action_cancel)) }
        }
    )
}
