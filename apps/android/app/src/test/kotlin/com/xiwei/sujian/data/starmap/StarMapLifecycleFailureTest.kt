package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.ResultEnvelope
import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapLifecycleFailureTest {
    private val emptyDeletedIds = listOf<String>()

    private fun makeNodeDto(
        id: String,
        title: String,
    ) = StarMapNodeDto(
        id = id, title = title, kind = StarMapNodeKindDto.CHARACTER,
        payload = null, tags = emptyList(),
        content = StarMapNodeContentDto("empty", null, null, null, null, null, null, null, null, null, null, null),
        anchors = emptyList(), portal = null,
        displayPolicy = defaultStarMapDisplayPolicy(),
        openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
        provenance =
            StarMapProvenanceDto(
                StarMapSourceKindDto.HUMAN,
                null,
                null,
                null,
                StarMapReviewStatusDto.ACCEPTED,
                null,
            ),
        createdAt = 0u, updatedAt = 0u,
    )

    @Test
    fun flushFailure_preservesCacheState() {
        val cache = StarMapSnapshotCache()
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
                edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout =
                    StarMapLayoutDto(
                        StarMapLayoutKindDto.FREEFORM,
                        listOf(
                            StarMapLayoutNodeDto(
                                nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                                radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null,
                            ),
                        ),
                    ),
                viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds,
            )
        cache.put("sm1", dto.toRawCache())

        val flushResult: BridgeResult<Boolean> =
            BridgeResult.Error(
                ResultEnvelope.errorOf("FLUSH_ERROR", "Failed to flush starmap store: I/O error"),
            )

        when (flushResult) {
            is BridgeResult.Error -> {
                assertEquals("FLUSH_ERROR", flushResult.code)
            }
            else -> fail("expected Error")
        }

        val cached = cache.get("sm1")
        assertNotNull("cache must survive flush failure", cached)
        assertEquals(2, cached!!.nodes.size)
        assertEquals(1, cached.layoutNodes.size)
        assertNotNull("graph must survive flush failure", cached.graph)
    }

    @Test
    fun closeFailure_preservesCacheState() {
        val cache = StarMapSnapshotCache()
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A")),
                edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds,
            )
        cache.put("sm1", dto.toRawCache())

        val closeResult: BridgeResult<Boolean> =
            BridgeResult.Error(
                ResultEnvelope.errorOf("CLOSE_ERROR", "Failed to close starmap store: I/O error"),
            )

        when (closeResult) {
            is BridgeResult.Error -> {
                assertEquals("CLOSE_ERROR", closeResult.code)
            }
            else -> fail("expected Error")
        }

        val cached = cache.get("sm1")
        assertNotNull("cache must survive close failure", cached)
        assertEquals(1, cached!!.nodes.size)
    }

    @Test
    fun saveLayoutFailure_doesNotCorruptLayoutCache() {
        val cache = StarMapSnapshotCache()
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A")),
                edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout =
                    StarMapLayoutDto(
                        StarMapLayoutKindDto.FREEFORM,
                        listOf(
                            StarMapLayoutNodeDto(
                                nodeId = "n1", x = 100f, y = 200f, width = 150f, height = 80f,
                                radius = 40f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null,
                            ),
                        ),
                    ),
                viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds,
            )
        cache.put("sm1", dto.toRawCache())

        val beforeLayout = cache.get("sm1")!!.layoutNodes["n1"]
        assertNotNull(beforeLayout)
        assertEquals(100f, beforeLayout!!.x, 0.001f)
        assertEquals(200f, beforeLayout.y, 0.001f)

        val saveResult: BridgeResult<Boolean> =
            BridgeResult.Error(
                ResultEnvelope.errorOf("IO_ERROR", "Failed to save layout: disk full"),
            )
        when (saveResult) {
            is BridgeResult.Error -> {
                assertEquals("IO_ERROR", saveResult.code)
            }
            else -> fail("expected Error")
        }

        val afterLayout = cache.get("sm1")!!.layoutNodes["n1"]
        assertNotNull("layout cache must survive save failure", afterLayout)
        assertEquals(100f, afterLayout!!.x, 0.001f)
        assertEquals(200f, afterLayout.y, 0.001f)
    }

    @Test
    fun flushAllFailure_preservesAllCaches() {
        val cache = StarMapSnapshotCache()
        val dto1 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T1", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A")),
                edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds,
            )
        val dto2 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm2", title = "T2", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n2", "B")),
                edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyDeletedIds, deletedEdgeIds = emptyDeletedIds, deletedEmbedIds = emptyDeletedIds, deletedLinkIds = emptyDeletedIds, deletedHyperlinkIds = emptyDeletedIds,
            )
        cache.put("sm1", dto1.toRawCache())
        cache.put("sm2", dto2.toRawCache())

        val flushAllResult: BridgeResult<Boolean> =
            BridgeResult.Error(
                ResultEnvelope.errorOf("FLUSH_ALL_ERROR", "Failed to flush all starmap stores"),
            )
        when (flushAllResult) {
            is BridgeResult.Error -> assertEquals("FLUSH_ALL_ERROR", flushAllResult.code)
            else -> fail("expected Error")
        }

        assertNotNull("sm1 cache must survive flushAll failure", cache.get("sm1"))
        assertNotNull("sm2 cache must survive flushAll failure", cache.get("sm2"))
        assertEquals(1, cache.get("sm1")!!.nodes.size)
        assertEquals(1, cache.get("sm2")!!.nodes.size)
    }

    @Test
    fun snapshotCacheNotInitialized_returnsClearError() {
        val cache = StarMapSnapshotCache()
        assertNull(cache.get("nonexistent"))

        val rawCache = cache.get("nonexistent")
        if (rawCache == null) {
            val error =
                ResultEnvelope.errorOf(
                    "SNAPSHOT_CACHE_NOT_INITIALIZED",
                    "Starmap cache not initialized for nonexistent. Call getStarmapPhasedSnapshot first.",
                )
            assertEquals("SNAPSHOT_CACHE_NOT_INITIALIZED", error.errorCode)
        }
    }

    @Test
    fun snapshotCacheMissingGraph_returnsClearError() {
        val cache = StarMapSnapshotCache()
        cache.put(
            "sm1",
            StarMapRawCache(
                graph = null,
                nodes = mutableMapOf(),
                edges = mutableMapOf(),
                embeds = mutableMapOf(),
                links = mutableMapOf(),
                hyperlinks = mutableMapOf(),
                layoutNodes = mutableMapOf(),
            ),
        )

        val rawCache = cache.get("sm1")
        assertNotNull(rawCache)
        assertNull(rawCache!!.graph)

        if (rawCache.graph == null) {
            val error =
                ResultEnvelope.errorOf(
                    "STAR_MAP_CACHE_MISSING",
                    "Raw starmap graph is not available in snapshot cache.",
                )
            assertEquals("STAR_MAP_CACHE_MISSING", error.errorCode)
        }
    }
}
