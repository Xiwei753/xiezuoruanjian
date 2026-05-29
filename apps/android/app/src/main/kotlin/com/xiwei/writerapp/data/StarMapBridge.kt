package com.xiwei.writerapp.data

class StarMapBridge(private val appService: AppServiceBridge) {
    fun listStarMaps(): BridgeResult<String> = appService.listStarMaps()
    fun createStarMap(title: String, desc: String): BridgeResult<String> = appService.createStarMap(title, desc)
    fun getStarMapGraph(starmapId: String): BridgeResult<String> = appService.getStarMapGraph(starmapId)
    fun addStarMapNode(starmapId: String, nodeJson: String): BridgeResult<String> = appService.addStarMapNode(starmapId, nodeJson)
    fun saveStarMapLayout(starmapId: String, layoutJson: String): BridgeResult<Boolean> = appService.saveStarMapLayout(starmapId, layoutJson)
}