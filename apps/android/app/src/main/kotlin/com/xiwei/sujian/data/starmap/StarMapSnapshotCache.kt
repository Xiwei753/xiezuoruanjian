package com.xiwei.sujian.data.starmap

import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapLayoutNodeDto
import uniffi.writer_core.StarMapNodeDto

internal data class StarMapRawCache(
    val graph: StarMapGraphDto? = null,
    val nodes: MutableMap<String, StarMapNodeDto> = mutableMapOf(),
    val edges: MutableMap<String, StarMapEdgeDto> = mutableMapOf(),
    val layoutNodes: MutableMap<String, StarMapLayoutNodeDto> = mutableMapOf()
)

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
        if (incoming.graph != null) {
            for (node in incoming.graph.nodes) {
                existing.nodes[node.id] = node
            }
            for (edge in incoming.graph.edges) {
                existing.edges[edge.id] = edge
            }
            val mergedGraph = StarMapGraphDto(
                schemaVersion = incoming.graph.schemaVersion,
                id = incoming.graph.id,
                starmapId = incoming.graph.starmapId,
                title = incoming.graph.title,
                nodes = existing.nodes.values.toList(),
                edges = existing.edges.values.toList(),
                embeds = incoming.graph.embeds,
                links = incoming.graph.links,
                createdAt = incoming.graph.createdAt,
                updatedAt = incoming.graph.updatedAt
            )
            rawCacheByStarmapId[starmapId] = existing.copy(graph = mergedGraph)
        }
        for ((nodeId, nodeDto) in incoming.nodes) {
            existing.nodes[nodeId] = nodeDto
        }
        for ((edgeId, edgeDto) in incoming.edges) {
            existing.edges[edgeId] = edgeDto
        }
        for ((nodeId, layoutNode) in incoming.layoutNodes) {
            existing.layoutNodes[nodeId] = layoutNode
        }
    }

    fun removeNode(starmapId: String, nodeId: String) { rawCacheByStarmapId[starmapId]?.nodes?.remove(nodeId) }

    fun removeEdge(starmapId: String, edgeId: String) { rawCacheByStarmapId[starmapId]?.edges?.remove(edgeId) }

    fun putNode(starmapId: String, nodeId: String, dto: StarMapNodeDto) { rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.nodes[nodeId] = dto }

    fun putEdge(starmapId: String, edgeId: String, dto: StarMapEdgeDto) { rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }.edges[edgeId] = dto }

    fun updateLayoutNodes(starmapId: String, nodes: List<StarMapLayoutNodeDto>) {
        val cache = rawCacheByStarmapId[starmapId] ?: return
        cache.layoutNodes.clear()
        cache.layoutNodes.putAll(nodes.associateBy { it.nodeId })
    }
}
