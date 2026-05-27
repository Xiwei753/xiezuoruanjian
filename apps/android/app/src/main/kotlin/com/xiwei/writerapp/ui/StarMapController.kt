package com.xiwei.writerapp.ui

import android.widget.FrameLayout
import android.widget.Toast
import com.xiwei.writerapp.data.NativeCoreBridge
import com.xiwei.writerapp.data.NativeResult
import com.xiwei.writerapp.model.StarMapData
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class StarMapController(
    private val activity: MainActivity,
    private val bridge: NativeCoreBridge,
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
                    val newData = data.copy(layout = data.layout.copy(nodes = mutableNodes))
                    currentData = newData
                    canvasView.setData(newData)
                }
            }
        }

        canvasView.onLayoutSavedListener = {
            saveLayout()
        }

        if (starmapId.isEmpty()) {
            CoroutineScope(Dispatchers.IO).launch {
                var starmaps = bridge.listStarmaps()
                if (starmaps is NativeResult.Success && starmaps.data.isEmpty()) {
                    bridge.createStarmap("作品宇宙", "自动生成的默认星图")
                    starmaps = bridge.listStarmaps()
                }
                if (starmaps is NativeResult.Success && starmaps.data.isNotEmpty()) {
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
            withContext(Dispatchers.Main) {
                when (result) {
                    is NativeResult.Success -> {
                        currentData = result.data
                        canvasView.setData(result.data)
                    }
                    is NativeResult.Error -> {
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
}
