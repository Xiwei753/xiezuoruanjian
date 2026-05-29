package com.xiwei.writerapp.data

import com.google.gson.GsonBuilder
import com.xiwei.writerapp.model.StarMapData
import com.xiwei.writerapp.model.StarMapEdgeKind
import com.xiwei.writerapp.model.StarMapEdgeKindDeserializer
import com.xiwei.writerapp.model.StarMapGraphNode
import com.xiwei.writerapp.model.StarMapLayoutData
import com.xiwei.writerapp.model.StarMapMeta
import com.xiwei.writerapp.model.StarMapNodeKind
import com.xiwei.writerapp.model.StarMapNodeKindDeserializer

class StarMapBridge(private val appService: AppServiceBridge) {
    private val gson = GsonBuilder()
        .registerTypeAdapter(StarMapNodeKind::class.java, StarMapNodeKindDeserializer())
        .registerTypeAdapter(StarMapEdgeKind::class.java, StarMapEdgeKindDeserializer())
        .create()

    fun listStarMaps(): BridgeResult<String> = appService.listStarMaps()
    fun listStarmaps(): BridgeResult<List<StarMapMeta>> = listStarMaps().parseJsonResult(gson, "starmap list")

    fun createStarMap(title: String, desc: String): BridgeResult<String> = appService.createStarMap(title, desc)
    fun createStarmap(title: String, desc: String): BridgeResult<StarMapMeta> = createStarMap(title, desc).parseJsonResult(gson, "starmap create")

    fun getStarMapGraph(starmapId: String): BridgeResult<String> = appService.getStarMapGraph(starmapId)
    fun getStarmapGraph(starmapId: String): BridgeResult<StarMapData> = getStarMapGraph(starmapId).parseJsonResult(gson, "starmap graph")

    fun addStarMapNode(starmapId: String, nodeJson: String): BridgeResult<String> = appService.addStarMapNode(starmapId, nodeJson)
    fun addStarmapNode(starmapId: String, node: StarMapGraphNode): BridgeResult<String> {
        return addStarMapNode(starmapId, gson.toJson(node))
    }

    fun saveStarMapLayout(starmapId: String, layoutJson: String): BridgeResult<Boolean> = appService.saveStarMapLayout(starmapId, layoutJson)
    fun saveStarmapLayout(starmapId: String, layout: StarMapLayoutData): BridgeResult<Boolean> {
        return saveStarMapLayout(starmapId, gson.toJson(layout))
    }
}
