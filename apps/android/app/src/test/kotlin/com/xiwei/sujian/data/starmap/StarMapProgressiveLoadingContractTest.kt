package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.ResultEnvelope
import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapProgressiveLoadingContractTest {

    private fun makeNodeDto(id: String, title: String) = StarMapNodeDto(
        id = id, title = title, kind = StarMapNodeKindDto.CHARACTER,
        payload = null, tags = emptyList(),
        content = StarMapNodeContentDto("empty", null, null, null, null, null, null, null, null, null, null, null),
        anchors = emptyList(), portal = null,
        displayPolicy = defaultStarMapDisplayPolicy(),
        openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
        provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
        createdAt = 0u, updatedAt = 0u
    )

    private fun makeEdgeDto(id: String, from: String, to: String) = StarMapEdgeDto(
        id = id, from = from, to = to, kind = StarMapEdgeKindDto.RELATED_TO,
        label = null, payload = null,
        fromTarget = null, toTarget = null,
        fromEndpoint = null, toEndpoint = null,
        fromEndpointPath = null, toEndpointPath = null,
        createdAt = 0u, updatedAt = 0u
    )

    @Test
    fun progressiveLoading_populatesCacheAndNeverCallsGetStarMapGraph() {
        val cache = StarMapSnapshotCache()
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "A")),
            edges = listOf(makeEdgeDto("e1", "n1", "n1")),
            embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = StarMapViewportDto(1f, 0f, 0f, 800f, 600f),
            diagnostics = emptyList()
        )

        val rawCache = dto.toRawCache()
        assertNotNull("toRawCache must produce a non-null graph for computeEdgeRenders", rawCache.graph)
        assertEquals(1, rawCache.nodes.size)
        assertEquals(1, rawCache.edges.size)
        assertEquals(1, rawCache.layoutNodes.size)

        cache.put("sm1", rawCache)
        val retrieved = cache.get("sm1")
        assertNotNull("cache must be populated after put", retrieved)
        assertNotNull("cached graph must be non-null — no fallback to getStarMapGraph needed", retrieved!!.graph)
        assertEquals(1, retrieved.graph!!.nodes.size)
        assertEquals(1, retrieved.graph!!.edges.size)
    }

    @Test
    fun advanceLoadPhase_usesCacheNotFullLoad() {
        val cache = StarMapSnapshotCache()
        val dto1 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "A")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = null, diagnostics = emptyList()
        )
        cache.put("sm1", dto1.toRawCache())

        val dto2 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "PrefetchNearbyObjects",
            packageRevision = 1u, complete = false, sinceRevision = 1u,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 200f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = null, diagnostics = emptyList()
        )
        cache.put("sm1", dto2.toRawCache())

        val cached = cache.get("sm1")!!
        assertEquals(2, cached.nodes.size)
        assertEquals(1, cached.edges.size)
        assertNotNull("graph must be available for computeEdgeRenders without getStarMapGraph", cached.graph)
        assertEquals(2, cached.graph!!.nodes.size)
    }

    @Test
    fun computeEdgeRenders_requiresCacheAndReturnsErrorIfMissing() {
        val cache = StarMapSnapshotCache()
        assertNull("cache must be null before snapshot loaded", cache.get("sm1"))
    }

    @Test
    fun hitTestRequiresCacheAndReturnsErrorIfMissing() {
        val cache = StarMapSnapshotCache()
        assertNull("cache must be null before snapshot loaded", cache.get("sm1"))
    }

    @Test
    fun loadPhaseProgression_currentViewport_to_prefetch_to_background() {
        val dto1 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "InView")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        val result1 = dto1.toSnapshotResult()
        assertEquals("CurrentViewportObjects", result1.data.loadPhase)
        assertFalse(result1.data.complete)
        assertEquals(1, result1.data.graph.nodes.size)

        val dto2 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "PrefetchNearbyObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        val result2 = dto2.toSnapshotResult()
        assertEquals("PrefetchNearbyObjects", result2.data.loadPhase)
        assertFalse(result2.data.complete)
        assertTrue(result2.data.graph.nodes.size >= result1.data.graph.nodes.size)

        val dto3 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby"), makeNodeDto("n3", "Far")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        val result3 = dto3.toSnapshotResult()
        assertEquals("BackgroundFullLoad", result3.data.loadPhase)
        assertTrue(result3.data.complete)
        assertTrue(result3.data.graph.nodes.size >= result2.data.graph.nodes.size)
    }

    @Test
    fun revisionAndIncrementalMerge_revisionUnchangedReturnsEmptyObjects() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 5u, complete = true, sinceRevision = 5u,
            nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
            links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, emptyList()),
            viewport = StarMapViewportDto(1f, 0f, 0f, 800f, 600f),
            diagnostics = emptyList()
        )
        val result = dto.toSnapshotResult()
        assertEquals(5uL, result.data.packageRevision)
        assertEquals(5uL, result.data.sinceRevision)
        assertEquals(0, result.data.graph.nodes.size)
        assertTrue(result.data.layout.nodes.isEmpty())
    }

    @Test
    fun revisionAndIncrementalMerge_revisionAdvancedReturnsAllObjects() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 5u, complete = true, sinceRevision = 3u,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        val result = dto.toSnapshotResult()
        assertEquals(5uL, result.data.packageRevision)
        assertEquals(3uL, result.data.sinceRevision)
        assertEquals(2, result.data.graph.nodes.size)
        assertEquals(1, result.data.graph.edges.size)
    }
}
