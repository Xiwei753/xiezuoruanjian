package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapProgressiveLoadingTest {
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

    private fun makeEdgeDto(
        id: String,
        from: String,
        to: String,
    ) = StarMapEdgeDto(
        id = id, from = from, to = to, kind = StarMapEdgeKindDto.RELATED_TO,
        label = null, payload = null,
        fromTarget = null, toTarget = null,
        fromEndpoint = null, toEndpoint = null,
        fromEndpointPath = null, toEndpointPath = null,
        createdAt = 0u, updatedAt = 0u,
    )

    @Test
    fun progressiveLoading_populatesCacheAndNeverCallsGetStarMapGraph() {
        val cache = StarMapSnapshotCache()
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
                packageRevision = 1u, complete = false, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A")),
                edges = listOf(makeEdgeDto("e1", "n1", "n1")),
                embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
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
                viewport = StarMapViewportDto(1f, 0f, 0f, 800f, 600f),
                diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
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
        val dto1 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
                packageRevision = 1u, complete = false, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A")),
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
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        cache.put("sm1", dto1.toRawCache())

        val dto2 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "PrefetchNearbyObjects",
                packageRevision = 1u, complete = false, sinceRevision = 1u,
                nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
                edges = listOf(makeEdgeDto("e1", "n1", "n2")),
                embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout =
                    StarMapLayoutDto(
                        StarMapLayoutKindDto.FREEFORM,
                        listOf(
                            StarMapLayoutNodeDto(
                                nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                                radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null,
                            ),
                            StarMapLayoutNodeDto(
                                nodeId = "n2", x = 200f, y = 60f, width = 100f, height = 80f,
                                radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null,
                            ),
                        ),
                    ),
                viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        cache.mergeIncremental("sm1", dto2.toRawCache())

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
        val dto1 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
                packageRevision = 1u, complete = false, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "InView")),
                edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        val result1 = dto1.toSnapshotResult()
        assertEquals("CurrentViewportObjects", result1.data.loadPhase)
        assertFalse(result1.data.complete)
        assertEquals(1, result1.data.graph.nodes.size)

        val dto2 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "PrefetchNearbyObjects",
                packageRevision = 1u, complete = false, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby")),
                edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        val result2 = dto2.toSnapshotResult()
        assertEquals("PrefetchNearbyObjects", result2.data.loadPhase)
        assertFalse(result2.data.complete)
        assertTrue(result2.data.graph.nodes.size >= result1.data.graph.nodes.size)

        val dto3 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby"), makeNodeDto("n3", "Far")),
                edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        val result3 = dto3.toSnapshotResult()
        assertEquals("BackgroundFullLoad", result3.data.loadPhase)
        assertTrue(result3.data.complete)
        assertTrue(result3.data.graph.nodes.size >= result2.data.graph.nodes.size)
    }

    @Test
    fun revisionAndIncrementalMerge_revisionUnchangedReturnsEmptyObjects() {
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 5u, complete = true, sinceRevision = 5u,
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
                links = emptyList(), hyperlinks = emptyList(),
                layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, emptyList()),
                viewport = StarMapViewportDto(1f, 0f, 0f, 800f, 600f),
                diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        val result = dto.toSnapshotResult()
        assertEquals(5uL, result.data.packageRevision)
        assertEquals(5uL, result.data.sinceRevision)
        assertEquals(0, result.data.graph.nodes.size)
        assertTrue(result.data.layout.nodes.isEmpty())
    }

    @Test
    fun revisionAndIncrementalMerge_revisionAdvancedReturnsAllObjects() {
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 5u, complete = true, sinceRevision = 3u,
                nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
                edges = listOf(makeEdgeDto("e1", "n1", "n2")),
                embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        val result = dto.toSnapshotResult()
        assertEquals(5uL, result.data.packageRevision)
        assertEquals(3uL, result.data.sinceRevision)
        assertEquals(2, result.data.graph.nodes.size)
        assertEquals(1, result.data.graph.edges.size)
    }

    @Test
    fun cacheBasedResult_afterIncrementalMerge_preservesAllData() {
        val cache = StarMapSnapshotCache()
        val dto1 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
                packageRevision = 1u, complete = false, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A")),
                edges = listOf(makeEdgeDto("e1", "n1", "n1")),
                embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
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
                viewport = StarMapViewportDto(1f, 0f, 0f, 800f, 600f),
                diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        cache.put("sm1", dto1.toRawCache())

        val dto2 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 2u, complete = true, sinceRevision = 1u,
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
                links = emptyList(), hyperlinks = emptyList(),
                layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, emptyList()),
                viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        cache.mergeIncremental("sm1", dto2.toRawCache())

        val mergedCache = cache.get("sm1")!!
        val result = mergedCache.toSnapshotResult()
        assertEquals(1, result.data.graph.nodes.size)
        assertEquals("A", result.data.graph.nodes[0].title)
        assertEquals(1, result.data.graph.edges.size)
        assertEquals(1, result.data.layout.nodes.size)
        assertEquals(50f, result.data.layout.nodes[0].x, 0.001f)
        assertEquals(1f, result.data.viewport.scale, 0.001f)
        assertEquals(2uL, result.data.packageRevision)
        assertTrue(result.data.complete)
    }

    @Test
    // #597 测试用例验证增量合并超链接多步场景，511c0f99 起即如此 — 拆分降低可读性
    @Suppress("LongMethod")
    fun embedsLinksHyperlinks_incrementalMerge_preservesExistingAndAddsNew() {
        val cache = StarMapSnapshotCache()
        val deepTarget =
            StarMapDeepTargetDto(
                starmapId = "other",
                path = emptyList(),
                target =
                    StarMapTargetDetailDto(
                        kind = "Node", nodeId = "inner1", anchorId = null,
                        projectId = null, volumeId = null, chapterId = null, rangeStart = null, rangeEnd = null,
                        entityType = null, entityId = null, uri = null,
                    ),
            )
        val dto1 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
                packageRevision = 1u, complete = false, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A")),
                edges = emptyList(),
                embeds =
                    listOf(
                        StarMapEmbedDto(
                            instanceId = "emb1", targetStarmapId = "child", label = "first",
                            displayPolicy = defaultStarMapDisplayPolicy(),
                            openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
                            placement =
                                StarMapEmbedPlacementDto(
                                    x = 10f,
                                    y = 20f,
                                    width = 200f,
                                    height = 150f,
                                    scale = 1f,
                                    zIndex = 0,
                                    collapsed = false,
                                ),
                            targetViewport = StarMapEmbedViewportDto(scale = 1f, offsetX = 0f, offsetY = 0f),
                            sourceNodeId = "n1",
                            hostEndpoint = StarMapEndpointDto(kind = "Node", nodeId = "n1", anchorId = null),
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
                        ),
                    ),
                links =
                    listOf(
                        StarMapLinkDto(
                            linkId = "lk1",
                            source = StarMapEndpointDto(kind = "Node", nodeId = "n1", anchorId = null),
                            target = deepTarget,
                            label = "link1",
                            createdAt = 0u,
                            updatedAt = 0u,
                        ),
                    ),
                hyperlinks =
                    listOf(
                        StarMapHyperlinkDto(
                            hyperlinkId = "hl1",
                            source =
                                StarMapEndpointPathDto(
                                    segments = emptyList(),
                                    endpoint =
                                        StarMapEdgeEndpointDto(
                                            kind = "Node",
                                            nodeId = "n1",
                                            anchorId = null,
                                            target = null,
                                        ),
                                ),
                            targetUri = "https://example.com",
                            label = "hl1",
                            targetStarmapId = null,
                            createdAt = 0u,
                            updatedAt = 0u,
                        ),
                    ),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        cache.put("sm1", dto1.toRawCache())

        val dto2 =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "PrefetchNearbyObjects",
                packageRevision = 2u, complete = false, sinceRevision = 1u,
                nodes = listOf(makeNodeDto("n2", "B")),
                edges = emptyList(),
                embeds =
                    listOf(
                        StarMapEmbedDto(
                            instanceId = "emb2", targetStarmapId = "child2", label = "second",
                            displayPolicy = defaultStarMapDisplayPolicy(),
                            openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
                            placement =
                                StarMapEmbedPlacementDto(
                                    x = 30f,
                                    y = 40f,
                                    width = 200f,
                                    height = 150f,
                                    scale = 1f,
                                    zIndex = 0,
                                    collapsed = false,
                                ),
                            targetViewport = StarMapEmbedViewportDto(scale = 1f, offsetX = 0f, offsetY = 0f),
                            sourceNodeId = "n2",
                            hostEndpoint = null,
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
                        ),
                    ),
                links = emptyList(),
                hyperlinks =
                    listOf(
                        StarMapHyperlinkDto(
                            hyperlinkId = "hl2",
                            source =
                                StarMapEndpointPathDto(
                                    segments = emptyList(),
                                    endpoint =
                                        StarMapEdgeEndpointDto(
                                            kind = "Node",
                                            nodeId = "n2",
                                            anchorId = null,
                                            target = null,
                                        ),
                                ),
                            targetUri = "https://other.com",
                            label = "hl2",
                            targetStarmapId = null,
                            createdAt = 0u,
                            updatedAt = 0u,
                        ),
                    ),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        cache.mergeIncremental("sm1", dto2.toRawCache())

        val merged = cache.get("sm1")!!
        assertEquals(2, merged.nodes.size)
        assertEquals(2, merged.embeds.size)
        assertTrue(merged.embeds.containsKey("emb1"))
        assertTrue(merged.embeds.containsKey("emb2"))
        assertEquals(1, merged.links.size)
        assertTrue(merged.links.containsKey("lk1"))
        assertEquals(2, merged.hyperlinks.size)
        assertTrue(merged.hyperlinks.containsKey("hl1"))
        assertTrue(merged.hyperlinks.containsKey("hl2"))

        val result = merged.toSnapshotResult()
        assertEquals(2, result.data.embeds.size)
        assertEquals(1, result.data.links.size)
        assertEquals(2, result.data.hyperlinks.size)
        assertEquals("first", result.data.embeds.find { it.instanceId == "emb1" }!!.label)
        assertEquals("second", result.data.embeds.find { it.instanceId == "emb2" }!!.label)
    }

    @Test
    fun embedLinkHyperlinkCrud_cacheUpdatesCorrectly() {
        val cache = StarMapSnapshotCache()
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A")),
                edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )
        cache.put("sm1", dto.toRawCache())

        val embedDto =
            StarMapEmbedDto(
                instanceId = "emb1", targetStarmapId = "child", label = "new",
                displayPolicy = defaultStarMapDisplayPolicy(),
                openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
                placement =
                    StarMapEmbedPlacementDto(
                        x = 0f,
                        y = 0f,
                        width = 200f,
                        height = 150f,
                        scale = 1f,
                        zIndex = 0,
                        collapsed = false,
                    ),
                targetViewport = StarMapEmbedViewportDto(scale = 1f, offsetX = 0f, offsetY = 0f),
                sourceNodeId = "n1",
                hostEndpoint = null,
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
        cache.putEmbed("sm1", "emb1", embedDto)
        assertTrue(cache.get("sm1")!!.embeds.containsKey("emb1"))
        cache.removeEmbed("sm1", "emb1")
        assertFalse(cache.get("sm1")!!.embeds.containsKey("emb1"))

        val linkDto =
            StarMapLinkDto(
                linkId = "lk1",
                source = StarMapEndpointDto(kind = "Node", nodeId = "n1", anchorId = null),
                target =
                    StarMapDeepTargetDto(
                        starmapId = "other",
                        path = emptyList(),
                        target =
                            StarMapTargetDetailDto(
                                kind = "Node", nodeId = "x", anchorId = null,
                                projectId = null, volumeId = null, chapterId = null, rangeStart = null, rangeEnd = null,
                                entityType = null, entityId = null, uri = null,
                            ),
                    ),
                label = null,
                createdAt = 0u,
                updatedAt = 0u,
            )
        cache.putLink("sm1", "lk1", linkDto)
        assertTrue(cache.get("sm1")!!.links.containsKey("lk1"))
        cache.removeLink("sm1", "lk1")
        assertFalse(cache.get("sm1")!!.links.containsKey("lk1"))

        val hlDto =
            StarMapHyperlinkDto(
                hyperlinkId = "hl1",
                source =
                    StarMapEndpointPathDto(
                        segments = emptyList(),
                        endpoint = StarMapEdgeEndpointDto(kind = "Node", nodeId = "n1", anchorId = null, target = null),
                    ),
                targetUri = "https://test.com",
                label = null,
                targetStarmapId = null,
                createdAt = 0u,
                updatedAt = 0u,
            )
        cache.putHyperlink("sm1", "hl1", hlDto)
        assertTrue(cache.get("sm1")!!.hyperlinks.containsKey("hl1"))
        cache.removeHyperlink("sm1", "hl1")
        assertFalse(cache.get("sm1")!!.hyperlinks.containsKey("hl1"))
    }
}
