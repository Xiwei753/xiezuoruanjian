package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapGraphData
import com.xiwei.sujian.model.StarMapLayoutData
import com.xiwei.sujian.model.StarMapLayoutKind
import com.xiwei.sujian.model.StarMapPhasedSnapshotResult
import com.xiwei.sujian.model.StarMapViewportData
import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapEmbedDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapHyperlinkDto
import uniffi.writer_core.StarMapLayoutKindDto
import uniffi.writer_core.StarMapLayoutNodeDto
import uniffi.writer_core.StarMapLinkDto
import uniffi.writer_core.StarMapNodeDto
import uniffi.writer_core.StarMapViewportDto

internal data class StarMapRawCache(
    var graph: StarMapGraphDto? = null,
    val nodes: MutableMap<String, StarMapNodeDto> = mutableMapOf(),
    val edges: MutableMap<String, StarMapEdgeDto> = mutableMapOf(),
    val embeds: MutableMap<String, StarMapEmbedDto> = mutableMapOf(),
    val links: MutableMap<String, StarMapLinkDto> = mutableMapOf(),
    val hyperlinks: MutableMap<String, StarMapHyperlinkDto> = mutableMapOf(),
    val layoutNodes: MutableMap<String, StarMapLayoutNodeDto> = mutableMapOf(),
    var layoutKind: StarMapLayoutKindDto = StarMapLayoutKindDto.FREEFORM,
    var loadPhase: String = "CurrentViewportObjects",
    var packageRevision: ULong = 0u,
    var sinceRevision: ULong = 0u,
    var complete: Boolean = false,
    var viewport: StarMapViewportDto? = null,
    var diagnostics: List<com.xiwei.sujian.model.StarMapLoadDiagnostic> = emptyList(),
    val deletedNodeIds: MutableSet<String> = mutableSetOf(),
    val deletedEdgeIds: MutableSet<String> = mutableSetOf(),
    val deletedEmbedIds: MutableSet<String> = mutableSetOf(),
    val deletedLinkIds: MutableSet<String> = mutableSetOf(),
    val deletedHyperlinkIds: MutableSet<String> = mutableSetOf()
)

internal fun StarMapRawCache.toSnapshotResult(): StarMapPhasedSnapshotResult {
    val layoutData = if (layoutNodes.isNotEmpty()) {
        StarMapLayoutData(
            kind = when (layoutKind) {
                StarMapLayoutKindDto.FREEFORM -> StarMapLayoutKind.Freeform
                StarMapLayoutKindDto.AUTO_RADIAL -> StarMapLayoutKind.AutoRadial
                StarMapLayoutKindDto.CUSTOM -> StarMapLayoutKind.Custom
            },
            nodes = layoutNodes.values.map { it.toModel() }
        )
    } else {
        StarMapLayoutData(kind = StarMapLayoutKind.Freeform, nodes = emptyList())
    }
    val graphMeta = graph
    val data = StarMapData(
        graph = StarMapGraphData(
            schemaVersion = graphMeta?.schemaVersion?.toInt() ?: 0,
            id = graphMeta?.id ?: "",
            starmapId = graphMeta?.starmapId ?: "",
            title = graphMeta?.title ?: "",
            nodes = nodes.values.map { it.toGraphNode() },
            edges = edges.values.map { it.toGraphEdge() },
            createdAt = graphMeta?.createdAt?.toLong() ?: 0L,
            updatedAt = graphMeta?.updatedAt?.toLong() ?: 0L
        ),
        layout = layoutData,
        viewport = viewport?.toModel() ?: StarMapViewportData(),
        embeds = embeds.values.map { it.toModel() },
        links = links.values.map { it.toModel() },
        hyperlinks = hyperlinks.values.map { it.toModel() },
        loadPhase = loadPhase,
        packageRevision = packageRevision,
        sinceRevision = sinceRevision,
        complete = complete
    )
    return StarMapPhasedSnapshotResult(
        data = data,
        diagnostics = diagnostics
    )
}

internal class StarMapSnapshotCache {
    private val rawCacheByStarmapId = mutableMapOf<String, StarMapRawCache>()

    fun get(starmapId: String): StarMapRawCache? = rawCacheByStarmapId[starmapId]

    fun getOrPut(starmapId: String): StarMapRawCache = rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }

    fun put(starmapId: String, cache: StarMapRawCache) { rawCacheByStarmapId[starmapId] = cache }

    fun mergeIncremental(starmapId: String, incoming: StarMapRawCache) {
        val existing = rawCacheByStarmapId[starmapId]
        if (existing == null) {
            rawCacheByStarmapId[starmapId] = incoming
            return
        }
        val incomingGraph = incoming.graph
        if (incomingGraph != null) {
            for (node in incomingGraph.nodes) {
                existing.nodes[node.id] = node
            }
            for (edge in incomingGraph.edges) {
                existing.edges[edge.id] = edge
            }
            for (embed in incomingGraph.embeds) {
                existing.embeds[embed.instanceId] = embed
            }
            for (link in incomingGraph.links) {
                existing.links[link.linkId] = link
            }
        }
        for ((nodeId, nodeDto) in incoming.nodes) {
            existing.nodes[nodeId] = nodeDto
        }
        for ((edgeId, edgeDto) in incoming.edges) {
            existing.edges[edgeId] = edgeDto
        }
        for ((instanceId, embedDto) in incoming.embeds) {
            existing.embeds[instanceId] = embedDto
        }
        for ((linkId, linkDto) in incoming.links) {
            existing.links[linkId] = linkDto
        }
        for ((hyperlinkId, hlDto) in incoming.hyperlinks) {
            existing.hyperlinks[hyperlinkId] = hlDto
        }
        for ((nodeId, layoutNode) in incoming.layoutNodes) {
            existing.layoutNodes[nodeId] = layoutNode
        }
        if (incoming.layoutKind != StarMapLayoutKindDto.FREEFORM || existing.layoutKind == StarMapLayoutKindDto.FREEFORM) {
            existing.layoutKind = incoming.layoutKind
        }
        if (incoming.loadPhase != "CurrentViewportObjects" || existing.loadPhase == "CurrentViewportObjects") {
            existing.loadPhase = incoming.loadPhase
        }
        if (incoming.packageRevision > existing.packageRevision) {
            existing.packageRevision = incoming.packageRevision
        }
        if (incoming.complete) {
            existing.complete = true
        }
        if (incoming.viewport != null) {
            existing.viewport = incoming.viewport
        }
        if (incoming.diagnostics.isNotEmpty()) {
            existing.diagnostics = incoming.diagnostics
        }
        for (deletedId in incoming.deletedNodeIds) {
            existing.nodes.remove(deletedId)
            existing.deletedNodeIds.add(deletedId)
        }
        for (deletedId in incoming.deletedEdgeIds) {
            existing.edges.remove(deletedId)
            existing.deletedEdgeIds.add(deletedId)
        }
        for (deletedId in incoming.deletedEmbedIds) {
            existing.embeds.remove(deletedId)
            existing.deletedEmbedIds.add(deletedId)
        }
        for (deletedId in incoming.deletedLinkIds) {
            existing.links.remove(deletedId)
            existing.deletedLinkIds.add(deletedId)
        }
        for (deletedId in incoming.deletedHyperlinkIds) {
            existing.hyperlinks.remove(deletedId)
            existing.deletedHyperlinkIds.add(deletedId)
        }
        rebuildGraph(existing)
    }

    private fun rebuildGraph(cache: StarMapRawCache) {
        val meta = cache.graph ?: return
        cache.graph = StarMapGraphDto(
            schemaVersion = meta.schemaVersion,
            id = meta.id,
            starmapId = meta.starmapId,
            title = meta.title,
            nodes = cache.nodes.values.toList(),
            edges = cache.edges.values.toList(),
            embeds = cache.embeds.values.toList(),
            links = cache.links.values.toList(),
            createdAt = meta.createdAt,
            updatedAt = meta.updatedAt
        )
    }

    fun removeNode(starmapId: String, nodeId: String) { rawCacheByStarmapId[starmapId]?.nodes?.remove(nodeId) }

    fun removeEdge(starmapId: String, edgeId: String) { rawCacheByStarmapId[starmapId]?.edges?.remove(edgeId) }

    fun removeEmbed(starmapId: String, instanceId: String) { rawCacheByStarmapId[starmapId]?.embeds?.remove(instanceId) }

    fun removeLink(starmapId: String, linkId: String) { rawCacheByStarmapId[starmapId]?.links?.remove(linkId) }

    fun removeHyperlink(starmapId: String, hyperlinkId: String) { rawCacheByStarmapId[starmapId]?.hyperlinks?.remove(hyperlinkId) }

    fun putNode(starmapId: String, nodeId: String, dto: StarMapNodeDto) { rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.nodes[nodeId] = dto }

    fun putEdge(starmapId: String, edgeId: String, dto: StarMapEdgeDto) { rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.edges[edgeId] = dto }

    fun putEmbed(starmapId: String, instanceId: String, dto: StarMapEmbedDto) { rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.embeds[instanceId] = dto }

    fun putLink(starmapId: String, linkId: String, dto: StarMapLinkDto) { rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.links[linkId] = dto }

    fun putHyperlink(starmapId: String, hyperlinkId: String, dto: StarMapHyperlinkDto) { rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.hyperlinks[hyperlinkId] = dto }

    fun updateLayoutNodes(starmapId: String, nodes: List<StarMapLayoutNodeDto>) {
        val cache = rawCacheByStarmapId[starmapId] ?: return
        cache.layoutNodes.clear()
        cache.layoutNodes.putAll(nodes.associateBy { it.nodeId })
    }
}
