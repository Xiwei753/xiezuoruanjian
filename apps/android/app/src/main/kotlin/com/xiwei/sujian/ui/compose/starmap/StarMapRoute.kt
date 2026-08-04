package com.xiwei.sujian.ui.compose.starmap

import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffold
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.layout.calculatePaneScaffoldDirective
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
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
import com.xiwei.sujian.R
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
import com.xiwei.sujian.ui.compose.navigation.predictiveBackStateFraction
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * 星图目的地：列表与编辑是同一个目的地内的窗格状态，由同一个 Material3
 * Adaptive 列表—详情 navigator 管理。
 *
 * - 手机/平板都强制单窗格（保持“列表或编辑全屏”的既有产品结构）；
 * - 系统返回手势按 BackEvent.progress 真实 seek 窗格过渡（拖动跟手、
 *   取消回原位、提交完成剩余过渡），顶栏返回调用同一 navigator；
 * - 列表层返回交给全局 NavDisplay（Works 常驻栈底，pop 带真实手势进度）。
 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun StarMapScreen(
    topBarState: com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState,
    modifier: Modifier = Modifier
) {
    // 星图列表—编辑窗格共用同一个 Material3 Adaptive 列表—详情 navigator：
    // 手机/平板都强制单窗格（保持“列表或编辑全屏”的既有产品结构），系统返回手势
    // 按 BackEvent.progress 真实 seek 窗格过渡；顶栏返回走同一 navigator。
    val directive = calculatePaneScaffoldDirective(currentWindowAdaptiveInfo())
        .copy(maxHorizontalPartitions = 1)
    val navigator = rememberListDetailPaneScaffoldNavigator<String>(
        scaffoldDirective = directive,
    )
    val coroutineScope = rememberCoroutineScope()
    val currentStarmapId = navigator.currentDestination?.contentKey

    // 编辑窗格内容在弹栈过渡期间保持上一次打开的星图：contentKey 在 pop 开始
    // 即变为 null，若直接读它，退出动画中编辑内容会瞬间消失（手势结束跳变）；
    // 这里只在向前导航时更新，弹出后由退出动画继续显示旧编辑内容。
    var editorStarmapId by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(currentStarmapId) {
        if (currentStarmapId != null) {
            editorStarmapId = currentStarmapId
        }
    }

    // 预测返回：手势拖动时按 BackEvent.progress seek 窗格过渡；取消回到原位；
    // 提交后播放剩余过渡（navigateBack 挂起至动画完成），动画结束后才复位编辑态
    // 顶栏（标题/返回/操作），无手势结束跳变。
    androidx.activity.compose.PredictiveBackHandler(enabled = navigator.canNavigateBack()) { progressEvents ->
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("starmap_editor", "start")
        try {
            progressEvents.collect { event ->
                if (event.progress != 0f) {
                    navigator.seekBack(
                        BackNavigationBehavior.PopUntilScaffoldValueChange,
                        predictiveBackStateFraction(event.progress),
                    )
                }
            }
            navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
            topBarState.clear()
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("starmap_editor")
        } catch (e: CancellationException) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("starmap_editor", "cancel")
            withContext(NonCancellable) {
                navigator.seekBack(BackNavigationBehavior.PopUntilScaffoldValueChange, 0f)
            }
            throw e
        }
    }

    ListDetailPaneScaffold(
        modifier = modifier,
        directive = navigator.scaffoldDirective,
        scaffoldState = navigator.scaffoldState,
        listPane = {
            AnimatedPane {
                StarMapListScreen(
                    onSelectStarmap = { starmapId ->
                        coroutineScope.launch {
                            navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, starmapId)
                        }
                    },
                )
            }
        },
        detailPane = {
            AnimatedPane {
                val starmapId = editorStarmapId
                if (starmapId != null) {
                    StarMapEditorScreen(
                        starmapId = starmapId,
                        topBarState = topBarState,
                        onBack = {
                            coroutineScope.launch {
                                navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
                                topBarState.clear()
                                com.xiwei.sujian.diagnostics.DiagnosticsEvents.workspaceBack("starmap_editor")
                            }
                        },
                    )
                }
            }
        },
    )
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
    topBarState: com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState,
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
                    editorState = editorState.copy(lastError = context.getString(R.string.starmap_op_failed, label, result.message), operationInProgress = false)
                    false
                }
                BridgeResult.NotLoaded -> {
                    editorState = editorState.copy(lastError = context.getString(R.string.starmap_op_failed_not_loaded, label), operationInProgress = false)
                    false
                }
            }
        } catch (e: Exception) {
            editorState = editorState.copy(lastError = context.getString(R.string.starmap_op_exception, label, e.message), operationInProgress = false)
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
                            editorState = editorState.copy(layoutSaveError = context.getString(R.string.starmap_layout_save_failed, result.message))
                        }
                        BridgeResult.NotLoaded -> {
                            editorState = editorState.copy(layoutSaveError = context.getString(R.string.starmap_layout_save_failed_not_loaded))
                        }
                    }
                } catch (e: Exception) {
                    editorState = editorState.copy(layoutSaveError = context.getString(R.string.starmap_layout_save_exception, e.message))
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
                            editorState = editorState.copy(viewportSaveError = context.getString(R.string.starmap_viewport_save_failed, result.message))
                        }
                        BridgeResult.NotLoaded -> {
                            editorState = editorState.copy(viewportSaveError = context.getString(R.string.starmap_viewport_save_failed_not_loaded))
                        }
                    }
                } catch (e: Exception) {
                    editorState = editorState.copy(viewportSaveError = context.getString(R.string.starmap_viewport_save_exception, e.message))
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
        topBarState = topBarState,
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
                                editorState = editorState.copy(layoutSaveError = context.getString(R.string.starmap_layout_save_failed, result.message))
                            }
                            BridgeResult.NotLoaded -> {
                                editorState = editorState.copy(layoutSaveError = context.getString(R.string.starmap_layout_save_failed_not_loaded))
                            }
                        }
                    } catch (e: Exception) {
                        editorState = editorState.copy(layoutSaveError = context.getString(R.string.starmap_layout_save_exception, e.message))
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
                                editorState = editorState.copy(viewportSaveError = context.getString(R.string.starmap_viewport_save_failed, result.message))
                            }
                            BridgeResult.NotLoaded -> {
                                editorState = editorState.copy(viewportSaveError = context.getString(R.string.starmap_viewport_save_failed_not_loaded))
                            }
                        }
                    } catch (e: Exception) {
                        editorState = editorState.copy(viewportSaveError = context.getString(R.string.starmap_viewport_save_exception, e.message))
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
                                val ok = executeOperation(context.getString(R.string.starmap_update_node)) {
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
                        val ok = executeOperation(context.getString(R.string.starmap_update_node)) {
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
                        val ok = executeOperation(context.getString(R.string.starmap_delete_node)) {
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
                    val ok = executeOperation(context.getString(R.string.starmap_add_node)) {
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
                    val ok = executeOperation(context.getString(R.string.starmap_add_edge)) {
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
