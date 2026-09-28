package com.xiwei.sujian.feature.starmap.data

import com.xiwei.sujian.feature.starmap.data.interop.toGraphEdge
import com.xiwei.sujian.feature.starmap.data.interop.toGraphNode
import com.xiwei.sujian.feature.starmap.data.interop.toModel
import com.xiwei.sujian.feature.starmap.data.model.StarMapData
import com.xiwei.sujian.feature.starmap.data.model.StarMapGraphData
import com.xiwei.sujian.feature.starmap.data.model.StarMapPhasedSnapshotResult
import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapEmbedDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapHyperlinkDto
import uniffi.writer_core.StarMapLinkDto
import uniffi.writer_core.StarMapNodeDto

/**
 * 星图原始 DTO 缓存。
 *
 * Core 收口后布局/视口不再随快照下发，节点坐标真相在 nodes[*].position 里，
 * 因此这里只缓存图对象本身。
 */
internal data class StarMapRawCache(
    var graph: StarMapGraphDto? = null,
    val nodes: MutableMap<String, StarMapNodeDto> = mutableMapOf(),
    val edges: MutableMap<String, StarMapEdgeDto> = mutableMapOf(),
    val embeds: MutableMap<String, StarMapEmbedDto> = mutableMapOf(),
    val links: MutableMap<String, StarMapLinkDto> = mutableMapOf(),
    val hyperlinks: MutableMap<String, StarMapHyperlinkDto> = mutableMapOf(),
    var loadPhase: String = "CurrentViewportObjects",
    var packageRevision: ULong = 0u,
    var sinceRevision: ULong = 0u,
    var complete: Boolean = false,
    var diagnostics: List<com.xiwei.sujian.feature.starmap.data.model.StarMapLoadDiagnostic> = emptyList(),
    val deletedNodeIds: MutableSet<String> = mutableSetOf(),
    val deletedEdgeIds: MutableSet<String> = mutableSetOf(),
    val deletedEmbedIds: MutableSet<String> = mutableSetOf(),
    val deletedLinkIds: MutableSet<String> = mutableSetOf(),
    val deletedHyperlinkIds: MutableSet<String> = mutableSetOf(),
)

internal fun StarMapRawCache.toSnapshotResult(): StarMapPhasedSnapshotResult {
    val graphMeta = graph
    val data =
        StarMapData(
            graph =
                StarMapGraphData(
                    schemaVersion = graphMeta?.schemaVersion?.toInt() ?: 0,
                    starmapId = graphMeta?.starmapId ?: "",
                    nodes = nodes.values.map { it.toGraphNode() },
                    edges = edges.values.map { it.toGraphEdge() },
                ),
            embeds = embeds.values.map { it.toModel() },
            links = links.values.map { it.toModel() },
            hyperlinks = hyperlinks.values.map { it.toModel() },
            loadPhase = loadPhase,
            packageRevision = packageRevision,
            sinceRevision = sinceRevision,
            complete = complete,
        )
    return StarMapPhasedSnapshotResult(
        data = data,
        diagnostics = diagnostics,
    )
}

internal class StarMapSnapshotCache {
    private val rawCacheByStarmapId = mutableMapOf<String, StarMapRawCache>()

    fun get(starmapId: String): StarMapRawCache? = rawCacheByStarmapId[starmapId]

    fun getOrPut(starmapId: String): StarMapRawCache = rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }

    /**
     * #600 评论 #5：清空所有星图内存缓存 — 应用级同步后本地星图文件可能被远端覆盖，
     * 内存缓存不再有效，必须失效以使下次读取从磁盘重新加载。
     */
    fun clear() {
        rawCacheByStarmapId.clear()
    }

    fun put(
        starmapId: String,
        cache: StarMapRawCache,
    ) {
        rawCacheByStarmapId[starmapId] = cache
    }

    fun mergeIncremental(
        starmapId: String,
        incoming: StarMapRawCache,
    ) {
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
        if (incoming.loadPhase != "CurrentViewportObjects" || existing.loadPhase == "CurrentViewportObjects") {
            existing.loadPhase = incoming.loadPhase
        }
        if (incoming.packageRevision > existing.packageRevision) {
            existing.packageRevision = incoming.packageRevision
        }
        if (incoming.complete) {
            existing.complete = true
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
        cache.graph =
            StarMapGraphDto(
                schemaVersion = meta.schemaVersion,
                starmapId = meta.starmapId,
                nodes = cache.nodes.values.toList(),
                edges = cache.edges.values.toList(),
                embeds = cache.embeds.values.toList(),
                links = cache.links.values.toList(),
                hyperlinks = cache.hyperlinks.values.toList(),
            )
    }

    fun removeNode(
        starmapId: String,
        nodeId: String,
    ) {
        rawCacheByStarmapId[starmapId]?.nodes?.remove(nodeId)
    }

    fun removeEdge(
        starmapId: String,
        edgeId: String,
    ) {
        rawCacheByStarmapId[starmapId]?.edges?.remove(edgeId)
    }

    fun removeEmbed(
        starmapId: String,
        instanceId: String,
    ) {
        rawCacheByStarmapId[starmapId]?.embeds?.remove(instanceId)
    }

    fun removeLink(
        starmapId: String,
        linkId: String,
    ) {
        rawCacheByStarmapId[starmapId]?.links?.remove(linkId)
    }

    fun removeHyperlink(
        starmapId: String,
        hyperlinkId: String,
    ) {
        rawCacheByStarmapId[starmapId]?.hyperlinks?.remove(hyperlinkId)
    }

    fun putNode(
        starmapId: String,
        nodeId: String,
        dto: StarMapNodeDto,
    ) {
        rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.nodes[nodeId] = dto
    }

    fun putEdge(
        starmapId: String,
        edgeId: String,
        dto: StarMapEdgeDto,
    ) {
        rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.edges[edgeId] = dto
    }

    fun putEmbed(
        starmapId: String,
        instanceId: String,
        dto: StarMapEmbedDto,
    ) {
        rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.embeds[instanceId] = dto
    }

    fun putLink(
        starmapId: String,
        linkId: String,
        dto: StarMapLinkDto,
    ) {
        rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.links[linkId] = dto
    }

    fun putHyperlink(
        starmapId: String,
        hyperlinkId: String,
        dto: StarMapHyperlinkDto,
    ) {
        rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.hyperlinks[hyperlinkId] = dto
    }
}
