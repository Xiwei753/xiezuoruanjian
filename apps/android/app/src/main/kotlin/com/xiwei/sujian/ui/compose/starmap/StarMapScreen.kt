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
import com.xiwei.sujian.designsystem.icon.SujianIcons
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import com.xiwei.sujian.editor.v2.compose.AnimatedTextField
import com.xiwei.sujian.editor.v2.compose.AnimatedTextArea
import com.xiwei.sujian.editor.v2.compose.AnimatedTextEditorSlot
import com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import androidx.compose.material3.Text
import com.xiwei.sujian.designsystem.component.SujianCard
import com.xiwei.sujian.designsystem.component.SujianDialog
import com.xiwei.sujian.designsystem.component.SujianFab
import com.xiwei.sujian.designsystem.component.SujianIconButton
import com.xiwei.sujian.designsystem.component.SujianTextButton
import com.xiwei.sujian.designsystem.layout.SujianScreenScaffold
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
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
    val dims = LocalSujianDimensions.current
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
    val dims = LocalSujianDimensions.current
    var starMaps by remember { mutableStateOf<List<StarMapMeta>>(emptyList()) }
    var isLoading by remember { mutableStateOf(true) }
    var showCreateDialog by remember { mutableStateOf(false) }

    val coordinator = LocalAnimatedTextEditorCoordinator.current
        ?: throw IllegalStateException(
            "StarMapListScreen requires an AnimatedTextEditorCoordinator. " +
            "Ensure the host Activity provides one via CompositionLocalProvider."
        )

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

    SujianScreenScaffold(
        title = stringResource(id = R.string.title_starmap),
        fabIcon = SujianIcons.Add,
        fabContentDescription = stringResource(id = R.string.starmap_create_new),
        onFabClick = { showCreateDialog = true },
        modifier = modifier,
    ) { paddingValues ->
        if (isLoading) {
            Box(modifier = Modifier.fillMaxSize().padding(paddingValues), contentAlignment = Alignment.Center) {
                Text(stringResource(id = R.string.loading), style = MaterialTheme.typography.bodyLarge)
            }
        } else if (starMaps.isEmpty()) {
            Column(
                modifier = Modifier.fillMaxSize().padding(paddingValues).padding(dims.space32),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text(stringResource(id = R.string.starmap_empty), style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(dims.space8))
                Text(stringResource(id = R.string.starmap_empty_hint), style = MaterialTheme.typography.bodyMedium)
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = dims.space16, vertical = dims.space8),
                modifier = Modifier.fillMaxSize().padding(paddingValues)
            ) {
                items(starMaps, key = { it.starmapId }) { meta ->
                    SujianCard(
                        onClick = { onSelectStarmap(meta.starmapId) },
                        modifier = Modifier.fillMaxWidth().padding(bottom = dims.space8),
                    ) {
                        Column(modifier = Modifier.padding(dims.space16)) {
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

        AnimatedTextEditorSlot(
            coordinator = coordinator
        )
    }

    if (showCreateDialog) {
        var title by remember { mutableStateOf("") }
        var description by remember { mutableStateOf("") }
        SujianDialog(
            onDismissRequest = {
                coordinator.cancelActiveEdit()
                showCreateDialog = false
            },
            title = stringResource(id = R.string.starmap_create_new),
            confirmText = stringResource(id = R.string.action_create),
            onConfirm = {
                coordinator.commitActiveEdit()
                val t = coordinator.lastCommittedText?.trim() ?: title.trim()
                if (t.isNotBlank()) {
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
            },
            dismissText = stringResource(id = R.string.action_cancel),
            onDismiss = {
                coordinator.cancelActiveEdit()
                showCreateDialog = false
            },
            body = {
                Column {
                    AnimatedTextField(
                        targetId = "starmap-title:new",
                        value = title,
                        onValueChange = { title = it },
                        onCommit = { },
                        label = { Text(stringResource(id = R.string.starmap_hint_title)) },
                        singleLine = true
                    )
                    Spacer(modifier = Modifier.height(dims.space8))
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
    val dims = LocalSujianDimensions.current
    var starMapData by remember { mutableStateOf<StarMapData?>(null) }
    var isLoading by remember { mutableStateOf(true) }
    var showAddNodeDialog by remember { mutableStateOf(false) }
    var showAddEdgeDialog by remember { mutableStateOf(false) }
    var selectedNodeId by remember { mutableStateOf<String?>(null) }
    var viewportSaveJob by remember { mutableStateOf<Job?>(null) }
    var canvasEditingNodeId by remember { mutableStateOf<String?>(null) }

    val coordinator = LocalAnimatedTextEditorCoordinator.current
        ?: throw IllegalStateException(
            "StarMapCanvasScreen requires an AnimatedTextEditorCoordinator. " +
            "Ensure the host Activity provides one via CompositionLocalProvider."
        )

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

    SujianScreenScaffold(
        title = starMapData?.graph?.title ?: stringResource(id = R.string.title_starmap),
        onNavigateBack = onBack,
        actions = {
            Text(
                stringResource(R.string.starmap_node_edge_count, starMapData?.graph?.nodes?.size ?: 0, starMapData?.graph?.edges?.size ?: 0),
                style = MaterialTheme.typography.bodySmall
            )
            Spacer(modifier = Modifier.width(dims.space8))
            SujianIconButton(
                onClick = { showAddEdgeDialog = true },
                icon = SujianIcons.Add,
                contentDescription = stringResource(id = R.string.starmap_add_edge),
            )
            SujianIconButton(
                onClick = { showAddNodeDialog = true },
                icon = SujianIcons.Add,
                contentDescription = stringResource(id = R.string.starmap_add_node),
            )
        },
        modifier = modifier,
    ) { paddingValues ->
        if (isLoading) {
            Box(modifier = Modifier.fillMaxSize().padding(paddingValues), contentAlignment = Alignment.Center) {
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
                        val target = EditableTextTarget(targetId = targetId)
                        target.updateProfile(TextEditorProfile.CanvasLabel)
                        target.updatePersistent(false)
                        target.updateText(graphNode.title)
                        target.onCommit = { finalText ->
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
                            }
                        target.onCancel = {
                                canvasEditingNodeId = null
                                coordinator.unregisterTarget(targetId)
                            }
                        target.onEditingStateChanged = { state ->
                                if (state == EditingState.IDLE || state == EditingState.RELEASED) {
                                    canvasEditingNodeId = null
                                }
                            }
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
                modifier = Modifier.fillMaxSize().padding(paddingValues)
            )
        } else {
            Box(modifier = Modifier.fillMaxSize().padding(paddingValues), contentAlignment = Alignment.Center) {
                Text(stringResource(id = R.string.starmap_load_failed), style = MaterialTheme.typography.bodyLarge)
            }
        }

        AnimatedTextEditorSlot(
            coordinator = coordinator
        )
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
        SujianDialog(
            onDismissRequest = {
                coordinator.cancelActiveEdit()
                showAddNodeDialog = false
            },
            title = stringResource(id = R.string.starmap_add_node),
            confirmText = stringResource(id = R.string.starmap_action_add),
            onConfirm = {
                coordinator.commitActiveEdit()
                val t = coordinator.lastCommittedText?.trim() ?: nodeTitle.trim()
                if (t.isNotBlank()) {
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
            },
            dismissText = stringResource(id = R.string.action_cancel),
            onDismiss = {
                coordinator.cancelActiveEdit()
                showAddNodeDialog = false
            },
            body = {
                Column {
                    AnimatedTextField(
                        targetId = "starmap-node-title:new",
                        value = nodeTitle,
                        onValueChange = { nodeTitle = it },
                        onCommit = { },
                        label = { Text(stringResource(id = R.string.starmap_hint_title)) },
                        singleLine = true
                    )
                    Spacer(modifier = Modifier.height(dims.space8))
                    Row(horizontalArrangement = Arrangement.spacedBy(dims.space4)) {
                        StarMapNodeKind.entries.take(6).forEach { kind ->
                            SujianTextButton(
                                text = kind.name,
                                onClick = { nodeKind = kind },
                            )
                        }
                    }
                }
            }
        )
    }

    if (showAddEdgeDialog && starMapData != null) {
        val nodes = starMapData!!.graph.nodes
        var fromNodeId by remember { mutableStateOf(nodes.firstOrNull()?.id ?: "") }
        var toNodeId by remember { mutableStateOf(nodes.drop(1).firstOrNull()?.id ?: "") }

        SujianDialog(
            onDismissRequest = {
                coordinator.cancelActiveEdit()
                showAddEdgeDialog = false
            },
            title = stringResource(id = R.string.starmap_add_edge),
            confirmText = stringResource(id = R.string.starmap_action_add),
            onConfirm = {
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
            dismissText = stringResource(id = R.string.action_cancel),
            onDismiss = {
                showAddEdgeDialog = false
            },
            body = {
                Column {
                    Text(stringResource(id = R.string.starmap_from_node), style = MaterialTheme.typography.bodySmall)
                    LazyColumn(modifier = Modifier.height(120.dp)) {
                        items(nodes) { node ->
                            SujianTextButton(
                                text = node.title,
                                onClick = { fromNodeId = node.id },
                                modifier = Modifier.fillMaxWidth()
                            )
                        }
                    }
                    Spacer(modifier = Modifier.height(dims.space8))
                    Text(stringResource(id = R.string.starmap_to_node), style = MaterialTheme.typography.bodySmall)
                    LazyColumn(modifier = Modifier.height(120.dp)) {
                        items(nodes) { node ->
                            SujianTextButton(
                                text = node.title,
                                onClick = { toNodeId = node.id },
                                modifier = Modifier.fillMaxWidth()
                            )
                        }
                    }
                }
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
    val dims = LocalSujianDimensions.current
    var editTitle by remember { mutableStateOf(node.title) }
    var editKind by remember { mutableStateOf(node.kind) }

    SujianDialog(
        onDismissRequest = {
            coordinator.cancelActiveEdit()
            onDismiss()
        },
        title = stringResource(id = R.string.starmap_edit_node),
        confirmText = stringResource(id = R.string.action_save),
        onConfirm = {
            coordinator.commitActiveEdit()
            val finalTitle = coordinator.lastCommittedText?.trim() ?: editTitle.trim()
            if (finalTitle.isNotBlank()) {
                onUpdate(finalTitle, editKind)
            }
        },
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = {
            coordinator.cancelActiveEdit()
            onDismiss()
        },
        body = {
            Column {
                AnimatedTextField(
                    targetId = "starmap-node-title:edit",
                    value = editTitle,
                    onValueChange = { editTitle = it },
                    onCommit = { },
                    label = { Text(stringResource(id = R.string.starmap_hint_title)) },
                    singleLine = true
                )
                Spacer(modifier = Modifier.height(dims.space8))
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    StarMapNodeKind.entries.take(6).forEach { kind ->
                        SujianTextButton(
                            text = kind.name,
                            onClick = { editKind = kind },
                        )
                    }
                }
                Spacer(modifier = Modifier.height(dims.space16))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.End
                ) {
                    SujianIconButton(
                        onClick = onDelete,
                        icon = SujianIcons.Delete,
                        contentDescription = stringResource(id = R.string.starmap_delete_node),
                    )
                }
            }
        }
    )
}
