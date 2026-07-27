package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapSnapshotMapperTest {

    @Test
    fun phasedSnapshotDto_toSnapshotResult_preservesLayoutCoordinates() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1",
            title = "Test",
            loadPhase = "BackgroundFullLoad",
            packageRevision = 5u,
            complete = true,
            sinceRevision = 0u,
            nodes = listOf(
                StarMapNodeDto(
                    id = "n1", title = "A", kind = StarMapNodeKindDto.CHARACTER,
                    payload = null, tags = emptyList(),
                    content = StarMapNodeContentDto("note", null, null, null, null, null, null, null, null, null, null, null),
                    anchors = emptyList(), portal = null,
                    displayPolicy = defaultStarMapDisplayPolicy(),
                    openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
                    provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
                    createdAt = 0u, updatedAt = 0u
                )
            ),
            edges = emptyList(),
            embeds = emptyList(),
            links = emptyList(),
            hyperlinks = emptyList(),
            layout = StarMapLayoutDto(
                kind = StarMapLayoutKindDto.FREEFORM,
                nodes = listOf(StarMapLayoutNodeDto(
                    nodeId = "n1", x = 100.0f, y = 200.0f, width = 150.0f, height = 80.0f,
                    radius = 40.0f, collapsed = false, zIndex = 0, scale = 1.0f,
                    depth = 0.0f, focusWeight = 1.0f, orbitGroup = null
                ))
            ),
            viewport = StarMapViewportDto(scale = 1.0f, offsetX = 0.0f, offsetY = 0.0f, width = 800.0f, height = 600.0f),
            diagnostics = emptyList()
        )

        val result = dto.toSnapshotResult()
        val layout = result.data.layout
        assertEquals(1, layout.nodes.size)
        assertEquals("n1", layout.nodes[0].nodeId)
        assertEquals(100.0f, layout.nodes[0].x, 0.001f)
        assertEquals(200.0f, layout.nodes[0].y, 0.001f)
        assertEquals(150.0f, layout.nodes[0].width, 0.001f)
    }

    @Test
    fun phasedSnapshotDto_toSnapshotResult_preservesSinceRevision() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "PrefetchNearbyObjects",
            packageRevision = 10u, complete = false, sinceRevision = 7u,
            nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
            links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        val result = dto.toSnapshotResult()
        assertEquals(7uL, result.data.sinceRevision)
        assertEquals(10uL, result.data.packageRevision)
    }

    @Test
    fun phasedSnapshotDto_toSnapshotResult_incrementalEmptyObjects() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 5u, complete = true, sinceRevision = 5u,
            nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
            links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, emptyList()),
            viewport = StarMapViewportDto(1.0f, 0.0f, 0.0f, 800.0f, 600.0f),
            diagnostics = emptyList()
        )
        val result = dto.toSnapshotResult()
        assertEquals(0, result.data.graph.nodes.size)
        assertTrue(result.data.layout.nodes.isEmpty())
    }

    @Test
    fun phasedSnapshotDto_toRawCache_buildsGraphDto() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 3u, complete = false, sinceRevision = 0u,
            nodes = listOf(
                StarMapNodeDto(
                    id = "n1", title = "A", kind = StarMapNodeKindDto.CHARACTER,
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
            layout = null, viewport = null, diagnostics = emptyList()
        )
        val cache = dto.toRawCache()
        assertNotNull(cache.graph)
        assertEquals("sm1", cache.graph!!.starmapId)
        assertEquals(1, cache.nodes.size)
        assertTrue(cache.nodes.containsKey("n1"))
    }
}
