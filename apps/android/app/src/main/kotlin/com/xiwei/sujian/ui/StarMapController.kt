package com.xiwei.sujian.ui

import android.widget.FrameLayout
import android.widget.Toast
import com.xiwei.sujian.R
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.StarMapBridge
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapMotionPolicyData
import com.xiwei.sujian.model.StarMapViewportData
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.Box
import androidx.compose.ui.unit.dp

/**
 * StarMapController — 星图控制器
 *
 * 管理星图的加载、渲染和节点操作。
 *
 * ## 架构定位
 * - MainActivity → StarMapController → StarMapBridge → AppServiceBridge → UniFFI → Rust Core
 *
 * ## 职责边界
 * - **做**：加载星图数据、处理节点拖拽、保存布局、新建节点
 * - **不做**：星图渲染（由 StarMapCanvasView 负责）
 *
 * ## 使用场景
 * - MainActivity 星图标签页的数据管理
 * - 节点拖拽后的布局保存
 * - 新建星图节点
 */
class StarMapController(
    private val activity: MainActivity,
    private val bridge: StarMapBridge,
    private val tabContainer: FrameLayout,
    private val canvasView: StarMapCanvasView
) {
    var starmapId: String = ""
    var currentData: StarMapData? = null

    fun initialize(existingStarmapId: String) {
        starmapId = existingStarmapId

        canvasView.onNodeDragListener = { nodeId, dx, dy ->
            currentData?.let { data ->
                val mutableNodes = data.layout.nodes.toMutableList()
                val idx = mutableNodes.indexOfFirst { it.nodeId == nodeId }
                if (idx != -1) {
                    val layout = mutableNodes[idx]
                    mutableNodes[idx] = layout.copy(x = layout.x + dx, y = layout.y + dy)
                    val newData = data.copy(layout = data.layout.copy(nodes = mutableNodes)).withCoreEdgeRenders()
                    currentData = newData
                    canvasView.setData(newData)
                }
            }
        }

        canvasView.onNodeHitTestListener = { graphX, graphY ->
            currentData?.let { data ->
                when (val result = bridge.hitTestStarmapNode(data, graphX, graphY)) {
                    is BridgeResult.Success -> result.data
                    else -> null
                }
            }
        }

        canvasView.onLayoutSavedListener = {
            saveLayout()
        }

        canvasView.onViewportChangedListener = { viewport ->
            currentData = currentData?.copy(viewport = viewport)
            saveViewport(viewport)
        }

        if (starmapId.isEmpty()) {
            CoroutineScope(Dispatchers.IO).launch {
                var starmaps = bridge.listStarmaps()
                if (starmaps is BridgeResult.Success && starmaps.data.isEmpty()) {
                    bridge.createStarmap(activity.getString(R.string.default_starmap_title), activity.getString(R.string.default_starmap_desc))
                    starmaps = bridge.listStarmaps()
                }
                if (starmaps is BridgeResult.Success && starmaps.data.isNotEmpty()) {
                    starmapId = starmaps.data[0].starmapId
                    loadGraph()
                }
            }
        } else {
            loadGraph()
        }
    }

    fun loadGraph() {
        if (starmapId.isEmpty()) return
        CoroutineScope(Dispatchers.IO).launch {
            val result = bridge.getStarmapGraph(starmapId)
            val viewport = when (val viewportResult = bridge.getStarmapViewport(starmapId)) {
                is BridgeResult.Success -> viewportResult.data
                else -> StarMapViewportData()
            }
            // 从 Core 获取动画策略
            val motionPolicy = when (val policyResult = bridge.getMotionPolicy()) {
                is BridgeResult.Success -> policyResult.data
                else -> StarMapMotionPolicyData()
            }
            withContext(Dispatchers.Main) {
                when (result) {
                    is BridgeResult.Success -> {
                        val data = result.data.copy(viewport = viewport).withCoreEdgeRenders()
                        currentData = data
                        canvasView.setMotionPolicy(motionPolicy)
                        canvasView.setViewport(data.viewport)
                        canvasView.setData(data)
                    }
                    is BridgeResult.Error -> {
                        Toast.makeText(activity, "Failed to load: ${result.message}", Toast.LENGTH_LONG).show()
                    }
                    else -> {}
                }
            }
        }
    }

    private fun saveLayout() {
        val data = canvasView.getData() ?: return
        CoroutineScope(Dispatchers.IO).launch {
            bridge.saveStarmapLayout(starmapId, data.layout)
        }
    }

    private fun saveViewport(viewport: StarMapViewportData) {
        if (starmapId.isEmpty()) return
        CoroutineScope(Dispatchers.IO).launch {
            bridge.saveStarmapViewport(starmapId, viewport)
        }
    }

    private fun StarMapData.withCoreEdgeRenders(): StarMapData {
        return when (val result = bridge.computeEdgeRenders(this)) {
            is BridgeResult.Success -> copy(edgeRenders = result.data)
            else -> this
        }
    }

    fun showNewNodeDialog() {
        if (starmapId.isEmpty()) return

        val composeView = androidx.compose.ui.platform.ComposeView(activity)
        var nodeTitle by mutableStateOf("")
        var selectedKindIndex by mutableStateOf(0)

        val kinds = listOf(
            activity.getString(R.string.node_kind_character),
            activity.getString(R.string.node_kind_location),
            activity.getString(R.string.node_kind_event),
            activity.getString(R.string.node_kind_item),
            activity.getString(R.string.node_kind_concept),
            activity.getString(R.string.node_kind_chapter),
            activity.getString(R.string.node_kind_other)
        )
        val kindMap = mapOf(
            activity.getString(R.string.node_kind_character) to com.xiwei.sujian.model.StarMapNodeKind.Character,
            activity.getString(R.string.node_kind_location) to com.xiwei.sujian.model.StarMapNodeKind.Location,
            activity.getString(R.string.node_kind_event) to com.xiwei.sujian.model.StarMapNodeKind.Event,
            activity.getString(R.string.node_kind_item) to com.xiwei.sujian.model.StarMapNodeKind.Item,
            activity.getString(R.string.node_kind_concept) to com.xiwei.sujian.model.StarMapNodeKind.Concept,
            activity.getString(R.string.node_kind_chapter) to com.xiwei.sujian.model.StarMapNodeKind.Chapter,
            activity.getString(R.string.node_kind_other) to com.xiwei.sujian.model.StarMapNodeKind.Custom
        )

        composeView.setContent {
            androidx.compose.foundation.layout.Column(
                modifier = androidx.compose.ui.Modifier.padding(24.dp)
            ) {
                com.xiwei.sujian.editor.v2.compose.AnimatedTextField(
                    targetId = "starmap-new-node-title",
                    value = nodeTitle,
                    onValueChange = { nodeTitle = it },
                    onCommit = { nodeTitle = it },
                    profile = com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile.CanvasLabel,
                    placeholder = { androidx.compose.material3.Text(activity.getString(R.string.hint_node_name)) },
                    coordinator = (activity as? com.xiwei.sujian.ui.MainActivity)?.textEditorCoordinator
                )
                androidx.compose.foundation.layout.Spacer(
                    modifier = androidx.compose.ui.Modifier.height(16.dp)
                )
                var expanded by mutableStateOf(false)
                Box {
                    androidx.compose.material3.OutlinedButton(
                        onClick = { expanded = true }
                    ) {
                        androidx.compose.material3.Text(kinds[selectedKindIndex])
                    }
                    androidx.compose.material3.DropdownMenu(
                        expanded = expanded,
                        onDismissRequest = { expanded = false }
                    ) {
                        kinds.forEachIndexed { index, kind ->
                            androidx.compose.material3.DropdownMenuItem(
                                text = { androidx.compose.material3.Text(kind) },
                                onClick = {
                                    selectedKindIndex = index
                                    expanded = false
                                }
                            )
                        }
                    }
                }
            }
        }

        androidx.appcompat.app.AlertDialog.Builder(activity)
            .setTitle(activity.getString(R.string.dialog_new_chapter_title))
            .setView(composeView)
            .setPositiveButton(activity.getString(R.string.action_ok)) { _, _ ->
                val title = nodeTitle.trim()
                if (title.isNotEmpty()) {
                    val kindStr = kinds[selectedKindIndex]
                    val kind = kindMap[kindStr] ?: com.xiwei.sujian.model.StarMapNodeKind.Custom
                    addNode(title, kind)
                }
            }
            .setNegativeButton(activity.getString(R.string.action_cancel), null)
            .show()
    }

    private fun addNode(title: String, kind: com.xiwei.sujian.model.StarMapNodeKind) {
        val node = com.xiwei.sujian.model.StarMapGraphNode(
            id = java.util.UUID.randomUUID().toString(),
            kind = kind,
            title = title
        )
        CoroutineScope(Dispatchers.IO).launch {
            val result = bridge.addStarmapNode(starmapId, node)
            withContext(Dispatchers.Main) {
                if (result is BridgeResult.Success) {
                    loadGraph()
                } else if (result is BridgeResult.Error) {
                    Toast.makeText(activity, activity.getString(R.string.error_create_node_failed, result.message), Toast.LENGTH_SHORT).show()
                }
            }
        }
    }
}
