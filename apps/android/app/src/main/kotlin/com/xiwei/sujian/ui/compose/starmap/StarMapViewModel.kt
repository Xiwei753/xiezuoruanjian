@file:Suppress("StringLiteralDuplication") // #597 技术债：协议字符串天然重复

package com.xiwei.sujian.ui.compose.starmap

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.starmap.StarMapRepository
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapNodeKind
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * 星图 ViewModel — 持有列表和编辑器状态，封装 Repository 访问。
 *
 * - 列表状态：星图列表加载、创建对话框
 * - 编辑器状态：当前星图数据、加载/保存/操作进度
 * - 旋转/分屏/折叠后状态不丢（ViewModel 跨配置变化存活）
 * - Repository 通过 BridgeProvider 注入，UI 层不直接引用 Context
 */
class StarMapViewModel private constructor(
    private val savedStateHandle: SavedStateHandle,
    internal val repository: StarMapRepository,
) : ViewModel() {

    // ── 列表状态 ──

    var listState by mutableStateOf(StarMapListUiState())
        internal set

    var showCreateDialog by mutableStateOf(false)
        internal set

    // ── 编辑器状态 ──

    var editorState by mutableStateOf(StarMapEditorUiState())
        internal set

    var showAddNodeDialog by mutableStateOf(false)
        internal set

    var showAddEdgeDialog by mutableStateOf(false)
        internal set

    private var viewportSaveJob: Job? = null

    // ── 列表操作 ──

    fun loadStarMaps() {
        viewModelScope.launch {
            val maps = withContext(Dispatchers.IO) {
                try {
                    when (val result = repository.listStarmaps()) {
                        is BridgeResult.Success -> result.data
                        else -> emptyList()
                    }
                } catch (_: Exception) {
                    emptyList()
                }
            }
            listState = listState.copy(starMaps = maps, isLoading = false)
        }
    }

    fun createStarmap(title: String, description: String) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try {
                    repository.createStarmap(title, description)
                } catch (_: Exception) { }
            }
            loadStarMaps()
        }
    }

    fun onShowCreateDialog(show: Boolean) {
        showCreateDialog = show
    }

    // ── 编辑器操作 ──

    fun loadStarMap(starmapId: String) {
        viewModelScope.launch {
            val data = withContext(Dispatchers.IO) {
                try {
                    when (val result = repository.getStarmapPhasedSnapshot(starmapId)) {
                        is BridgeResult.Success -> {
                            val snapshotResult = result.data
                            val graphData = snapshotResult.data
                            val edgeRenders = when (val er = repository.computeEdgeRenders(graphData)) {
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
    }

    @Suppress("CognitiveComplexMethod")
    // 分阶段加载涉及多级 phase 切换、BridgeResult 分支和错误回退，
    // 逻辑天然嵌套；拆分需引入 Phase 状态机，超出当前修复范围。
    fun advanceLoadPhase(starmapId: String) {
        val current = editorState.starMapData ?: return
        if (current.complete) return
        val nextPhase = when (current.loadPhase) {
            "CurrentViewportObjects" -> "PrefetchNearbyObjects"
            "PrefetchNearbyObjects" -> "BackgroundFullLoad"
            else -> null
        }
        if (nextPhase == null) return
        viewModelScope.launch {
            delay(100)
            val advanced = withContext(Dispatchers.IO) {
                try {
                    when (val result = repository.advanceLoadPhase(starmapId, nextPhase, current.packageRevision)) {
                        is BridgeResult.Success -> {
                            val graphData = result.data.data
                            val edgeRenders = when (val er = repository.computeEdgeRenders(graphData)) {
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
    }

    fun onNodeDrag(starmapId: String, nodeId: String, x: Float, y: Float) {
        viewModelScope.launch {
            val data = editorState.starMapData ?: return@launch
            val updatedNodes = data.layout.nodes.map {
                if (it.nodeId == nodeId) it.copy(x = x, y = y) else it
            }
            val updatedLayout = data.layout.copy(nodes = updatedNodes)
            editorState = editorState.copy(
                starMapData = data.copy(layout = updatedLayout),
                hasPendingLayoutSave = true, layoutSaveError = null,
            )
            withContext(Dispatchers.IO) {
                try {
                    when (val result = repository.saveStarmapLayout(starmapId, updatedLayout)) {
                        is BridgeResult.Success -> {
                            editorState = editorState.copy(hasPendingLayoutSave = false, layoutSaveError = null)
                        }
                        is BridgeResult.Error -> {
                            editorState = editorState.copy(layoutSaveError = "Layout save failed: ${result.message}")
                        }
                        BridgeResult.NotLoaded -> {
                            editorState = editorState.copy(layoutSaveError = "Layout save failed: not loaded")
                        }
                    }
                } catch (e: Exception) {
                    editorState = editorState.copy(layoutSaveError = "Layout save exception: ${e.message}")
                }
            }
        }
    }

    fun onViewportChange(starmapId: String, viewport: com.xiwei.sujian.model.StarMapViewportData) {
        viewportSaveJob?.cancel()
        viewportSaveJob = viewModelScope.launch {
            delay(500)
            editorState = editorState.copy(
                starMapData = editorState.starMapData?.copy(viewport = viewport),
                hasPendingViewportSave = true, viewportSaveError = null,
            )
            withContext(Dispatchers.IO) {
                try {
                    when (val result = repository.saveStarmapViewport(starmapId, viewport)) {
                        is BridgeResult.Success -> {
                            editorState = editorState.copy(hasPendingViewportSave = false, viewportSaveError = null)
                        }
                        is BridgeResult.Error -> {
                            editorState = editorState.copy(viewportSaveError = "Viewport save failed: ${result.message}")
                        }
                        BridgeResult.NotLoaded -> {
                            editorState = editorState.copy(viewportSaveError = "Viewport save failed: not loaded")
                        }
                    }
                } catch (e: Exception) {
                    editorState = editorState.copy(viewportSaveError = "Viewport save exception: ${e.message}")
                }
            }
        }
    }

    fun onNodeTap(nodeId: String) {
        if (editorState.editingNodeId == null) {
            editorState = editorState.copy(selectedNodeId = nodeId)
        }
    }

    fun clearNodeSelection() {
        editorState = editorState.copy(selectedNodeId = null)
    }

    fun startEditingNode(nodeId: String?) {
        editorState = editorState.copy(editingNodeId = nodeId)
    }

    fun stopEditingNode() {
        editorState = editorState.copy(editingNodeId = null)
    }

    /**
     * 执行星图操作 — 返回 Deferred<Boolean>，调用方可在协程中 await。
     * 操作结果通过 [editorState.lastError] 反映。
     */
    fun executeOperation(label: String, block: suspend () -> BridgeResult<*>) = viewModelScope.async {
        editorState = editorState.copy(operationInProgress = true)
        try {
            val result = withContext(Dispatchers.IO) { block() }
            when (result) {
                is BridgeResult.Success -> {
                    editorState = editorState.copy(lastError = null, operationInProgress = false)
                    true
                }
                is BridgeResult.Error -> {
                    editorState = editorState.copy(
                        lastError = "Operation '$label' failed: ${result.message}",
                        operationInProgress = false,
                    )
                    false
                }
                BridgeResult.NotLoaded -> {
                    editorState = editorState.copy(
                        lastError = "Operation '$label' failed: not loaded",
                        operationInProgress = false,
                    )
                    false
                }
            }
        } catch (e: Exception) {
            editorState = editorState.copy(
                lastError = "Operation '$label' exception: ${e.message}",
                operationInProgress = false,
            )
            false
        }
    }

    fun retryPendingSaves(starmapId: String) {
        viewModelScope.launch {
            retryPendingSavesInternal(starmapId)
        }
    }

    @Suppress("CognitiveComplexMethod")
    // 重试布局+视口两路保存，每路含 Success/Error/NotLoaded/Exception 四分支，
    // 逻辑对称但嵌套深；合并为通用重试函数需引入泛型保存器，超出当前修复范围。
    private suspend fun retryPendingSavesInternal(starmapId: String) {
        if (editorState.hasPendingLayoutSave && editorState.starMapData != null) {
            withContext(Dispatchers.IO) {
                try {
                    when (val result = repository.saveStarmapLayout(starmapId, editorState.starMapData!!.layout)) {
                        is BridgeResult.Success -> {
                            editorState = editorState.copy(hasPendingLayoutSave = false, layoutSaveError = null)
                        }
                        is BridgeResult.Error -> {
                            editorState = editorState.copy(layoutSaveError = "Layout save failed: ${result.message}")
                        }
                        BridgeResult.NotLoaded -> {
                            editorState = editorState.copy(layoutSaveError = "Layout save failed: not loaded")
                        }
                    }
                } catch (e: Exception) {
                    editorState = editorState.copy(layoutSaveError = "Layout save exception: ${e.message}")
                }
            }
        }
        if (editorState.hasPendingViewportSave && editorState.starMapData != null) {
            withContext(Dispatchers.IO) {
                try {
                    when (val result = repository.saveStarmapViewport(starmapId, editorState.starMapData!!.viewport)) {
                        is BridgeResult.Success -> {
                            editorState = editorState.copy(hasPendingViewportSave = false, viewportSaveError = null)
                        }
                        is BridgeResult.Error -> {
                            editorState = editorState.copy(viewportSaveError = "Viewport save failed: ${result.message}")
                        }
                        BridgeResult.NotLoaded -> {
                            editorState = editorState.copy(viewportSaveError = "Viewport save failed: not loaded")
                        }
                    }
                } catch (e: Exception) {
                    editorState = editorState.copy(viewportSaveError = "Viewport save exception: ${e.message}")
                }
            }
        }
    }

    fun flushAndCloseStarmapStore(starmapId: String) {
        val flushResult = repository.flushStarmapStore(starmapId)
        if (flushResult is BridgeResult.Error) {
            DiagnosticsLogger.e("StarMapViewModel", "flushStarmapStore failed on dispose: ${flushResult.message}")
            return
        }
        val closeResult = repository.closeStarmapStore(starmapId)
        if (closeResult is BridgeResult.Error) {
            DiagnosticsLogger.e("StarMapViewModel", "closeStarmapStore failed on dispose: ${closeResult.message}")
        }
    }

    fun onShowAddNodeDialog(show: Boolean) {
        showAddNodeDialog = show
    }

    fun onShowAddEdgeDialog(show: Boolean) {
        showAddEdgeDialog = show
    }

    fun resetEditorState() {
        editorState = StarMapEditorUiState()
    }

    class Factory(
        private val context: Context,
    ) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T {
            val repository = BridgeProvider.getStarmapBridge(context.applicationContext).repository
            return StarMapViewModel(
                savedStateHandle = SavedStateHandle(),
                repository = repository,
            ) as T
        }
    }
}
