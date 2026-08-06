package com.xiwei.sujian.data.starmap

import android.content.Context
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapEdgeRenderData
import com.xiwei.sujian.model.StarMapMeta
import com.xiwei.sujian.model.StarMapPhasedSnapshotResult

/**
 * 星图 UseCase — UI 层通过此 UseCase 访问星图功能。
 *
 * 封装 BridgeProvider/BridgeResult 细节，UI 层不直接引用 Bridge 基础设施。
 * 操作结果使用 [StarMapOperationResult] 表达，不暴露 [BridgeResult]。
 */
class StarMapUseCase(private val context: Context) {

    private fun repository(): StarMapRepository =
        BridgeProvider.getStarmapBridge(context).repository

    fun listStarmaps(): List<StarMapMeta> {
        return when (val result = repository().listStarmaps()) {
            is BridgeResult.Success -> result.data
            else -> emptyList()
        }
    }

    fun createStarmap(title: String, description: String): StarMapMeta? {
        return when (val result = repository().createStarmap(title, description)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    fun getStarmapPhasedSnapshot(starmapId: String): StarMapPhasedSnapshotResult? {
        return when (val result = repository().getStarmapPhasedSnapshot(starmapId)) {
            is BridgeResult.Success -> result.data
            else -> null
        }
    }

    fun advanceLoadPhase(
        starmapId: String,
        targetPhase: String,
        sinceRevision: ULong = 0u,
    ): StarMapData? {
        return when (val result = repository().advanceLoadPhase(starmapId, targetPhase, sinceRevision)) {
            is BridgeResult.Success -> result.data.data
            else -> null
        }
    }

    fun computeEdgeRenders(data: StarMapData): List<StarMapEdgeRenderData> {
        return when (val result = repository().computeEdgeRenders(data)) {
            is BridgeResult.Success -> result.data
            else -> emptyList()
        }
    }

    fun saveStarmapLayout(starmapId: String, layout: com.xiwei.sujian.model.StarMapLayoutData): StarMapOperationResult {
        return when (val result = repository().saveStarmapLayout(starmapId, layout)) {
            is BridgeResult.Success -> StarMapOperationResult.Success
            is BridgeResult.Error -> StarMapOperationResult.Error(result.message)
            BridgeResult.NotLoaded -> StarMapOperationResult.NotLoaded
        }
    }

    fun saveStarmapViewport(starmapId: String, viewport: com.xiwei.sujian.model.StarMapViewportData): StarMapOperationResult {
        return when (val result = repository().saveStarmapViewport(starmapId, viewport)) {
            is BridgeResult.Success -> StarMapOperationResult.Success
            is BridgeResult.Error -> StarMapOperationResult.Error(result.message)
            BridgeResult.NotLoaded -> StarMapOperationResult.NotLoaded
        }
    }

    fun flushStarmapStore(starmapId: String): StarMapOperationResult {
        return when (val result = repository().flushStarmapStore(starmapId)) {
            is BridgeResult.Success -> StarMapOperationResult.Success
            is BridgeResult.Error -> StarMapOperationResult.Error(result.message)
            BridgeResult.NotLoaded -> StarMapOperationResult.NotLoaded
        }
    }

    fun closeStarmapStore(starmapId: String): StarMapOperationResult {
        return when (val result = repository().closeStarmapStore(starmapId)) {
            is BridgeResult.Success -> StarMapOperationResult.Success
            is BridgeResult.Error -> StarMapOperationResult.Error(result.message)
            BridgeResult.NotLoaded -> StarMapOperationResult.NotLoaded
        }
    }
}

/**
 * 星图操作结果 — 替代 BridgeResult 的 app 层类型。
 */
sealed class StarMapOperationResult {
    data object Success : StarMapOperationResult()
    data class Error(val message: String) : StarMapOperationResult()
    data object NotLoaded : StarMapOperationResult()
}
