package com.xiwei.sujian.data

import com.xiwei.sujian.data.starmap.StarMapRepository
import com.xiwei.sujian.data.starmap.StarMapSnapshotCache
import com.xiwei.sujian.data.starmap.toModel
import com.xiwei.sujian.data.starmap.toDto
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapEdgeKind
import com.xiwei.sujian.model.StarMapEdgeRenderData
import com.xiwei.sujian.model.StarMapGraphEdge
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapLayoutData
import com.xiwei.sujian.model.StarMapMeta
import com.xiwei.sujian.model.StarMapMotionPolicyData
import com.xiwei.sujian.model.StarMapNodeKind
import com.xiwei.sujian.model.StarMapViewportData
import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapEdgeKindDto
import uniffi.writer_core.StarMapEdgePatchInputDto
import uniffi.writer_core.StarMapEdgeRenderDto
import uniffi.writer_core.StarMapEmbedDto
import uniffi.writer_core.StarMapEmbedPatchInputDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapHyperlinkDto
import uniffi.writer_core.StarMapHyperlinkPatchInputDto
import uniffi.writer_core.StarMapLayoutDto
import uniffi.writer_core.StarMapLinkDto
import uniffi.writer_core.StarMapLinkPatchInputDto
import uniffi.writer_core.StarMapMetaDto
import uniffi.writer_core.StarMapMotionPolicyDto
import uniffi.writer_core.StarMapNodeDto
import uniffi.writer_core.StarMapNodePatchInputDto
import uniffi.writer_core.StarMapPhasedSnapshotDto
import uniffi.writer_core.StarMapReferenceDto
import uniffi.writer_core.StarMapViewportDto

/**
 * 星图 领域 Bridge。
 *
 * 从 AppServiceBridge 拆出，负责星图相关操作。
 * 通过 WriterAppServiceHolder 直接调用 Core 服务，不再依赖 AppServiceBridge。
 *
 * 提供两层 API：
 * - DTO 层（listStarMaps / getStarMapGraph 等）：返回原始 DTO，供 AppServiceBridge 门面委托
 * - Model 层（listStarmaps / getStarmapGraph 等）：返回 Android 端模型，供 UI 层使用
 */
class StarMapBridge internal constructor(private val holder: WriterAppServiceHolder) {
    companion object {
        private const val TAG = "StarMapBridge"
    }

    private val cache = StarMapSnapshotCache()
    private val repository = StarMapRepository(this, cache)

    // ── DTO 层 API（供 AppServiceBridge 门面委托） ──

    fun listStarMaps(): BridgeResult<List<StarMapMetaDto>> = holder.wrapResult {
        holder.service.listStarmaps()
    }

    fun getStarMapGraph(starmapId: String): BridgeResult<StarMapGraphDto> = holder.wrapResult {
        holder.service.getStarmapGraph(starmapId)
    }

    fun createStarMap(title: String, desc: String): BridgeResult<StarMapMetaDto> = holder.wrapResult {
        holder.service.createStarmap(title, desc)
    }

    fun addStarMapNode(starmapId: String, node: StarMapNodeDto, x: Float, y: Float): BridgeResult<StarMapNodeDto> = holder.wrapResult {
        holder.service.addStarmapNode(starmapId, node, x, y)
    }

    fun saveStarMapLayout(starmapId: String, layout: StarMapLayoutDto): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.saveStarmapLayout(starmapId, layout)
    }

    fun getStarMapViewport(starmapId: String): BridgeResult<StarMapViewportDto> = holder.wrapResult {
        holder.service.getStarmapViewport(starmapId)
    }

    fun saveStarMapViewport(starmapId: String, viewport: StarMapViewportDto): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.saveStarmapViewport(starmapId, viewport)
    }

    fun computeStarMapEdgeRenders(graph: StarMapGraphDto, layout: StarMapLayoutDto): BridgeResult<List<StarMapEdgeRenderDto>> = holder.wrapResult {
        holder.service.computeStarmapEdgeRenders(graph, layout)
    }

    fun hitTestStarMapNode(layout: StarMapLayoutDto, x: Float, y: Float): BridgeResult<String?> = holder.wrapResult {
        holder.service.hitTestStarmapNode(layout, x, y)
    }

    fun addStarmapEmbed(starmapId: String, embed: StarMapEmbedDto): BridgeResult<StarMapEmbedDto> = holder.wrapResult {
        holder.service.addStarmapEmbed(starmapId, embed)
    }

    fun updateStarmapEmbed(starmapId: String, instanceId: String, patch: StarMapEmbedPatchInputDto): BridgeResult<StarMapEmbedDto> = holder.wrapResult {
        holder.service.updateStarmapEmbed(starmapId, instanceId, patch)
    }

    fun deleteStarmapEmbed(starmapId: String, instanceId: String): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.deleteStarmapEmbed(starmapId, instanceId)
    }

    fun addStarmapLink(starmapId: String, link: StarMapLinkDto): BridgeResult<StarMapLinkDto> = holder.wrapResult {
        holder.service.addStarmapLink(starmapId, link)
    }

    fun updateStarmapLink(starmapId: String, linkId: String, patch: StarMapLinkPatchInputDto): BridgeResult<StarMapLinkDto> = holder.wrapResult {
        holder.service.updateStarmapLink(starmapId, linkId, patch)
    }

    fun deleteStarmapLink(starmapId: String, linkId: String): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.deleteStarmapLink(starmapId, linkId)
    }

    fun addStarmapHyperlink(starmapId: String, hl: StarMapHyperlinkDto): BridgeResult<StarMapHyperlinkDto> = holder.wrapResult {
        holder.service.addStarmapHyperlink(starmapId, hl)
    }

    fun updateStarmapHyperlink(starmapId: String, hyperlinkId: String, patch: StarMapHyperlinkPatchInputDto): BridgeResult<StarMapHyperlinkDto> = holder.wrapResult {
        holder.service.updateStarmapHyperlink(starmapId, hyperlinkId, patch)
    }

    fun deleteStarmapHyperlink(starmapId: String, hyperlinkId: String): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.deleteStarmapHyperlink(starmapId, hyperlinkId)
    }

    fun listStarmapHyperlinks(starmapId: String): BridgeResult<uniffi.writer_core.StarMapHyperlinkListWithDiagnosticsDto> = holder.wrapResult {
        holder.service.listStarmapHyperlinks(starmapId)
    }

    fun getStarmapPhasedSnapshot(starmapId: String): BridgeResult<StarMapPhasedSnapshotDto> = holder.wrapResult {
        holder.service.getStarmapPhasedSnapshot(starmapId)
    }

    fun findStarmapReferences(targetStarmapId: String): BridgeResult<List<StarMapReferenceDto>> = holder.wrapResult {
        holder.service.findStarmapReferences(targetStarmapId)
    }

    fun getStarMapMotionPolicy(): BridgeResult<StarMapMotionPolicyData> {
        return holder.wrapResult {
            val dto = holder.service.getStarmapMotionPolicy()
            StarMapMotionPolicyData(
                enabled = dto.enabled,
                idleWobbleEnabled = dto.idleWobbleEnabled,
                idleAmplitudeVp = dto.idleAmplitudeVp,
                idlePeriodMs = dto.idlePeriodMs.toInt(),
                dragLiftScale = dto.dragLiftScale,
                dragShadowBoost = dto.dragShadowBoost,
                settleDurationMs = dto.settleDurationMs.toInt(),
                reduceMotion = dto.reduceMotion
            )
        }
    }

    fun updateStarMapNode(starmapId: String, nodeId: String, patch: StarMapNodePatchInputDto): BridgeResult<StarMapNodeDto> = holder.wrapResult {
        holder.service.updateStarmapNode(starmapId, nodeId, patch)
    }

    fun deleteStarMapNode(starmapId: String, nodeId: String): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.deleteStarmapNode(starmapId, nodeId)
    }

    fun addStarMapEdge(starmapId: String, edge: StarMapEdgeDto): BridgeResult<StarMapEdgeDto> = holder.wrapResult {
        holder.service.addStarmapEdge(starmapId, edge)
    }

    fun deleteStarMapEdge(starmapId: String, edgeId: String): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.deleteStarmapEdge(starmapId, edgeId)
    }

    fun updateStarMapEdge(starmapId: String, edgeId: String, patch: StarMapEdgePatchInputDto): BridgeResult<StarMapEdgeDto> = holder.wrapResult {
        holder.service.updateStarmapEdge(starmapId, edgeId, patch)
    }

    // ── Model 层 API（供 UI 层使用，保留向后兼容） ──

    fun listStarmaps(): BridgeResult<List<StarMapMeta>> {
        return when (val result = listStarMaps()) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.map { it.toModel() })
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun createStarmap(title: String, desc: String): BridgeResult<StarMapMeta> {
        return when (val result = createStarMap(title, desc)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun getStarmapGraph(starmapId: String): BridgeResult<StarMapData> = repository.getStarmapGraph(starmapId)

    fun addStarmapNode(starmapId: String, node: StarMapGraphNode, x: Float = 0f, y: Float = 0f): BridgeResult<StarMapGraphNode> = repository.addStarmapNode(starmapId, node, x, y)

    fun updateStarmapNode(starmapId: String, nodeId: String, title: String? = null, kind: StarMapNodeKind? = null, tags: List<String>? = null): BridgeResult<StarMapGraphNode> = repository.updateStarmapNode(starmapId, nodeId, title, kind, tags)

    fun deleteStarmapNode(starmapId: String, nodeId: String): BridgeResult<Boolean> = repository.deleteStarmapNode(starmapId, nodeId)

    fun addStarmapEdge(starmapId: String, from: String, to: String, kind: StarMapEdgeKind = StarMapEdgeKind.RelatedTo, label: String? = null): BridgeResult<StarMapGraphEdge> = repository.addStarmapEdge(starmapId, from, to, kind, label)

    fun deleteStarmapEdge(starmapId: String, edgeId: String): BridgeResult<Boolean> = repository.deleteStarmapEdge(starmapId, edgeId)

    fun updateStarmapEdge(starmapId: String, edgeId: String, kind: StarMapEdgeKind? = null, label: String? = null): BridgeResult<StarMapGraphEdge> = repository.updateStarmapEdge(starmapId, edgeId, kind, label)

    fun saveStarmapLayout(starmapId: String, layout: StarMapLayoutData): BridgeResult<Boolean> = repository.saveStarmapLayout(starmapId, layout)

    fun getStarmapViewport(starmapId: String): BridgeResult<StarMapViewportData> {
        return when (val result = getStarMapViewport(starmapId)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun saveStarmapViewport(starmapId: String, viewport: StarMapViewportData): BridgeResult<Boolean> {
        return saveStarMapViewport(starmapId, viewport.toDto())
    }

    fun computeEdgeRenders(data: StarMapData): BridgeResult<List<StarMapEdgeRenderData>> = repository.computeEdgeRenders(data)

    fun hitTestStarmapNode(data: StarMapData, x: Float, y: Float): BridgeResult<String?> = repository.hitTestStarmapNode(data, x, y)

    fun getMotionPolicy(): BridgeResult<StarMapMotionPolicyData> = getStarMapMotionPolicy()

    fun flushStarmapStore(starmapId: String): BridgeResult<Boolean> {
        return try {
            holder.service.flushStarmapStore(starmapId)
            BridgeResult.Success(true)
        } catch (e: Exception) {
            BridgeResult.Error(ResultEnvelope.error("FLUSH_ERROR", "Failed to flush starmap store: ${e.message}"))
        }
    }

    fun closeStarmapStore(starmapId: String): BridgeResult<Boolean> {
        return try {
            holder.service.closeStarmapStore(starmapId)
            BridgeResult.Success(true)
        } catch (e: Exception) {
            BridgeResult.Error(ResultEnvelope.error("CLOSE_ERROR", "Failed to close starmap store: ${e.message}"))
        }
    }

    fun flushAllStarmapStores(): BridgeResult<Boolean> {
        return try {
            holder.service.flushAllStarmapStores()
            BridgeResult.Success(true)
        } catch (e: Exception) {
            BridgeResult.Error(ResultEnvelope.error("FLUSH_ALL_ERROR", "Failed to flush all starmap stores: ${e.message}"))
        }
    }
}
