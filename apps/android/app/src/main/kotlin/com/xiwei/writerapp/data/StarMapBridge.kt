package com.xiwei.writerapp.data

import com.google.gson.GsonBuilder
import com.google.gson.reflect.TypeToken
import com.xiwei.writerapp.model.BridgeError
import com.xiwei.writerapp.model.BridgeErrorCode
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
    fun listStarmaps(): BridgeResult<List<StarMapMeta>> = parseResult(listStarMaps())

    fun createStarMap(title: String, desc: String): BridgeResult<String> = appService.createStarMap(title, desc)
    fun createStarmap(title: String, desc: String): BridgeResult<StarMapMeta> = parseResult(createStarMap(title, desc))

    fun getStarMapGraph(starmapId: String): BridgeResult<String> = appService.getStarMapGraph(starmapId)
    fun getStarmapGraph(starmapId: String): BridgeResult<StarMapData> = parseResult(getStarMapGraph(starmapId))

    fun addStarMapNode(starmapId: String, nodeJson: String): BridgeResult<String> = appService.addStarMapNode(starmapId, nodeJson)
    fun addStarmapNode(starmapId: String, node: StarMapGraphNode): BridgeResult<String> {
        return addStarMapNode(starmapId, gson.toJson(node))
    }

    fun saveStarMapLayout(starmapId: String, layoutJson: String): BridgeResult<Boolean> = appService.saveStarMapLayout(starmapId, layoutJson)
    fun saveStarmapLayout(starmapId: String, layout: StarMapLayoutData): BridgeResult<Boolean> {
        return saveStarMapLayout(starmapId, gson.toJson(layout))
    }

    private inline fun <reified T> parseResult(result: BridgeResult<String>): BridgeResult<T> {
        return when (result) {
            is BridgeResult.Success -> try {
                val type = object : TypeToken<T>() {}.type
                BridgeResult.Success(gson.fromJson<T>(result.data, type))
            } catch (e: Exception) {
                BridgeResult.Error(BridgeError(BridgeErrorCode.JsonError, e.message ?: "Invalid StarMap JSON"))
            }
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }
}
