package com.xiwei.writerapp.data

import com.google.gson.GsonBuilder
import com.xiwei.writerapp.model.MindMapNodeKind
import com.xiwei.writerapp.model.MindMapNodeKindDeserializer
import com.xiwei.writerapp.model.MindMapSnapshot

class MindMapBridge(private val appService: AppServiceBridge) {
    private val gson = GsonBuilder()
        .registerTypeAdapter(MindMapNodeKind::class.java, MindMapNodeKindDeserializer())
        .create()

    fun getMindMapSnapshot(projectId: String): BridgeResult<MindMapSnapshot> {
        return getMindMapSnapshotRaw(projectId).parseJsonResult(gson, "mindmap snapshot")
    }

    // 仅供 Bridge 内部封装/调试使用；上层主入口必须使用 typed MindMapSnapshot。
    internal fun getMindMapSnapshotRaw(projectId: String): BridgeResult<String> {
        return appService.getMindMapSnapshot(projectId)
    }
}
