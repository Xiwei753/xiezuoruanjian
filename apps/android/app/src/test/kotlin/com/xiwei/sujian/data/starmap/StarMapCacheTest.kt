package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapCacheTest {

    private val emptyDeletedIds = listOf<String>()

    @Test
    fun rawCache_fromSnapshot_hasGraphDto() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = listOf(
                StarMapNodeDto(
                    id = "n1", title = "A", kind = StarMapNodeKindDto.NOTE,
                    payload = null, tags = emptyList(),
                    content = StarMapNodeContentDto("note", null, null, null, null, null, null, null, null, null, null, null),
                    anchors = emptyList(), portal = null,
                    displayPolicy = defaultStarMapDisplayPolicy(),
                    openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
                    provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
                    createdAt = 0u, updatedAt = 0u
                )
            ),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = null, diagnostics = emptyList(),
            deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds
        )
        val cache = dto.toRawCache()
        assertNotNull("rawCache.graph must be initialized from snapshot", cache.graph)
        assertEquals(1, cache.graph!!.nodes.size)
        assertEquals(1, cache.nodes.size)
        assertEquals(1, cache.layoutNodes.size)
        assertTrue(cache.layoutNodes.containsKey("n1"))
    }

    @Test
    fun snapshotCache_putAndGet_isConsistent() {
        val cache = StarMapSnapshotCache()
        val rawCache = StarMapRawCache(
            graph = StarMapGraphDto(
                schemaVersion = 1u, id = "sm1", starmapId = "sm1", title = "T",
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(), links = emptyList(),
                hyperlinks = emptyList(),
                createdAt = 0u, updatedAt = 0u
            ),
            nodes = mutableMapOf(), edges = mutableMapOf(),
            embeds = mutableMapOf(), links = mutableMapOf(), hyperlinks = mutableMapOf(),
            layoutNodes = mutableMapOf()
        )
        cache.put("sm1", rawCache)
        val retrieved = cache.get("sm1")
        assertNotNull(retrieved)
        assertEquals("sm1", retrieved!!.graph!!.starmapId)
    }

    @Test
    fun snapshotCache_crudUpdatesAreConsistent() {
        val cache = StarMapSnapshotCache()
        val rawCache = StarMapRawCache(graph = null, nodes = mutableMapOf(), edges = mutableMapOf(),
            embeds = mutableMapOf(), links = mutableMapOf(), hyperlinks = mutableMapOf(), layoutNodes = mutableMapOf())
        cache.put("sm1", rawCache)

        val nodeDto = StarMapNodeDto(
            id = "n1", title = "A", kind = StarMapNodeKindDto.NOTE,
            payload = null, tags = emptyList(),
            content = StarMapNodeContentDto("note", null, null, null, null, null, null, null, null, null, null, null),
            anchors = emptyList(), portal = null,
            displayPolicy = defaultStarMapDisplayPolicy(),
            openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
            provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
            createdAt = 0u, updatedAt = 0u
        )
        cache.putNode("sm1", "n1", nodeDto)
        val c = cache.get("sm1")
        assertNotNull(c)
        assertTrue(c!!.nodes.containsKey("n1"))

        cache.removeNode("sm1", "n1")
        assertFalse(cache.get("sm1")!!.nodes.containsKey("n1"))
    }

    @Test
    fun snapshotCache_computeEdgeRenders_requiresCache() {
        val cache = StarMapSnapshotCache()
        assertNull("cache must be null before snapshot is loaded", cache.get("sm1"))
    }

    @Test
    fun phasedSnapshotDto_toSnapshotResult_emptyLayoutWhenNull() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "GraphMeta",
            packageRevision = 0u, complete = false, sinceRevision = 0u,
            nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
            links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList(),
            deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds
        )
        val result = dto.toSnapshotResult()
        assertNotNull(result.data.layout)
        assertEquals(StarMapLayoutKind.Freeform, result.data.layout.kind)
        assertTrue(result.data.layout.nodes.isEmpty())
    }

    @Test
    fun phasedSnapshotDto_toSnapshotResult_loadPhaseAndCompletePreserved() {
        val dto1 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
            links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList(),
            deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds
        )
        val result1 = dto1.toSnapshotResult()
        assertEquals("CurrentViewportObjects", result1.data.loadPhase)
        assertFalse(result1.data.complete)

        val dto2 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 2u, complete = true, sinceRevision = 1u,
            nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
            links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList(),
            deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds
        )
        val result2 = dto2.toSnapshotResult()
        assertEquals("BackgroundFullLoad", result2.data.loadPhase)
        assertTrue(result2.data.complete)
        assertEquals(1uL, result2.data.sinceRevision)
    }

    @Test
    fun rawCache_toSnapshotResult_preservesAllFields() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 3u, complete = true, sinceRevision = 0u,
            nodes = listOf(StarMapNodeDto(
                id = "n1", title = "A", kind = StarMapNodeKindDto.CHARACTER,
                payload = null, tags = emptyList(),
                content = StarMapNodeContentDto("empty", null, null, null, null, null, null, null, null, null, null, null),
                anchors = emptyList(), portal = null,
                displayPolicy = defaultStarMapDisplayPolicy(),
                openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
                provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
                createdAt = 0u, updatedAt = 0u
            )),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 100f, y = 200f, width = 150f, height = 80f,
                    radius = 40f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = StarMapViewportDto(2f, 10f, 20f, 800f, 600f),
            diagnostics = emptyList(),
            deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds
        )
        val rawCache = dto.toRawCache()
        val result = rawCache.toSnapshotResult()
        assertEquals(1, result.data.graph.nodes.size)
        assertEquals("A", result.data.graph.nodes[0].title)
        assertEquals(1, result.data.layout.nodes.size)
        assertEquals(100f, result.data.layout.nodes[0].x, 0.001f)
        assertEquals(2f, result.data.viewport.scale, 0.001f)
        assertEquals(3uL, result.data.packageRevision)
        assertTrue(result.data.complete)
    }

    private fun makeHyperlinkDto(
        hyperlinkId: String = "hl1",
        targetUri: String = "https://example.com",
        label: String? = "Example",
        targetStarmapId: String? = null
    ) = StarMapHyperlinkDto(
        hyperlinkId = hyperlinkId,
        source = StarMapEndpointPathDto(
            segments = emptyList(),
            endpoint = StarMapEdgeEndpointDto(kind = "node", nodeId = "n1", anchorId = null, target = null)
        ),
        targetUri = targetUri,
        label = label,
        targetStarmapId = targetStarmapId,
        createdAt = 0u,
        updatedAt = 0u
    )

    @Test
    fun snapshotDto_toRawCache_preservesHyperlinks() {
        val hl = makeHyperlinkDto()
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
            links = emptyList(), hyperlinks = listOf(hl),
            layout = null, viewport = null, diagnostics = emptyList(),
            deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds,
            deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds
        )
        val cache = dto.toRawCache()
        assertEquals(1, cache.hyperlinks.size)
        assertTrue(cache.hyperlinks.containsKey("hl1"))
        assertEquals("https://example.com", cache.hyperlinks["hl1"]!!.targetUri)
        assertEquals("Example", cache.hyperlinks["hl1"]!!.label)
    }

    @Test
    fun rawCache_toSnapshotResult_preservesHyperlinks() {
        val hl = makeHyperlinkDto()
        val cache = StarMapRawCache(
            graph = StarMapGraphDto(
                schemaVersion = 1u, id = "sm1", starmapId = "sm1", title = "T",
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(), links = emptyList(),
                hyperlinks = listOf(hl),
                createdAt = 0u, updatedAt = 0u
            ),
            nodes = mutableMapOf(), edges = mutableMapOf(), embeds = mutableMapOf(),
            links = mutableMapOf(),
            hyperlinks = mutableMapOf("hl1" to hl),
            layoutNodes = mutableMapOf()
        )
        val result = cache.toSnapshotResult()
        assertEquals(1, result.data.hyperlinks.size)
        assertEquals("hl1", result.data.hyperlinks[0].hyperlinkId)
        assertEquals("https://example.com", result.data.hyperlinks[0].targetUri)
    }

    @Test
    fun snapshotCache_mergeIncremental_preservesHyperlinks() {
        val cache = StarMapSnapshotCache()
        val hl1 = makeHyperlinkDto(hyperlinkId = "hl1", targetUri = "https://a.com")
        val rawCache1 = StarMapRawCache(
            graph = StarMapGraphDto(
                schemaVersion = 1u, id = "sm1", starmapId = "sm1", title = "T",
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(), links = emptyList(),
                hyperlinks = listOf(hl1),
                createdAt = 0u, updatedAt = 0u
            ),
            nodes = mutableMapOf(), edges = mutableMapOf(), embeds = mutableMapOf(),
            links = mutableMapOf(),
            hyperlinks = mutableMapOf("hl1" to hl1),
            layoutNodes = mutableMapOf(),
            packageRevision = 1u
        )
        cache.put("sm1", rawCache1)

        val hl2 = makeHyperlinkDto(hyperlinkId = "hl2", targetUri = "https://b.com")
        val rawCache2 = StarMapRawCache(
            graph = StarMapGraphDto(
                schemaVersion = 1u, id = "sm1", starmapId = "sm1", title = "T",
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(), links = emptyList(),
                hyperlinks = listOf(hl1, hl2),
                createdAt = 0u, updatedAt = 0u
            ),
            nodes = mutableMapOf(), edges = mutableMapOf(), embeds = mutableMapOf(),
            links = mutableMapOf(),
            hyperlinks = mutableMapOf("hl1" to hl1, "hl2" to hl2),
            layoutNodes = mutableMapOf(),
            packageRevision = 2u
        )
        cache.mergeIncremental("sm1", rawCache2)

        val merged = cache.get("sm1")!!
        assertEquals(2, merged.hyperlinks.size)
        assertTrue(merged.hyperlinks.containsKey("hl1"))
        assertTrue(merged.hyperlinks.containsKey("hl2"))
    }

    @Test
    fun snapshotCache_mergeIncremental_deletesHyperlinks() {
        val cache = StarMapSnapshotCache()
        val hl1 = makeHyperlinkDto(hyperlinkId = "hl1")
        val hl2 = makeHyperlinkDto(hyperlinkId = "hl2")
        val rawCache1 = StarMapRawCache(
            graph = StarMapGraphDto(
                schemaVersion = 1u, id = "sm1", starmapId = "sm1", title = "T",
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(), links = emptyList(),
                hyperlinks = listOf(hl1, hl2),
                createdAt = 0u, updatedAt = 0u
            ),
            nodes = mutableMapOf(), edges = mutableMapOf(), embeds = mutableMapOf(),
            links = mutableMapOf(),
            hyperlinks = mutableMapOf("hl1" to hl1, "hl2" to hl2),
            layoutNodes = mutableMapOf()
        )
        cache.put("sm1", rawCache1)

        val rawCache2 = StarMapRawCache(
            graph = StarMapGraphDto(
                schemaVersion = 1u, id = "sm1", starmapId = "sm1", title = "T",
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(), links = emptyList(),
                hyperlinks = emptyList(),
                createdAt = 0u, updatedAt = 0u
            ),
            nodes = mutableMapOf(), edges = mutableMapOf(), embeds = mutableMapOf(),
            links = mutableMapOf(), hyperlinks = mutableMapOf(),
            layoutNodes = mutableMapOf(),
            deletedHyperlinkIds = mutableSetOf("hl1")
        )
        cache.mergeIncremental("sm1", rawCache2)

        val merged = cache.get("sm1")!!
        assertEquals(1, merged.hyperlinks.size)
        assertFalse(merged.hyperlinks.containsKey("hl1"))
        assertTrue(merged.hyperlinks.containsKey("hl2"))
    }

    @Test
    fun rebuildGraph_includesHyperlinks() {
        val cache = StarMapSnapshotCache()
        val hl = makeHyperlinkDto(hyperlinkId = "hl1")
        val graphDto = StarMapGraphDto(
            schemaVersion = 1u, id = "sm1", starmapId = "sm1", title = "T",
            nodes = emptyList(), edges = emptyList(), embeds = emptyList(), links = emptyList(),
            hyperlinks = emptyList(),
            createdAt = 0u, updatedAt = 0u
        )
        val initialCache = StarMapRawCache(
            graph = graphDto,
            nodes = mutableMapOf(), edges = mutableMapOf(), embeds = mutableMapOf(),
            links = mutableMapOf(), hyperlinks = mutableMapOf(),
            layoutNodes = mutableMapOf()
        )
        cache.mergeIncremental("sm1", initialCache)

        val incomingCache = StarMapRawCache(
            graph = graphDto,
            nodes = mutableMapOf(), edges = mutableMapOf(), embeds = mutableMapOf(),
            links = mutableMapOf(), hyperlinks = mutableMapOf("hl1" to hl),
            layoutNodes = mutableMapOf()
        )
        cache.mergeIncremental("sm1", incomingCache)

        val graph = cache.get("sm1")!!.graph!!
        assertEquals(1, graph.hyperlinks.size)
        assertEquals("hl1", graph.hyperlinks[0].hyperlinkId)
    }

    @Test
    fun snapshotDto_withHyperlinks_roundtrip_preservesAllFields() {
        val hl = makeHyperlinkDto(hyperlinkId = "hl1", targetUri = "https://example.com", label = "Link")
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 2u, complete = true, sinceRevision = 0u,
            nodes = listOf(StarMapNodeDto(
                id = "n1", title = "A", kind = StarMapNodeKindDto.NOTE,
                payload = null, tags = emptyList(),
                content = StarMapNodeContentDto("note", null, null, null, null, null, null, null, null, null, null, null),
                anchors = emptyList(), portal = null,
                displayPolicy = defaultStarMapDisplayPolicy(),
                openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
                provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
                createdAt = 0u, updatedAt = 0u
            )),
            edges = emptyList(), embeds = emptyList(), links = emptyList(),
            hyperlinks = listOf(hl),
            layout = null, viewport = null, diagnostics = emptyList(),
            deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds,
            deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds
        )
        val rawCache = dto.toRawCache()
        val result = rawCache.toSnapshotResult()
        assertEquals(1, result.data.hyperlinks.size)
        assertEquals("hl1", result.data.hyperlinks[0].hyperlinkId)
        assertEquals("https://example.com", result.data.hyperlinks[0].targetUri)
        assertEquals("Link", result.data.hyperlinks[0].label)
    }
}
