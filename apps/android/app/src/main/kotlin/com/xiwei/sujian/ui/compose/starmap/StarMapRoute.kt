package com.xiwei.sujian.ui.compose.starmap

import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapGraphEdge
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapLayoutData
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
    var starMaps by remember { mutableStateOf<List<com.xiwei.sujian.model.StarMapMeta>>(emptyList()) }
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
                    is BridgeResult.Success -> result.data
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

    StarMapListContent(
        state = StarMapListUiState(starMaps = starMaps, isLoading = isLoading),
        onSelectStarmap = onSelectStarmap,
        onCreateClick = { showCreateDialog = true },
        modifier = modifier
    )

    if (showCreateDialog) {
        StarMapCreateDialog(
            coordinator = coordinator,
            onConfirm = { title, description ->
                coroutineScope.launch {
                    withContext(Dispatchers.IO) {
                        try {
                            val bridge = BridgeProvider.getStarmapBridge(context)
                            bridge.createStarmap(title, description)
                        } catch (_: Exception) { }
                    }
                    loadStarMaps()
                }
            },
            onDismiss = { showCreateDialog = false }
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

    val coordinator = LocalAnimatedTextEditorCoordinator.current
        ?: throw IllegalStateException(
            "StarMapCanvasScreen requires an AnimatedTextEditorCoordinator. " +
            "Ensure the host Activity provides one via CompositionLocalProvider."
        )

    suspend fun loadStarMap() {
        val data = withContext(Dispatchers.IO) {
            try {
                val bridge = BridgeProvider.getStarmapBridge(context)
                when (val result = bridge.getStarmapPhasedSnapshot(starmapId)) {
                    is BridgeResult.Success -> {
                        val snapshot = result.data
                        val graphData = StarMapData(
                            graph = com.xiwei.sujian.model.StarMapGraphData(
                                schemaVersion = 0,
                                id = snapshot.starmapId,
                                starmapId = snapshot.starmapId,
                                title = snapshot.title,
                                nodes = snapshot.nodes.map { node ->
                                    com.xiwei.sujian.model.StarMapGraphNode(
                                        id = node.id,
                                        title = node.title,
                                        kind = when (node.kind) {
                                            uniffi.writer_core.StarMapNodeKindDto.CHARACTER -> StarMapNodeKind.Character
                                            uniffi.writer_core.StarMapNodeKindDto.EVENT -> StarMapNodeKind.Event
                                            uniffi.writer_core.StarMapNodeKindDto.LOCATION -> StarMapNodeKind.Location
                                            uniffi.writer_core.StarMapNodeKindDto.ITEM -> StarMapNodeKind.Item
                                            uniffi.writer_core.StarMapNodeKindDto.CONCEPT -> StarMapNodeKind.Concept
                                            uniffi.writer_core.StarMapNodeKindDto.THEME -> StarMapNodeKind.Theme
                                            uniffi.writer_core.StarMapNodeKindDto.NOTE -> StarMapNodeKind.Note
                                            uniffi.writer_core.StarMapNodeKindDto.ORGANIZATION -> StarMapNodeKind.Organization
                                            uniffi.writer_core.StarMapNodeKindDto.TIMELINE -> StarMapNodeKind.Timeline
                                            uniffi.writer_core.StarMapNodeKindDto.PLOT -> StarMapNodeKind.Plot
                                            uniffi.writer_core.StarMapNodeKindDto.FORESHADOWING -> StarMapNodeKind.Foreshadowing
                                            uniffi.writer_core.StarMapNodeKindDto.CHAPTER -> StarMapNodeKind.Chapter
                                            uniffi.writer_core.StarMapNodeKindDto.CUSTOM -> StarMapNodeKind.Custom
                                        },
                                        createdAt = node.createdAt.toLong(),
                                        updatedAt = node.updatedAt.toLong()
                                    )
                                },
                                edges = snapshot.edges.map { edge ->
                                    com.xiwei.sujian.model.StarMapGraphEdge(
                                        id = edge.id,
                                        from = edge.from ?: "",
                                        to = edge.to ?: "",
                                        kind = when (edge.kind) {
                                            uniffi.writer_core.StarMapEdgeKindDto.CONTAINS -> com.xiwei.sujian.model.StarMapEdgeKind.Contains
                                            uniffi.writer_core.StarMapEdgeKindDto.REFERENCES -> com.xiwei.sujian.model.StarMapEdgeKind.References
                                            uniffi.writer_core.StarMapEdgeKindDto.APPEARS_IN -> com.xiwei.sujian.model.StarMapEdgeKind.AppearsIn
                                            uniffi.writer_core.StarMapEdgeKindDto.CAUSES -> com.xiwei.sujian.model.StarMapEdgeKind.Causes
                                            uniffi.writer_core.StarMapEdgeKindDto.RELATED_TO -> com.xiwei.sujian.model.StarMapEdgeKind.RelatedTo
                                            uniffi.writer_core.StarMapEdgeKindDto.LOCATED_AT -> com.xiwei.sujian.model.StarMapEdgeKind.LocatedAt
                                            uniffi.writer_core.StarMapEdgeKindDto.CHARACTER_RELATION -> com.xiwei.sujian.model.StarMapEdgeKind.CharacterRelation
                                            uniffi.writer_core.StarMapEdgeKindDto.TIMELINE -> com.xiwei.sujian.model.StarMapEdgeKind.Timeline
                                            uniffi.writer_core.StarMapEdgeKindDto.FORESHADOWS -> com.xiwei.sujian.model.StarMapEdgeKind.Foreshadows
                                            uniffi.writer_core.StarMapEdgeKindDto.RESOLVES -> com.xiwei.sujian.model.StarMapEdgeKind.Resolves
                                            uniffi.writer_core.StarMapEdgeKindDto.DEPENDS_ON -> com.xiwei.sujian.model.StarMapEdgeKind.DependsOn
                                            uniffi.writer_core.StarMapEdgeKindDto.CONFLICTS_WITH -> com.xiwei.sujian.model.StarMapEdgeKind.ConflictsWith
                                            uniffi.writer_core.StarMapEdgeKindDto.CUSTOM -> com.xiwei.sujian.model.StarMapEdgeKind.Custom
                                        },
                                        label = edge.label,
                                        createdAt = edge.createdAt.toLong(),
                                        updatedAt = edge.updatedAt.toLong()
                                    )
                                },
                                createdAt = 0L,
                                updatedAt = 0L
                            ),
                            layout = StarMapLayoutData(
                                kind = com.xiwei.sujian.model.StarMapLayoutKind.Freeform,
                                nodes = emptyList()
                            )
                        )
                        val viewport = snapshot.viewport?.let { vp ->
                            StarMapViewportData(
                                scale = vp.scale,
                                offsetX = vp.offsetX,
                                offsetY = vp.offsetY,
                                width = vp.width,
                                height = vp.height
                            )
                        } ?: StarMapViewportData()
                        val edgeRenders = when (val er = bridge.computeEdgeRenders(graphData)) {
                            is BridgeResult.Success -> er.data
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

    DisposableEffect(starmapId) {
        onDispose {
            val bridge = BridgeProvider.getStarmapBridge(context)
            val flushResult = bridge.flushStarmapStore(starmapId)
            if (flushResult is BridgeResult.Error) {
                DiagnosticsLogger.e("StarMapScreen", "flushStarmapStore failed on dispose: ${flushResult.message}")
                return@onDispose
            }
            val closeResult = bridge.closeStarmapStore(starmapId)
            if (closeResult is BridgeResult.Error) {
                DiagnosticsLogger.e("StarMapScreen", "closeStarmapStore failed on dispose: ${closeResult.message}")
            }
        }
    }

    StarMapEditorContent(
        starMapData = starMapData,
        isLoading = isLoading,
        editingNodeId = canvasEditingNodeId,
        onBack = onBack,
        onAddNodeClick = { showAddNodeDialog = true },
        onAddEdgeClick = { showAddEdgeDialog = true },
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
                                var success = false
                                withContext(Dispatchers.IO) {
                                    try {
                                        val bridge = BridgeProvider.getStarmapBridge(context)
                                        val result = bridge.updateStarmapNode(starmapId, geometry.nodeId, title = finalText.trim(), kind = null)
                                        success = result is BridgeResult.Success
                                        if (!success) {
                                            DiagnosticsLogger.e("StarMapScreen", "updateStarmapNode onCommit failed: ${(result as? BridgeResult.Error)?.message}")
                                        }
                                    } catch (e: Exception) {
                                        DiagnosticsLogger.e("StarMapScreen", "updateStarmapNode onCommit exception: ${e.message}")
                                    }
                                }
                                if (success) {
                                    loadStarMap()
                                }
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
        modifier = modifier
    )

    selectedNodeId?.let { nodeId ->
        val graphNode = starMapData?.graph?.nodes?.find { it.id == nodeId }
        if (graphNode != null) {
            NodeEditPanel(
                node = graphNode,
                coordinator = coordinator,
                onUpdate = { newTitle, newKind ->
                    coroutineScope.launch {
                        var success = false
                        withContext(Dispatchers.IO) {
                            try {
                                val bridge = BridgeProvider.getStarmapBridge(context)
                                val result = bridge.updateStarmapNode(starmapId, nodeId, title = newTitle, kind = newKind)
                                success = result is BridgeResult.Success
                                if (!success) {
                                    DiagnosticsLogger.e("StarMapScreen", "updateStarmapNode failed: ${(result as? BridgeResult.Error)?.message}")
                                }
                            } catch (e: Exception) {
                                DiagnosticsLogger.e("StarMapScreen", "updateStarmapNode exception: ${e.message}")
                            }
                        }
                        if (success) {
                            selectedNodeId = null
                            loadStarMap()
                        }
                    }
                },
                onDelete = {
                    coroutineScope.launch {
                        var success = false
                        withContext(Dispatchers.IO) {
                            try {
                                val bridge = BridgeProvider.getStarmapBridge(context)
                                val result = bridge.deleteStarmapNode(starmapId, nodeId)
                                success = result is BridgeResult.Success
                                if (!success) {
                                    DiagnosticsLogger.e("StarMapScreen", "deleteStarmapNode failed: ${(result as? BridgeResult.Error)?.message}")
                                }
                            } catch (e: Exception) {
                                DiagnosticsLogger.e("StarMapScreen", "deleteStarmapNode exception: ${e.message}")
                            }
                        }
                        if (success) {
                            selectedNodeId = null
                            loadStarMap()
                        }
                    }
                },
                onDismiss = { selectedNodeId = null }
            )
        }
    }

    if (showAddNodeDialog) {
        StarMapAddNodeDialog(
            coordinator = coordinator,
            onConfirm = { title, kind ->
                coroutineScope.launch {
                    var success = false
                    withContext(Dispatchers.IO) {
                        try {
                            val bridge = BridgeProvider.getStarmapBridge(context)
                            val nodeId = java.util.UUID.randomUUID().toString()
                            val node = StarMapGraphNode(
                                id = nodeId,
                                title = title,
                                kind = kind
                            )
                            val result = bridge.addStarmapNode(starmapId, node)
                            success = result is BridgeResult.Success
                            if (!success) {
                                DiagnosticsLogger.e("StarMapScreen", "addStarmapNode failed: ${(result as? BridgeResult.Error)?.message}")
                            }
                        } catch (e: Exception) {
                            DiagnosticsLogger.e("StarMapScreen", "addStarmapNode exception: ${e.message}")
                        }
                    }
                    if (success) {
                        loadStarMap()
                    }
                }
            },
            onDismiss = { showAddNodeDialog = false }
        )
    }

    if (showAddEdgeDialog && starMapData != null) {
        StarMapAddEdgeDialog(
            nodes = starMapData!!.graph.nodes,
            onConfirm = { fromNodeId, toNodeId ->
                coroutineScope.launch {
                    var success = false
                    withContext(Dispatchers.IO) {
                        try {
                            val bridge = BridgeProvider.getStarmapBridge(context)
                            val result = bridge.addStarmapEdge(starmapId, fromNodeId, toNodeId)
                            success = result is BridgeResult.Success
                            if (!success) {
                                DiagnosticsLogger.e("StarMapScreen", "addStarmapEdge failed: ${(result as? BridgeResult.Error)?.message}")
                            }
                        } catch (e: Exception) {
                            DiagnosticsLogger.e("StarMapScreen", "addStarmapEdge exception: ${e.message}")
                        }
                    }
                    if (success) {
                        loadStarMap()
                    }
                }
            },
            onDismiss = { showAddEdgeDialog = false }
        )
    }
}
