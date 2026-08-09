package com.xiwei.sujian.feature.starmap.data.interop
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.starmap.data.StarMapRepository
import com.xiwei.sujian.feature.starmap.data.StarMapSnapshotCache
import uniffi.writer_core.PhasedSnapshotRequestDto
import uniffi.writer_core.StarMapEdgeDto
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
 * 星图领域 Bridge — 平台边界委托与统一错误封装。
 *
 * 只负责 UniFFI 调用和 BridgeResult 包装，不做 DTO↔Model 转换。
 * Model 层 API 由 [StarMapRepository] 提供。
 */
class StarMapBridge internal constructor(private val holder: WriterAppServiceHolder) : StarMapBridgeOps {
    companion object {
        private const val TAG = "StarMapBridge"
    }

    private val cache = StarMapSnapshotCache()
    internal val repository = StarMapRepository(this, cache)

    override fun listStarMaps(): BridgeResult<List<StarMapMetaDto>> =
        holder.wrapResult {
            holder.service.listStarmaps()
        }

    @Suppress("OverridingDeprecatedMember")
    @Deprecated("Use getStarmapPhasedSnapshot for progressive loading. getStarMapGraph performs full graph load.")
    override fun getStarMapGraph(starmapId: String): BridgeResult<StarMapGraphDto> {
        return holder.wrapResult {
            holder.service.getStarmapGraph(starmapId)
        }
    }

    override fun createStarMap(
        title: String,
        desc: String,
    ): BridgeResult<StarMapMetaDto> =
        holder.wrapResult {
            holder.service.createStarmap(title, desc)
        }

    override fun addStarMapNode(
        starmapId: String,
        node: StarMapNodeDto,
        x: Float,
        y: Float,
    ): BridgeResult<StarMapNodeDto> =
        holder.wrapResult {
            holder.service.addStarmapNode(starmapId, node, x, y)
        }

    override fun saveStarMapLayout(
        starmapId: String,
        layout: StarMapLayoutDto,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveStarmapLayout(starmapId, layout)
        }

    override fun getStarMapViewport(starmapId: String): BridgeResult<StarMapViewportDto> =
        holder.wrapResult {
            holder.service.getStarmapViewport(starmapId)
        }

    override fun saveStarMapViewport(
        starmapId: String,
        viewport: StarMapViewportDto,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveStarmapViewport(starmapId, viewport)
        }

    override fun computeStarMapEdgeRenders(
        graph: StarMapGraphDto,
        layout: StarMapLayoutDto,
    ): BridgeResult<List<StarMapEdgeRenderDto>> =
        holder.wrapResult {
            holder.service.computeStarmapEdgeRenders(graph, layout)
        }

    override fun hitTestStarMapNode(
        layout: StarMapLayoutDto,
        x: Float,
        y: Float,
    ): BridgeResult<String?> =
        holder.wrapResult {
            holder.service.hitTestStarmapNode(layout, x, y)
        }

    override fun addStarmapEmbed(
        starmapId: String,
        embed: StarMapEmbedDto,
    ): BridgeResult<StarMapEmbedDto> =
        holder.wrapResult {
            holder.service.addStarmapEmbed(starmapId, embed)
        }

    override fun updateStarmapEmbed(
        starmapId: String,
        instanceId: String,
        patch: StarMapEmbedPatchInputDto,
    ): BridgeResult<StarMapEmbedDto> =
        holder.wrapResult {
            holder.service.updateStarmapEmbed(starmapId, instanceId, patch)
        }

    override fun deleteStarmapEmbed(
        starmapId: String,
        instanceId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.deleteStarmapEmbed(starmapId, instanceId)
        }

    override fun addStarmapLink(
        starmapId: String,
        link: StarMapLinkDto,
    ): BridgeResult<StarMapLinkDto> =
        holder.wrapResult {
            holder.service.addStarmapLink(starmapId, link)
        }

    override fun updateStarmapLink(
        starmapId: String,
        linkId: String,
        patch: StarMapLinkPatchInputDto,
    ): BridgeResult<StarMapLinkDto> =
        holder.wrapResult {
            holder.service.updateStarmapLink(starmapId, linkId, patch)
        }

    override fun deleteStarmapLink(
        starmapId: String,
        linkId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.deleteStarmapLink(starmapId, linkId)
        }

    override fun addStarmapHyperlink(
        starmapId: String,
        hl: StarMapHyperlinkDto,
    ): BridgeResult<StarMapHyperlinkDto> =
        holder.wrapResult {
            holder.service.addStarmapHyperlink(starmapId, hl)
        }

    override fun updateStarmapHyperlink(
        starmapId: String,
        hyperlinkId: String,
        patch: StarMapHyperlinkPatchInputDto,
    ): BridgeResult<StarMapHyperlinkDto> =
        holder.wrapResult {
            holder.service.updateStarmapHyperlink(starmapId, hyperlinkId, patch)
        }

    override fun deleteStarmapHyperlink(
        starmapId: String,
        hyperlinkId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.deleteStarmapHyperlink(starmapId, hyperlinkId)
        }

    override fun listStarmapHyperlinks(
        starmapId: String,
    ): BridgeResult<uniffi.writer_core.StarMapHyperlinkListWithDiagnosticsDto> =
        holder.wrapResult {
            holder.service.listStarmapHyperlinks(starmapId)
        }

    override fun getStarmapPhasedSnapshot(
        starmapId: String,
        request: PhasedSnapshotRequestDto,
    ): BridgeResult<StarMapPhasedSnapshotDto> =
        holder.wrapResult {
            holder.service.getStarmapPhasedSnapshot(starmapId, request)
        }

    override fun findStarmapReferences(targetStarmapId: String): BridgeResult<List<StarMapReferenceDto>> =
        holder.wrapResult {
            holder.service.findStarmapReferences(targetStarmapId)
        }

    override fun getStarMapMotionPolicy(): BridgeResult<StarMapMotionPolicyDto> =
        holder.wrapResult {
            holder.service.getStarmapMotionPolicy()
        }

    override fun updateStarMapNode(
        starmapId: String,
        nodeId: String,
        patch: StarMapNodePatchInputDto,
    ): BridgeResult<StarMapNodeDto> =
        holder.wrapResult {
            holder.service.updateStarmapNode(starmapId, nodeId, patch)
        }

    override fun deleteStarMapNode(
        starmapId: String,
        nodeId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.deleteStarmapNode(starmapId, nodeId)
        }

    override fun addStarMapEdge(
        starmapId: String,
        edge: StarMapEdgeDto,
    ): BridgeResult<StarMapEdgeDto> =
        holder.wrapResult {
            holder.service.addStarmapEdge(starmapId, edge)
        }

    override fun deleteStarMapEdge(
        starmapId: String,
        edgeId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.deleteStarmapEdge(starmapId, edgeId)
        }

    override fun updateStarMapEdge(
        starmapId: String,
        edgeId: String,
        patch: StarMapEdgePatchInputDto,
    ): BridgeResult<StarMapEdgeDto> =
        holder.wrapResult {
            holder.service.updateStarmapEdge(starmapId, edgeId, patch)
        }

    override fun flushStarmapStore(starmapId: String): BridgeResult<Boolean> {
        return holder.wrapResult {
            holder.service.flushStarmapStore(starmapId)
            true
        }
    }

    override fun closeStarmapStore(starmapId: String): BridgeResult<Boolean> {
        return holder.wrapResult {
            holder.service.closeStarmapStore(starmapId)
            true
        }
    }

    override fun flushAllStarmapStores(): BridgeResult<Boolean> {
        return holder.wrapResult {
            holder.service.flushAllStarmapStores()
            true
        }
    }
}
