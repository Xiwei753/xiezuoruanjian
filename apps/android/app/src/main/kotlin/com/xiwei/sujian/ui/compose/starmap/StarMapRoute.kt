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
import com.xiwei.sujian.data.starmap.StarMapRepository
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapNodeKind
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
                val repository = BridgeProvider.getStarmapBridge(context).repository
                when (val result = repository.listStarmaps()) {
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
                            val repository = BridgeProvider.getStarmapBridge(context).repository
                            repository.createStarmap(title, description)
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
    var editorState by remember { mutableStateOf(StarMapEditorUiState()) }
    var showAddNodeDialog by remember { mutableStateOf(false) }
    var showAddEdgeDialog by remember { mutableStateOf(false) }
    var viewportSaveJob by remember { mutableStateOf<Job?>(null) }

    val coordinator = LocalAnimatedTextEditorCoordinator.current
        ?: throw IllegalStateException(
            "StarMapCanvasScreen requires an AnimatedTextEditorCoordinator. " +
            "Ensure the host Activity provides one via CompositionLocalProvider."
        )

    fun repository(): StarMapRepository = BridgeProvider.getStarmapBridge(context).repository

    suspend fun loadStarMap() {
        val data = withContext(Dispatchers.IO) {
            try {
                val repo = repository()
                when (val result = repo.getStarmapPhasedSnapshot(starmapId)) {
                    is BridgeResult.Success -> {
                        val snapshotResult = result.data
                        val graphData = snapshotResult.data
                        val edgeRenders = when (val er = repo.computeEdgeRenders(graphData)) {
                            is BridgeResult.Success -> er.data
                            else -> emptyList()
                        }
                        graphData.copy(edgeRenders = edgeRenders)
                    }
                    else -> null
                }
            } catch (_: Exception) { null }
        }
        editorState = editorState.copy(starMapData = data, isLoading = false)
    }

    suspend fun executeOperation(label: String, block: suspend () -> BridgeResult<*>): Boolean {
        editorState = editorState.copy(operationInProgress = true)
        return try {
            val result = withContext(Dispatchers.IO) { block() }
            when (result) {
                is BridgeResult.Success -> {
                    editorState = editorState.copy(lastError = null, operationInProgress = false)
                    true
                }
                is BridgeResult.Error -> {
                    editorState = editorState.copy(lastError = "${label}失败: ${result.message}", operationInProgress = false)
                    false
                }
                BridgeResult.NotLoaded -> {
                    editorState = editorState.copy(lastError = "${label}失败: 未加载", operationInProgress = false)
                    false
                }
            }
        } catch (e: Exception) {
            editorState = editorState.copy(lastError = "${label}异常: ${e.message}", operationInProgress = false)
            false
        }
    }

    LaunchedEffect(starmapId) {
        loadStarMap()
    }

    LaunchedEffect(starmapId, editorState.starMapData?.loadPhase, editorState.starMapData?.complete) {
        val current = editorState.starMapData ?: return@LaunchedEffect
        if (current.complete) return@LaunchedEffect
        val nextPhase = when (current.loadPhase) {
            "CurrentViewportObjects" -> "PrefetchNearbyObjects"
            "PrefetchNearbyObjects" -> "BackgroundFullLoad"
            else -> null
        }
        if (nextPhase == null) return@LaunchedEffect
        delay(100)
        val advanced = withContext(Dispatchers.IO) {
            try {
                val repo = repository()
                when (val result = repo.advanceLoadPhase(starmapId, nextPhase, current.packageRevision)) {
                    is BridgeResult.Success -> {
                        val graphData = result.data.data
                        val edgeRenders = when (val er = repo.computeEdgeRenders(graphData)) {
                            is BridgeResult.Success -> er.data
                            else -> current.edgeRenders
                        }
                        graphData.copy(edgeRenders = edgeRenders)
                    }
                    else -> null
                }
            } catch (_: Exception) { null }
        }
        if (advanced != null) {
            editorState = editorState.copy(starMapData = advanced)
        }
    }

    suspend fun retryPendingSaves() {
        if (editorState.hasPendingLayoutSave && editorState.starMapData != null) {
            withContext(Dispatchers.IO) {
                try {
                    val repo = repository()
                    when (val result = repo.saveStarmapLayout(starmapId, editorState.starMapData!!.layout)) {
                        is BridgeResult.Success -> {
                            editorState = editorState.copy(hasPendingLayoutSave = false, layoutSaveError = null)
                        }
                        is BridgeResult.Error -> {
                            editorState = editorState.copy(layoutSaveError = "布局保存失败: ${result.message}")
                        }
                        BridgeResult.NotLoaded -> {
                            editorState = editorState.copy(layoutSaveError = "布局保存失败: 未加载")
                        }
                    }
                } catch (e: Exception) {
                    editorState = editorState.copy(layoutSaveError = "布局保存异常: ${e.message}")
                }
            }
        }
        if (editorState.hasPendingViewportSave && editorState.starMapData != null) {
            withContext(Dispatchers.IO) {
                try {
                    val repo = repository()
                    when (val result = repo.saveStarmapViewport(starmapId, editorState.starMapData!!.viewport)) {
                        is BridgeResult.Success -> {
                            editorState = editorState.copy(hasPendingViewportSave = false, viewportSaveError = null)
                        }
                        is BridgeResult.Error -> {
                            editorState = editorState.copy(viewportSaveError = "视口保存失败: ${result.message}")
                        }
                        BridgeResult.NotLoaded -> {
                            editorState = editorState.copy(viewportSaveError = "视口保存失败: 未加载")
                        }
                    }
                } catch (e: Exception) {
                    editorState = editorState.copy(viewportSaveError = "视口保存异常: ${e.message}")
                }
            }
        }
    }

    DisposableEffect(starmapId) {
        onDispose {
            val repo = repository()
            val flushResult = repo.flushStarmapStore(starmapId)
            if (flushResult is BridgeResult.Error) {
                DiagnosticsLogger.e("StarMapScreen", "flushStarmapStore failed on dispose: ${flushResult.message}")
                return@onDispose
            }
            val closeResult = repo.closeStarmapStore(starmapId)
            if (closeResult is BridgeResult.Error) {
                DiagnosticsLogger.e("StarMapScreen", "closeStarmapStore failed on dispose: ${closeResult.message}")
            }
        }
    }

    StarMapEditorContent(
        state = editorState,
        onBack = onBack,
        onAddNodeClick = { showAddNodeDialog = true },
        onAddEdgeClick = { showAddEdgeDialog = true },
        onNodeDrag = { nodeId, x, y ->
            coroutineScope.launch {
                val updatedNodes = editorState.starMapData!!.layout.nodes.map {
                    if (it.nodeId == nodeId) it.copy(x = x, y = y) else it
                }
                val updatedLayout = editorState.starMapData!!.layout.copy(nodes = updatedNodes)
                editorState = editorState.copy(
                    starMapData = editorState.starMapData!!.copy(layout = updatedLayout),
                    hasPendingLayoutSave = true, layoutSaveError = null
                )
                withContext(Dispatchers.IO) {
                    try {
                        val repo = repository()
                        when (val result = repo.saveStarmapLayout(starmapId, updatedLayout)) {
                            is BridgeResult.Success -> {
                                editorState = editorState.copy(hasPendingLayoutSave = false, layoutSaveError = null)
                            }
                            is BridgeResult.Error -> {
                                editorState = editorState.copy(layoutSaveError = "布局保存失败: ${result.message}")
                            }
                            BridgeResult.NotLoaded -> {
                                editorState = editorState.copy(layoutSaveError = "布局保存失败: 未加载")
                            }
                        }
                    } catch (e: Exception) {
                        editorState = editorState.copy(layoutSaveError = "布局保存异常: ${e.message}")
                    }
                }
            }
        },
        onViewportChange = { viewport ->
            viewportSaveJob?.cancel()
            viewportSaveJob = coroutineScope.launch {
                delay(500)
                editorState = editorState.copy(
                    starMapData = editorState.starMapData?.copy(viewport = viewport),
                    hasPendingViewportSave = true, viewportSaveError = null
                )
                withContext(Dispatchers.IO) {
                    try {
                        val repo = repository()
                        when (val result = repo.saveStarmapViewport(starmapId, viewport)) {
                            is BridgeResult.Success -> {
                                editorState = editorState.copy(hasPendingViewportSave = false, viewportSaveError = null)
                            }
                            is BridgeResult.Error -> {
                                editorState = editorState.copy(viewportSaveError = "视口保存失败: ${result.message}")
                            }
                            BridgeResult.NotLoaded -> {
                                editorState = editorState.copy(viewportSaveError = "视口保存失败: 未加载")
                            }
                        }
                    } catch (e: Exception) {
                        editorState = editorState.copy(viewportSaveError = "视口保存异常: ${e.message}")
                    }
                }
            }
        },
        onNodeTap = { nodeId ->
            if (editorState.editingNodeId == null) {
                editorState = editorState.copy(selectedNodeId = nodeId)
            }
        },
        onRetrySaves = {
            coroutineScope.launch { retryPendingSaves() }
        },
        onNodeDoubleTap = { geometry ->
            val graphNode = editorState.starMapData?.graph?.nodes?.find { it.id == geometry.nodeId }
            if (graphNode != null) {
                val targetId = "starmap-node-title:${starmapId}:${geometry.nodeId}"
                val target = EditableTextTarget(targetId = targetId)
                target.updateProfile(TextEditorProfile.CanvasLabel)
                target.updatePersistent(false)
                target.updateText(graphNode.title)
                    target.onCommit = { finalText ->
                        if (finalText.isNotBlank() && finalText.trim() != graphNode.title) {
                            coroutineScope.launch {
                                val ok = executeOperation("更新节点") {
                                    repository().updateStarmapNode(starmapId, geometry.nodeId, title = finalText.trim())
                                }
                                if (ok) {
                                    editorState = editorState.copy(editingNodeId = null)
                                    coordinator.unregisterTarget(targetId)
                                    loadStarMap()
                                }
                            }
                        } else {
                            editorState = editorState.copy(editingNodeId = null)
                            coordinator.unregisterTarget(targetId)
                        }
                    }
                target.onCancel = {
                        editorState = editorState.copy(editingNodeId = null)
                        coordinator.unregisterTarget(targetId)
                    }
                target.onEditingStateChanged = { state ->
                        if (state == EditingState.IDLE || state == EditingState.RELEASED) {
                            editorState = editorState.copy(editingNodeId = null)
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
                    editorState = editorState.copy(editingNodeId = geometry.nodeId)
                }
            }
        },
        modifier = modifier
    )

    editorState.selectedNodeId?.let { nodeId ->
        val graphNode = editorState.starMapData?.graph?.nodes?.find { it.id == nodeId }
        if (graphNode != null) {
            NodeEditPanel(
                node = graphNode,
                coordinator = coordinator,
                onUpdate = { newTitle, newKind ->
                    coroutineScope.launch {
                        val ok = executeOperation("更新节点") {
                            repository().updateStarmapNode(starmapId, nodeId, title = newTitle, kind = newKind)
                        }
                        if (ok) {
                            editorState = editorState.copy(selectedNodeId = null)
                            loadStarMap()
                        }
                    }
                },
                onDelete = {
                    coroutineScope.launch {
                        val ok = executeOperation("删除节点") {
                            repository().deleteStarmapNode(starmapId, nodeId)
                        }
                        if (ok) {
                            editorState = editorState.copy(selectedNodeId = null)
                            loadStarMap()
                        }
                    }
                },
                onDismiss = { editorState = editorState.copy(selectedNodeId = null) }
            )
        }
    }

    if (showAddNodeDialog) {
        StarMapAddNodeDialog(
            coordinator = coordinator,
            onConfirm = { title, kind ->
                coroutineScope.launch {
                    val ok = executeOperation("添加节点") {
                        val nodeId = java.util.UUID.randomUUID().toString()
                        val node = StarMapGraphNode(
                            id = nodeId,
                            title = title,
                            kind = kind
                        )
                        repository().addStarmapNode(starmapId, node)
                    }
                    if (ok) {
                        showAddNodeDialog = false
                        loadStarMap()
                    }
                }
            },
            onDismiss = { showAddNodeDialog = false }
        )
    }

    if (showAddEdgeDialog && editorState.starMapData != null) {
        StarMapAddEdgeDialog(
            nodes = editorState.starMapData!!.graph.nodes,
            onConfirm = { fromNodeId, toNodeId ->
                coroutineScope.launch {
                    val ok = executeOperation("添加连线") {
                        repository().addStarmapEdge(starmapId, fromNodeId, toNodeId)
                    }
                    if (ok) {
                        showAddEdgeDialog = false
                        loadStarMap()
                    }
                }
            },
            onDismiss = { showAddEdgeDialog = false }
        )
    }
}
