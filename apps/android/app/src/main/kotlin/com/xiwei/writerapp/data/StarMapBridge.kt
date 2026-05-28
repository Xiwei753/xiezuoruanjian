package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.StarMapMeta
import com.xiwei.writerapp.model.StarMapData
import com.xiwei.writerapp.model.StarMapGraphNode
import com.xiwei.writerapp.model.StarMapLayoutData

class StarMapBridge(private val nativeBridge: NativeCoreBridge) {
    fun listStarmaps(): BridgeResult<List<StarMapMeta>> = nativeBridge.listStarmaps().toBridgeResult()
    fun createStarmap(title: String, desc: String): BridgeResult<StarMapMeta> = nativeBridge.createStarmap(title, desc).toBridgeResult()
    fun getStarmapGraph(starmapId: String): BridgeResult<StarMapData> = nativeBridge.getStarmapGraph(starmapId).toBridgeResult()
    fun addStarmapNode(starmapId: String, node: StarMapGraphNode): BridgeResult<StarMapGraphNode> =
        nativeBridge.addStarmapNode(starmapId, node).toBridgeResult()
    fun saveStarmapLayout(starmapId: String, layout: StarMapLayoutData): BridgeResult<Boolean> =
        nativeBridge.saveStarmapLayout(starmapId, layout).toBridgeResult()
}
