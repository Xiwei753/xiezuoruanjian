package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.data.BridgeResult
import uniffi.writer_core.*

internal interface StarMapBridgeOps {
    fun listStarMaps(): BridgeResult<List<StarMapMetaDto>>
    @Deprecated("Use getStarmapPhasedSnapshot for progressive loading.")
    fun getStarMapGraph(starmapId: String): BridgeResult<StarMapGraphDto>
    fun createStarMap(title: String, desc: String): BridgeResult<StarMapMetaDto>
    fun addStarMapNode(starmapId: String, node: StarMapNodeDto, x: Float, y: Float): BridgeResult<StarMapNodeDto>
    fun saveStarMapLayout(starmapId: String, layout: StarMapLayoutDto): BridgeResult<Boolean>
    fun getStarMapViewport(starmapId: String): BridgeResult<StarMapViewportDto>
    fun saveStarMapViewport(starmapId: String, viewport: StarMapViewportDto): BridgeResult<Boolean>
    fun computeStarMapEdgeRenders(graph: StarMapGraphDto, layout: StarMapLayoutDto): BridgeResult<List<StarMapEdgeRenderDto>>
    fun hitTestStarMapNode(layout: StarMapLayoutDto, x: Float, y: Float): BridgeResult<String?>
    fun addStarmapEmbed(starmapId: String, embed: StarMapEmbedDto): BridgeResult<StarMapEmbedDto>
    fun updateStarmapEmbed(starmapId: String, instanceId: String, patch: StarMapEmbedPatchInputDto): BridgeResult<StarMapEmbedDto>
    fun deleteStarmapEmbed(starmapId: String, instanceId: String): BridgeResult<Boolean>
    fun addStarmapLink(starmapId: String, link: StarMapLinkDto): BridgeResult<StarMapLinkDto>
    fun updateStarmapLink(starmapId: String, linkId: String, patch: StarMapLinkPatchInputDto): BridgeResult<StarMapLinkDto>
    fun deleteStarmapLink(starmapId: String, linkId: String): BridgeResult<Boolean>
    fun addStarmapHyperlink(starmapId: String, hl: StarMapHyperlinkDto): BridgeResult<StarMapHyperlinkDto>
    fun updateStarmapHyperlink(starmapId: String, hyperlinkId: String, patch: StarMapHyperlinkPatchInputDto): BridgeResult<StarMapHyperlinkDto>
    fun deleteStarmapHyperlink(starmapId: String, hyperlinkId: String): BridgeResult<Boolean>
    fun listStarmapHyperlinks(starmapId: String): BridgeResult<StarMapHyperlinkListWithDiagnosticsDto>
    fun getStarmapPhasedSnapshot(starmapId: String, request: PhasedSnapshotRequestDto): BridgeResult<StarMapPhasedSnapshotDto>
    fun findStarmapReferences(targetStarmapId: String): BridgeResult<List<StarMapReferenceDto>>
    fun getStarMapMotionPolicy(): BridgeResult<StarMapMotionPolicyDto>
    fun updateStarMapNode(starmapId: String, nodeId: String, patch: StarMapNodePatchInputDto): BridgeResult<StarMapNodeDto>
    fun deleteStarMapNode(starmapId: String, nodeId: String): BridgeResult<Boolean>
    fun addStarMapEdge(starmapId: String, edge: StarMapEdgeDto): BridgeResult<StarMapEdgeDto>
    fun deleteStarMapEdge(starmapId: String, edgeId: String): BridgeResult<Boolean>
    fun updateStarMapEdge(starmapId: String, edgeId: String, patch: StarMapEdgePatchInputDto): BridgeResult<StarMapEdgeDto>
    fun flushStarmapStore(starmapId: String): BridgeResult<Boolean>
    fun closeStarmapStore(starmapId: String): BridgeResult<Boolean>
    fun flushAllStarmapStores(): BridgeResult<Boolean>
}
