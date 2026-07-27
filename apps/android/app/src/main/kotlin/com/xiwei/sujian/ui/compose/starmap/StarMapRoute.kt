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
    var lastOperationError by remember { mutableStateOf<String?>(null) }

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
                        val snapshotResult = result.data
                        val graphData = snapshotResult.data
                        val edgeRenders = when (val er = bridge.computeEdgeRenders(graphData)) {
                            is BridgeResult.Success -> er.data
                            else -> emptyList()
                        }
                        graphData.copy(edgeRenders = edgeRenders)
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
                                            lastOperationError = "更新节点失败: ${(result as? BridgeResult.Error)?.message}"
                                        }
                                    } catch (e: Exception) {
                                        lastOperationError = "更新节点异常: ${e.message}"
                                    }
                                }
                                if (success) {
                                    lastOperationError = null
                                    loadStarMap()
                                }
                            }
                        } else {
                            canvasEditingNodeId = null
                            coordinator.unregisterTarget(targetId)
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
                                    lastOperationError = "更新节点失败: ${(result as? BridgeResult.Error)?.message}"
                                }
                            } catch (e: Exception) {
                                lastOperationError = "更新节点异常: ${e.message}"
                            }
                        }
                        if (success) {
                            lastOperationError = null
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
                                    lastOperationError = "删除节点失败: ${(result as? BridgeResult.Error)?.message}"
                                }
                            } catch (e: Exception) {
                                lastOperationError = "删除节点异常: ${e.message}"
                            }
                        }
                        if (success) {
                            lastOperationError = null
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
                                lastOperationError = "添加节点失败: ${(result as? BridgeResult.Error)?.message}"
                            }
                        } catch (e: Exception) {
                            lastOperationError = "添加节点异常: ${e.message}"
                        }
                    }
                    if (success) {
                        lastOperationError = null
                        showAddNodeDialog = false
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
                                lastOperationError = "添加连线失败: ${(result as? BridgeResult.Error)?.message}"
                            }
                        } catch (e: Exception) {
                            lastOperationError = "添加连线异常: ${e.message}"
                        }
                    }
                    if (success) {
                        lastOperationError = null
                        showAddEdgeDialog = false
                        loadStarMap()
                    }
                }
            },
            onDismiss = { showAddEdgeDialog = false }
        )
    }
}
