package com.xiwei.sujian.ui

import android.widget.FrameLayout
import android.widget.Toast
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.StarMapBridge
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapMotionPolicyData
import com.xiwei.sujian.model.StarMapViewportData
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

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
                    bridge.createStarmap("作品宇宙", "自动生成的默认星图")
                    starmaps = bridge.listStarmaps()
                }
                if (starmaps is BridgeResult.Success && starmaps.data.isNotEmpty()) {
                    starmapId = starmaps.data[0].starmapId
                    activity.onStarmapIdInitialized(starmapId)
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
        
        val layout = android.widget.LinearLayout(activity)
        layout.orientation = android.widget.LinearLayout.VERTICAL
        layout.setPadding(48, 48, 48, 48)
        
        val titleInput = android.widget.EditText(activity)
        titleInput.hint = "节点名称"
        layout.addView(titleInput)
        
        val kindSpinner = android.widget.Spinner(activity)
        val kinds = arrayOf("角色", "地点", "事件", "物品", "概念", "章节", "其它")
        val kindMap = mapOf(
            "角色" to com.xiwei.sujian.model.StarMapNodeKind.Character,
            "地点" to com.xiwei.sujian.model.StarMapNodeKind.Location,
            "事件" to com.xiwei.sujian.model.StarMapNodeKind.Event,
            "物品" to com.xiwei.sujian.model.StarMapNodeKind.Item,
            "概念" to com.xiwei.sujian.model.StarMapNodeKind.Concept,
            "章节" to com.xiwei.sujian.model.StarMapNodeKind.Chapter,
            "其它" to com.xiwei.sujian.model.StarMapNodeKind.Custom
        )
        val adapter = android.widget.ArrayAdapter(activity, android.R.layout.simple_spinner_item, kinds)
        adapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item)
        kindSpinner.adapter = adapter
        layout.addView(kindSpinner)
        
        androidx.appcompat.app.AlertDialog.Builder(activity)
            .setTitle("新建节点")
            .setView(layout)
            .setPositiveButton("确定") { _, _ ->
                val title = titleInput.text.toString().trim()
                if (title.isNotEmpty()) {
                    val kindStr = kindSpinner.selectedItem.toString()
                    val kind = kindMap[kindStr] ?: com.xiwei.sujian.model.StarMapNodeKind.Custom
                    addNode(title, kind)
                }
            }
            .setNegativeButton("取消", null)
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
                    Toast.makeText(activity, "创建失败: ${result.message}", Toast.LENGTH_SHORT).show()
                }
            }
        }
    }
}
