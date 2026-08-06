package com.xiwei.sujian.data.starmap

import android.content.Context
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapEdgeRenderData
import com.xiwei.sujian.model.StarMapLayoutData
import com.xiwei.sujian.model.StarMapMeta
import com.xiwei.sujian.model.StarMapPhasedSnapshotResult
import com.xiwei.sujian.model.StarMapViewportData
import com.xiwei.sujian.data.BridgeResult

/**
 * 星图操作门面 — UI 层通过此门面访问星图功能。
 *
 * UI 层不应直接引用 BridgeProvider 或具体 Bridge 类（架构分层规则 #597），
 * 此门面封装了 BridgeProvider 调用，UI 层只引用 data.starmap 包。
 */
class StarMapFacade(context: Context) {
    private val repository = BridgeProvider.getStarmapBridge(context).repository

    fun listStarmaps(): List<StarMapMeta> {
        return when (val result = repository.listStarmaps()) {
            is BridgeResult.Success -> result.data
            else -> emptyList()
        }
    }

    fun createStarmap(title: String, desc: String): StarMapMeta? {
        return when (val result = repository.createStarmap(title, desc)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    fun getStarmapPhasedSnapshot(starmapId: String): StarMapPhasedSnapshotResult? {
        return when (val result = repository.getStarmapPhasedSnapshot(starmapId)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    fun advanceLoadPhase(starmapId: String, targetPhase: String, sinceRevision: ULong = 0u): StarMapPhasedSnapshotResult? {
        return when (val result = repository.advanceLoadPhase(starmapId, targetPhase, sinceRevision)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    fun computeEdgeRenders(data: StarMapData): List<StarMapEdgeRenderData> {
        return when (val result = repository.computeEdgeRenders(data)) {
            is BridgeResult.Success -> result.data
            else -> emptyList()
        }
    }

    fun saveStarmapLayout(starmapId: String, layout: StarMapLayoutData): BridgeResult<Boolean> {
        return repository.saveStarmapLayout(starmapId, layout)
    }

    fun saveStarmapViewport(starmapId: String, viewport: StarMapViewportData): BridgeResult<Boolean> {
        return repository.saveStarmapViewport(starmapId, viewport)
    }

    fun flushStarmapStore(starmapId: String): BridgeResult<Boolean> {
        return repository.flushStarmapStore(starmapId)
    }

    fun closeStarmapStore(starmapId: String): BridgeResult<Boolean> {
        return repository.closeStarmapStore(starmapId)
    }

    fun addStarmapNode(starmapId: String, node: com.xiwei.sujian.model.StarMapGraphNode): BridgeResult<com.xiwei.sujian.model.StarMapGraphNode> {
        return repository.addStarmapNode(starmapId, node)
    }

    fun updateStarmapNode(starmapId: String, nodeId: String, title: String? = null, kind: com.xiwei.sujian.model.StarMapNodeKind? = null): BridgeResult<com.xiwei.sujian.model.StarMapGraphNode> {
        return repository.updateStarmapNode(starmapId, nodeId, title, kind)
    }

    fun deleteStarmapNode(starmapId: String, nodeId: String): BridgeResult<Boolean> {
        return repository.deleteStarmapNode(starmapId, nodeId)
    }

    fun addStarmapEdge(starmapId: String, fromNodeId: String, toNodeId: String): BridgeResult<com.xiwei.sujian.model.StarMapGraphEdge> {
        return repository.addStarmapEdge(starmapId, fromNodeId, toNodeId)
    }
}
