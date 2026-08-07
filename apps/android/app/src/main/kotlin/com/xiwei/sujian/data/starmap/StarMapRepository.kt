package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.ResultEnvelope
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapEdgeKind
import com.xiwei.sujian.model.StarMapEdgeRenderData
import com.xiwei.sujian.model.StarMapEmbedData
import com.xiwei.sujian.model.StarMapGraphEdge
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapHyperlinkData
import com.xiwei.sujian.model.StarMapLayoutData
import com.xiwei.sujian.model.StarMapLinkData
import com.xiwei.sujian.model.StarMapMeta
import com.xiwei.sujian.model.StarMapMotionPolicyData
import com.xiwei.sujian.model.StarMapNodeKind
import com.xiwei.sujian.model.StarMapPhasedSnapshotResult
import com.xiwei.sujian.model.StarMapViewportData
import uniffi.writer_core.PhasedSnapshotRequestDto
import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapEdgePatchInputDto
import uniffi.writer_core.StarMapNodePatchInputDto

internal class StarMapRepository(
    private val bridge: StarMapBridgeOps,
    private val cache: StarMapSnapshotCache,
) {
    fun listStarmaps(): BridgeResult<List<StarMapMeta>> {
        return when (val result = bridge.listStarMaps()) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.map { it.toModel() })
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun createStarmap(
        title: String,
        desc: String,
    ): BridgeResult<StarMapMeta> {
        return when (val result = bridge.createStarMap(title, desc)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun getStarmapPhasedSnapshot(
        starmapId: String,
        targetPhase: String = "CurrentViewportObjects",
        sinceRevision: ULong = 0u,
    ): BridgeResult<StarMapPhasedSnapshotResult> {
        val request =
            PhasedSnapshotRequestDto(
                targetPhase = targetPhase,
                sinceRevision = sinceRevision,
            )
        return when (val result = bridge.getStarmapPhasedSnapshot(starmapId, request)) {
            is BridgeResult.Success -> {
                try {
                    val incomingCache = result.data.toRawCache()
                    val existingCache = cache.get(starmapId)
                    if (existingCache != null && sinceRevision > 0u) {
                        cache.mergeIncremental(starmapId, incomingCache)
                    } else {
                        cache.put(starmapId, incomingCache)
                    }
                    val mergedCache = cache.get(starmapId)
                    if (mergedCache != null) {
                        BridgeResult.Success(mergedCache.toSnapshotResult())
                    } else {
                        BridgeResult.Success(result.data.toSnapshotResult())
                    }
                } catch (e: Exception) {
                    BridgeResult.Error(
                        ResultEnvelope.errorOf("CONVERSION_ERROR", "Failed to convert phased snapshot: ${e.message}"),
                    )
                }
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun advanceLoadPhase(
        starmapId: String,
        targetPhase: String,
        currentRevision: ULong,
    ): BridgeResult<StarMapPhasedSnapshotResult> {
        return getStarmapPhasedSnapshot(starmapId, targetPhase, currentRevision)
    }

    fun addStarmapNode(
        starmapId: String,
        node: StarMapGraphNode,
        x: Float = 0f,
        y: Float = 0f,
    ): BridgeResult<StarMapGraphNode> {
        val baseNode = cache.get(starmapId)?.nodes?.get(node.id)
        return when (val result = bridge.addStarMapNode(starmapId, node.toDto(baseNode), x, y)) {
            is BridgeResult.Success -> {
                cache.putNode(starmapId, result.data.id, result.data)
                BridgeResult.Success(result.data.toGraphNode())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun updateStarmapNode(
        starmapId: String,
        nodeId: String,
        title: String? = null,
        kind: StarMapNodeKind? = null,
        tags: List<String>? = null,
    ): BridgeResult<StarMapGraphNode> {
        val patch =
            StarMapNodePatchInputDto(
                title = title,
                kind = kind?.toDto(),
                payload = null,
                clearPayload = false,
                tags = tags,
            )
        return when (val result = bridge.updateStarMapNode(starmapId, nodeId, patch)) {
            is BridgeResult.Success -> {
                cache.get(starmapId)?.nodes?.put(result.data.id, result.data)
                BridgeResult.Success(result.data.toGraphNode())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun deleteStarmapNode(
        starmapId: String,
        nodeId: String,
    ): BridgeResult<Boolean> {
        return when (val result = bridge.deleteStarMapNode(starmapId, nodeId)) {
            is BridgeResult.Success -> {
                cache.removeNode(starmapId, nodeId)
                BridgeResult.Success(true)
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun addStarmapEdge(
        starmapId: String,
        from: String,
        to: String,
        kind: StarMapEdgeKind = StarMapEdgeKind.RelatedTo,
        label: String? = null,
    ): BridgeResult<StarMapGraphEdge> {
        val now = System.currentTimeMillis()
        val edge =
            StarMapEdgeDto(
                id = java.util.UUID.randomUUID().toString(),
                from = from,
                to = to,
                kind = kind.toDto(),
                label = label,
                payload = null,
                fromTarget = null,
                toTarget = null,
                fromEndpoint = null,
                toEndpoint = null,
                fromEndpointPath = null,
                toEndpointPath = null,
                createdAt = now.toULong(),
                updatedAt = now.toULong(),
            )
        return when (val result = bridge.addStarMapEdge(starmapId, edge)) {
            is BridgeResult.Success -> {
                cache.get(starmapId)?.edges?.put(result.data.id, result.data)
                BridgeResult.Success(result.data.toGraphEdge())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun deleteStarmapEdge(
        starmapId: String,
        edgeId: String,
    ): BridgeResult<Boolean> {
        return when (val result = bridge.deleteStarMapEdge(starmapId, edgeId)) {
            is BridgeResult.Success -> {
                cache.removeEdge(starmapId, edgeId)
                BridgeResult.Success(true)
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun updateStarmapEdge(
        starmapId: String,
        edgeId: String,
        kind: StarMapEdgeKind? = null,
        label: String? = null,
    ): BridgeResult<StarMapGraphEdge> {
        val patch =
            StarMapEdgePatchInputDto(
                kind = kind?.toDto(),
                label = label,
                clearLabel = false,
            )
        return when (val result = bridge.updateStarMapEdge(starmapId, edgeId, patch)) {
            is BridgeResult.Success -> {
                cache.get(starmapId)?.edges?.put(result.data.id, result.data)
                BridgeResult.Success(result.data.toGraphEdge())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun addStarmapEmbed(
        starmapId: String,
        embed: uniffi.writer_core.StarMapEmbedDto,
    ): BridgeResult<StarMapEmbedData> {
        return when (val result = bridge.addStarmapEmbed(starmapId, embed)) {
            is BridgeResult.Success -> {
                cache.putEmbed(starmapId, result.data.instanceId, result.data)
                BridgeResult.Success(result.data.toModel())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun updateStarmapEmbed(
        starmapId: String,
        instanceId: String,
        patch: uniffi.writer_core.StarMapEmbedPatchInputDto,
    ): BridgeResult<StarMapEmbedData> {
        return when (val result = bridge.updateStarmapEmbed(starmapId, instanceId, patch)) {
            is BridgeResult.Success -> {
                cache.putEmbed(starmapId, result.data.instanceId, result.data)
                BridgeResult.Success(result.data.toModel())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun deleteStarmapEmbed(
        starmapId: String,
        instanceId: String,
    ): BridgeResult<Boolean> {
        return when (val result = bridge.deleteStarmapEmbed(starmapId, instanceId)) {
            is BridgeResult.Success -> {
                cache.removeEmbed(starmapId, instanceId)
                BridgeResult.Success(true)
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun addStarmapLink(
        starmapId: String,
        link: uniffi.writer_core.StarMapLinkDto,
    ): BridgeResult<StarMapLinkData> {
        return when (val result = bridge.addStarmapLink(starmapId, link)) {
            is BridgeResult.Success -> {
                cache.putLink(starmapId, result.data.linkId, result.data)
                BridgeResult.Success(result.data.toModel())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun updateStarmapLink(
        starmapId: String,
        linkId: String,
        patch: uniffi.writer_core.StarMapLinkPatchInputDto,
    ): BridgeResult<StarMapLinkData> {
        return when (val result = bridge.updateStarmapLink(starmapId, linkId, patch)) {
            is BridgeResult.Success -> {
                cache.putLink(starmapId, result.data.linkId, result.data)
                BridgeResult.Success(result.data.toModel())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun deleteStarmapLink(
        starmapId: String,
        linkId: String,
    ): BridgeResult<Boolean> {
        return when (val result = bridge.deleteStarmapLink(starmapId, linkId)) {
            is BridgeResult.Success -> {
                cache.removeLink(starmapId, linkId)
                BridgeResult.Success(true)
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun addStarmapHyperlink(
        starmapId: String,
        hl: uniffi.writer_core.StarMapHyperlinkDto,
    ): BridgeResult<StarMapHyperlinkData> {
        return when (val result = bridge.addStarmapHyperlink(starmapId, hl)) {
            is BridgeResult.Success -> {
                cache.putHyperlink(starmapId, result.data.hyperlinkId, result.data)
                BridgeResult.Success(result.data.toModel())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun updateStarmapHyperlink(
        starmapId: String,
        hyperlinkId: String,
        patch: uniffi.writer_core.StarMapHyperlinkPatchInputDto,
    ): BridgeResult<StarMapHyperlinkData> {
        return when (val result = bridge.updateStarmapHyperlink(starmapId, hyperlinkId, patch)) {
            is BridgeResult.Success -> {
                cache.putHyperlink(starmapId, result.data.hyperlinkId, result.data)
                BridgeResult.Success(result.data.toModel())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun deleteStarmapHyperlink(
        starmapId: String,
        hyperlinkId: String,
    ): BridgeResult<Boolean> {
        return when (val result = bridge.deleteStarmapHyperlink(starmapId, hyperlinkId)) {
            is BridgeResult.Success -> {
                cache.removeHyperlink(starmapId, hyperlinkId)
                BridgeResult.Success(true)
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun listStarmapHyperlinks(starmapId: String): BridgeResult<List<StarMapHyperlinkData>> {
        return when (val result = bridge.listStarmapHyperlinks(starmapId)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.items.map { it.toModel() })
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun saveStarmapLayout(
        starmapId: String,
        layout: StarMapLayoutData,
    ): BridgeResult<Boolean> {
        val rawCache =
            cache.get(starmapId) ?: return BridgeResult.Error(
                ResultEnvelope.errorOf(
                    "SNAPSHOT_CACHE_NOT_INITIALIZED",
                    "Starmap cache not initialized for $starmapId. Call getStarmapPhasedSnapshot first.",
                ),
            )
        val dto = layout.toDto(rawCache)
        return when (val result = bridge.saveStarMapLayout(starmapId, dto)) {
            is BridgeResult.Success -> {
                cache.updateLayoutNodes(starmapId, dto.nodes)
                BridgeResult.Success(result.data)
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun getStarmapViewport(starmapId: String): BridgeResult<StarMapViewportData> {
        return when (val result = bridge.getStarMapViewport(starmapId)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun saveStarmapViewport(
        starmapId: String,
        viewport: StarMapViewportData,
    ): BridgeResult<Boolean> {
        return bridge.saveStarMapViewport(starmapId, viewport.toDto())
    }

    fun computeEdgeRenders(data: StarMapData): BridgeResult<List<StarMapEdgeRenderData>> {
        val rawCache =
            cache.get(data.graph.starmapId) ?: return BridgeResult.Error(
                ResultEnvelope.errorOf(
                    "SNAPSHOT_CACHE_NOT_INITIALIZED",
                    "Starmap snapshot cache not initialized for ${data.graph.starmapId}. " +
                        "Call getStarmapPhasedSnapshot first.",
                ),
            )
        val graph =
            rawCache.graph ?: return BridgeResult.Error(
                ResultEnvelope.errorOf(
                    "STAR_MAP_CACHE_MISSING",
                    "Raw starmap graph is not available in snapshot cache. " +
                        "This should not happen after a successful getStarmapPhasedSnapshot call.",
                ),
            )
        return when (val result = bridge.computeStarMapEdgeRenders(graph, data.layout.toDto(rawCache))) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.map { it.toModel() })
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun hitTestStarmapNode(
        data: StarMapData,
        x: Float,
        y: Float,
    ): BridgeResult<String?> {
        val rawCache =
            cache.get(data.graph.starmapId) ?: return BridgeResult.Error(
                ResultEnvelope.errorOf(
                    "SNAPSHOT_CACHE_NOT_INITIALIZED",
                    "Starmap snapshot cache not initialized for ${data.graph.starmapId}. " +
                        "Call getStarmapPhasedSnapshot first.",
                ),
            )
        return bridge.hitTestStarMapNode(data.layout.toDto(rawCache), x, y)
    }

    fun getMotionPolicy(): BridgeResult<StarMapMotionPolicyData> {
        return when (val result = bridge.getStarMapMotionPolicy()) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun flushStarmapStore(starmapId: String): BridgeResult<Boolean> = bridge.flushStarmapStore(starmapId)

    fun closeStarmapStore(starmapId: String): BridgeResult<Boolean> = bridge.closeStarmapStore(starmapId)

    fun flushAllStarmapStores(): BridgeResult<Boolean> = bridge.flushAllStarmapStores()
}
