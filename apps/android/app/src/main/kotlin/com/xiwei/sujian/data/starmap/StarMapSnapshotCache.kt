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
