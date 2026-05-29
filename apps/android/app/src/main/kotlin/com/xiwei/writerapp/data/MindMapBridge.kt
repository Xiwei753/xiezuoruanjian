package com.xiwei.writerapp.data

class MindMapBridge(private val appService: AppServiceBridge) {
    fun getMindMapSnapshot(projectId: String): BridgeResult<String> = appService.getMindMapSnapshot(projectId)
}