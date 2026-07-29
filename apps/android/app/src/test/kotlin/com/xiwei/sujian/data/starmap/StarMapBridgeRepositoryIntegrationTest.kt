package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.ResultEnvelope
import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapBridgeRepositoryIntegrationTest {

    private var getStarMapGraphCallCount = 0

    private fun makeNodeDto(id: String, title: String, kind: StarMapNodeKindDto = StarMapNodeKindDto.CHARACTER) = StarMapNodeDto(
        id = id, title = title, kind = kind,
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

    private fun makePhasedSnapshotDto(
        starmapId: String = "sm1",
        loadPhase: String = "CurrentViewportObjects",
        packageRevision: ULong = 1u,
        complete: Boolean = false,
        sinceRevision: ULong = 0u,
        nodes: List<StarMapNodeDto> = emptyList(),
        edges: List<StarMapEdgeDto> = emptyList(),
        layout: StarMapLayoutDto? = null,
        viewport: StarMapViewportDto? = null,
    ) = StarMapPhasedSnapshotDto(
        starmapId = starmapId, title = "T", loadPhase = loadPhase,
        packageRevision = packageRevision, complete = complete, sinceRevision = sinceRevision,
        nodes = nodes, edges = edges, embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
        layout = layout, viewport = viewport, diagnostics = emptyList(),
        deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList()
    )

    private fun createFakeBridge(
        phasedSnapshotResults: MutableMap<String, StarMapPhasedSnapshotDto> = mutableMapOf(),
        addNodeResult: (() -> BridgeResult<StarMapNodeDto>)? = null,
        updateNodeResult: (() -> BridgeResult<StarMapNodeDto>)? = null,
        deleteNodeResult: (() -> BridgeResult<Boolean>)? = null,
        addEdgeResult: (() -> BridgeResult<StarMapEdgeDto>)? = null,
    ): StarMapBridgeOps = object : StarMapBridgeOps {
        override fun listStarMaps(): BridgeResult<List<StarMapMetaDto>> = BridgeResult.Success(emptyList())
        override fun getStarMapGraph(starmapId: String): BridgeResult<StarMapGraphDto> {
            getStarMapGraphCallCount++
            return BridgeResult.Error(ResultEnvelope.error("DEPRECATED", "getStarMapGraph should not be called"))
        }
        override fun createStarMap(title: String, desc: String): BridgeResult<StarMapMetaDto> =
            BridgeResult.Success(StarMapMetaDto("sm1", title, desc, "", null, false, "", 0u, 0u, 0u, 0u, 0u, 0u))
        override fun addStarMapNode(starmapId: String, node: StarMapNodeDto, x: Float, y: Float): BridgeResult<StarMapNodeDto> =
            addNodeResult?.invoke() ?: BridgeResult.Success(node)
        override fun saveStarMapLayout(starmapId: String, layout: StarMapLayoutDto): BridgeResult<Boolean> = BridgeResult.Success(true)
        override fun getStarMapViewport(starmapId: String): BridgeResult<StarMapViewportDto> =
            BridgeResult.Success(StarMapViewportDto(1f, 0f, 0f, 800f, 600f))
        override fun saveStarMapViewport(starmapId: String, viewport: StarMapViewportDto): BridgeResult<Boolean> = BridgeResult.Success(true)
        override fun computeStarMapEdgeRenders(graph: StarMapGraphDto, layout: StarMapLayoutDto): BridgeResult<List<StarMapEdgeRenderDto>> =
            BridgeResult.Success(emptyList())
        override fun hitTestStarMapNode(layout: StarMapLayoutDto, x: Float, y: Float): BridgeResult<String?> = BridgeResult.Success(null)
        override fun addStarmapEmbed(starmapId: String, embed: StarMapEmbedDto): BridgeResult<StarMapEmbedDto> = BridgeResult.Success(embed)
        override fun updateStarmapEmbed(starmapId: String, instanceId: String, patch: StarMapEmbedPatchInputDto): BridgeResult<StarMapEmbedDto> =
            BridgeResult.Error(ResultEnvelope.error("NOT_IMPLEMENTED", "not implemented"))
        override fun deleteStarmapEmbed(starmapId: String, instanceId: String): BridgeResult<Boolean> = BridgeResult.Success(true)
        override fun addStarmapLink(starmapId: String, link: StarMapLinkDto): BridgeResult<StarMapLinkDto> = BridgeResult.Success(link)
        override fun updateStarmapLink(starmapId: String, linkId: String, patch: StarMapLinkPatchInputDto): BridgeResult<StarMapLinkDto> =
            BridgeResult.Error(ResultEnvelope.error("NOT_IMPLEMENTED", "not implemented"))
        override fun deleteStarmapLink(starmapId: String, linkId: String): BridgeResult<Boolean> = BridgeResult.Success(true)
        override fun addStarmapHyperlink(starmapId: String, hl: StarMapHyperlinkDto): BridgeResult<StarMapHyperlinkDto> = BridgeResult.Success(hl)
        override fun updateStarmapHyperlink(starmapId: String, hyperlinkId: String, patch: StarMapHyperlinkPatchInputDto): BridgeResult<StarMapHyperlinkDto> =
            BridgeResult.Error(ResultEnvelope.error("NOT_IMPLEMENTED", "not implemented"))
        override fun deleteStarmapHyperlink(starmapId: String, hyperlinkId: String): BridgeResult<Boolean> = BridgeResult.Success(true)
        override fun listStarmapHyperlinks(starmapId: String): BridgeResult<StarMapHyperlinkListWithDiagnosticsDto> =
            BridgeResult.Success(StarMapHyperlinkListWithDiagnosticsDto(emptyList(), emptyList()))
        override fun getStarmapPhasedSnapshot(starmapId: String, request: PhasedSnapshotRequestDto): BridgeResult<StarMapPhasedSnapshotDto> {
            val dto = phasedSnapshotResults[starmapId]
                ?: return BridgeResult.Error(ResultEnvelope.error("NOT_FOUND", "starmap not found"))
            return BridgeResult.Success(dto)
        }
        override fun findStarmapReferences(targetStarmapId: String): BridgeResult<List<StarMapReferenceDto>> = BridgeResult.Success(emptyList())
        override fun getStarMapMotionPolicy(): BridgeResult<StarMapMotionPolicyDto> =
            BridgeResult.Success(StarMapMotionPolicyDto(false, false, 1f, 3000u, 1f, 5f, 100u, false))
        override fun updateStarMapNode(starmapId: String, nodeId: String, patch: StarMapNodePatchInputDto): BridgeResult<StarMapNodeDto> =
            updateNodeResult?.invoke() ?: BridgeResult.Error(ResultEnvelope.error("NOT_IMPLEMENTED", "not implemented"))
        override fun deleteStarMapNode(starmapId: String, nodeId: String): BridgeResult<Boolean> =
            deleteNodeResult?.invoke() ?: BridgeResult.Success(true)
        override fun addStarMapEdge(starmapId: String, edge: StarMapEdgeDto): BridgeResult<StarMapEdgeDto> =
            addEdgeResult?.invoke() ?: BridgeResult.Success(edge)
        override fun deleteStarMapEdge(starmapId: String, edgeId: String): BridgeResult<Boolean> = BridgeResult.Success(true)
        override fun updateStarMapEdge(starmapId: String, edgeId: String, patch: StarMapEdgePatchInputDto): BridgeResult<StarMapEdgeDto> =
            BridgeResult.Error(ResultEnvelope.error("NOT_IMPLEMENTED", "not implemented"))
        override fun flushStarmapStore(starmapId: String): BridgeResult<Boolean> = BridgeResult.Success(true)
        override fun closeStarmapStore(starmapId: String): BridgeResult<Boolean> = BridgeResult.Success(true)
        override fun flushAllStarmapStores(): BridgeResult<Boolean> = BridgeResult.Success(true)
    }

    @Test
    fun progressiveLoading_neverCallsGetStarMapGraph() {
        getStarMapGraphCallCount = 0
        val dto1 = makePhasedSnapshotDto(
            loadPhase = "CurrentViewportObjects",
            nodes = listOf(makeNodeDto("n1", "InView")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = StarMapViewportDto(1f, 0f, 0f, 800f, 600f)
        )
        val bridge = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto1))
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        val result = repo.getStarmapPhasedSnapshot("sm1")
        assertTrue("phased snapshot must succeed", result is BridgeResult.Success)
        assertEquals("getStarMapGraph must not be called during progressive loading", 0, getStarMapGraphCallCount)

        val data = (result as BridgeResult.Success).data.data
        assertEquals(1, data.graph.nodes.size)
        assertEquals("CurrentViewportObjects", data.loadPhase)
        assertFalse(data.complete)
    }

    @Test
    fun advanceLoadPhase_usesCacheAndNeverCallsGetStarMapGraph() {
        getStarMapGraphCallCount = 0
        val dto1 = makePhasedSnapshotDto(
            loadPhase = "CurrentViewportObjects", packageRevision = 1u,
            nodes = listOf(makeNodeDto("n1", "InView")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            ))
        )
        val dto2 = makePhasedSnapshotDto(
            loadPhase = "PrefetchNearbyObjects", packageRevision = 2u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 200f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            ))
        )
        val bridge = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto2))
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        val result1 = repo.getStarmapPhasedSnapshot("sm1")
        assertTrue(result1 is BridgeResult.Success)

        val result2 = repo.advanceLoadPhase("sm1", "PrefetchNearbyObjects", 1u)
        assertTrue("advance must succeed", result2 is BridgeResult.Success)
        assertEquals("getStarMapGraph must not be called during advance", 0, getStarMapGraphCallCount)

        val data = (result2 as BridgeResult.Success).data.data
        assertEquals(2, data.graph.nodes.size)
    }

    @Test
    fun computeEdgeRenders_usesCacheNotFullLoad() {
        getStarMapGraphCallCount = 0
        val dto = makePhasedSnapshotDto(
            loadPhase = "BackgroundFullLoad", complete = true,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 0f, y = 0f, width = 100f, height = 50f,
                    radius = 25f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 200f, y = 0f, width = 100f, height = 50f,
                    radius = 25f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            ))
        )
        val bridge = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto))
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        val snapshotResult = repo.getStarmapPhasedSnapshot("sm1")
        assertTrue(snapshotResult is BridgeResult.Success)
        val data = (snapshotResult as BridgeResult.Success).data.data

        val edgeRenders = repo.computeEdgeRenders(data)
        assertTrue("computeEdgeRenders must succeed from cache", edgeRenders is BridgeResult.Success)
        assertEquals("getStarMapGraph must not be called for computeEdgeRenders", 0, getStarMapGraphCallCount)
    }

    @Test
    fun computeEdgeRenders_withoutCache_returnsError() {
        getStarMapGraphCallCount = 0
        val bridge = createFakeBridge()
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        val data = StarMapData(
            graph = StarMapGraphData(schemaVersion = 1, id = "sm1", starmapId = "sm1", title = "T",
                nodes = emptyList(), edges = emptyList(), createdAt = 0, updatedAt = 0),
            layout = StarMapLayoutData(kind = StarMapLayoutKind.Freeform, nodes = emptyList())
        )
        val result = repo.computeEdgeRenders(data)
        assertTrue("computeEdgeRenders must return error without cache", result is BridgeResult.Error)
        assertEquals("getStarMapGraph must not be called even when cache missing", 0, getStarMapGraphCallCount)
    }

    @Test
    fun incrementalMerge_throughRepository_preservesExistingAndAddsNew() {
        getStarMapGraphCallCount = 0
        val dto1 = makePhasedSnapshotDto(
            loadPhase = "CurrentViewportObjects", packageRevision = 1u,
            nodes = listOf(makeNodeDto("n1", "InView")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            ))
        )
        val dto2 = makePhasedSnapshotDto(
            loadPhase = "PrefetchNearbyObjects", packageRevision = 2u, sinceRevision = 1u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 200f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            ))
        )
        val results = mutableMapOf("sm1" to dto1)
        val bridge = createFakeBridge(phasedSnapshotResults = results)
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        val result1 = repo.getStarmapPhasedSnapshot("sm1")
        assertTrue(result1 is BridgeResult.Success)
        assertEquals(1, (result1 as BridgeResult.Success).data.data.graph.nodes.size)

        results["sm1"] = dto2
        val result2 = repo.getStarmapPhasedSnapshot("sm1", sinceRevision = 1u)
        assertTrue(result2 is BridgeResult.Success)
        val merged = (result2 as BridgeResult.Success).data.data
        assertEquals("incremental merge must add new node", 2, merged.graph.nodes.size)
        assertEquals("incremental merge must add new edge", 1, merged.graph.edges.size)
        assertEquals(0, getStarMapGraphCallCount)
    }

    @Test
    fun crudAddNode_success_updatesCache() {
        getStarMapGraphCallCount = 0
        val dto = makePhasedSnapshotDto(
            loadPhase = "BackgroundFullLoad", complete = true,
            nodes = listOf(makeNodeDto("n1", "A")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            ))
        )
        val bridge = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto))
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        repo.getStarmapPhasedSnapshot("sm1")
        assertEquals(1, cache.get("sm1")!!.nodes.size)

        val newNode = StarMapGraphNode(id = "n2", title = "B", kind = StarMapNodeKind.Event)
        val addResult = repo.addStarmapNode("sm1", newNode)
        assertTrue("addNode must succeed", addResult is BridgeResult.Success)
        assertEquals("cache must contain new node after successful add", 2, cache.get("sm1")!!.nodes.size)
        assertTrue(cache.get("sm1")!!.nodes.containsKey("n2"))
        assertEquals(0, getStarMapGraphCallCount)
    }

    @Test
    fun crudAddNode_failure_doesNotCorruptCache() {
        getStarMapGraphCallCount = 0
        val dto = makePhasedSnapshotDto(
            loadPhase = "BackgroundFullLoad", complete = true,
            nodes = listOf(makeNodeDto("n1", "A"))
        )
        val bridge = createFakeBridge(
            phasedSnapshotResults = mutableMapOf("sm1" to dto),
            addNodeResult = { BridgeResult.Error(ResultEnvelope.error("NATIVE_ERROR", "add failed")) }
        )
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        repo.getStarmapPhasedSnapshot("sm1")
        assertEquals(1, cache.get("sm1")!!.nodes.size)

        val newNode = StarMapGraphNode(id = "n2", title = "B", kind = StarMapNodeKind.Event)
        val addResult = repo.addStarmapNode("sm1", newNode)
        assertTrue("addNode must fail", addResult is BridgeResult.Error)
        assertEquals("cache must not change after failed add", 1, cache.get("sm1")!!.nodes.size)
        assertEquals(0, getStarMapGraphCallCount)
    }

    @Test
    fun crudDeleteNode_success_updatesCache() {
        getStarMapGraphCallCount = 0
        val dto = makePhasedSnapshotDto(
            loadPhase = "BackgroundFullLoad", complete = true,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B"))
        )
        val bridge = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto))
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        repo.getStarmapPhasedSnapshot("sm1")
        assertEquals(2, cache.get("sm1")!!.nodes.size)

        val deleteResult = repo.deleteStarmapNode("sm1", "n1")
        assertTrue("deleteNode must succeed", deleteResult is BridgeResult.Success)
        assertEquals("cache must remove node after successful delete", 1, cache.get("sm1")!!.nodes.size)
        assertFalse(cache.get("sm1")!!.nodes.containsKey("n1"))
        assertEquals(0, getStarMapGraphCallCount)
    }

    @Test
    fun crudDeleteNode_failure_doesNotCorruptCache() {
        getStarMapGraphCallCount = 0
        val dto = makePhasedSnapshotDto(
            loadPhase = "BackgroundFullLoad", complete = true,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B"))
        )
        val bridge = createFakeBridge(
            phasedSnapshotResults = mutableMapOf("sm1" to dto),
            deleteNodeResult = { BridgeResult.Error(ResultEnvelope.error("NATIVE_ERROR", "delete failed")) }
        )
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        repo.getStarmapPhasedSnapshot("sm1")
        assertEquals(2, cache.get("sm1")!!.nodes.size)

        val deleteResult = repo.deleteStarmapNode("sm1", "n1")
        assertTrue("deleteNode must fail", deleteResult is BridgeResult.Error)
        assertEquals("cache must not change after failed delete", 2, cache.get("sm1")!!.nodes.size)
        assertEquals(0, getStarMapGraphCallCount)
    }

    @Test
    fun crudUpdateNode_success_updatesCache() {
        getStarMapGraphCallCount = 0
        val dto = makePhasedSnapshotDto(
            loadPhase = "BackgroundFullLoad", complete = true,
            nodes = listOf(makeNodeDto("n1", "Original"))
        )
        val updatedDto = makeNodeDto("n1", "Updated")
        val bridge = createFakeBridge(
            phasedSnapshotResults = mutableMapOf("sm1" to dto),
            updateNodeResult = { BridgeResult.Success(updatedDto) }
        )
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        repo.getStarmapPhasedSnapshot("sm1")
        assertEquals("Original", cache.get("sm1")!!.nodes["n1"]!!.title)

        val updateResult = repo.updateStarmapNode("sm1", "n1", title = "Updated")
        assertTrue("updateNode must succeed", updateResult is BridgeResult.Success)
        assertEquals("Updated", cache.get("sm1")!!.nodes["n1"]!!.title)
        assertEquals(0, getStarMapGraphCallCount)
    }

    @Test
    fun layoutCoordinates_preservedThroughRepositoryChain() {
        getStarMapGraphCallCount = 0
        val dto = makePhasedSnapshotDto(
            loadPhase = "BackgroundFullLoad", complete = true,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 100f, y = 200f, width = 150f, height = 80f,
                    radius = 40f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 300f, y = 400f, width = 120f, height = 60f,
                    radius = 30f, collapsed = true, zIndex = 1, scale = 0.8f, depth = 1f, focusWeight = 0.5f, orbitGroup = "g1")
            )),
            viewport = StarMapViewportDto(1.5f, 10f, 20f, 800f, 600f)
        )
        val bridge = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto))
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        val result = repo.getStarmapPhasedSnapshot("sm1")
        assertTrue(result is BridgeResult.Success)
        val data = (result as BridgeResult.Success).data.data

        assertEquals(2, data.layout.nodes.size)
        assertEquals(100f, data.layout.nodes.find { it.nodeId == "n1" }!!.x, 0.001f)
        assertEquals(200f, data.layout.nodes.find { it.nodeId == "n1" }!!.y, 0.001f)
        assertEquals(300f, data.layout.nodes.find { it.nodeId == "n2" }!!.x, 0.001f)
        assertTrue(data.layout.nodes.find { it.nodeId == "n2" }!!.collapsed)
        assertEquals(0.8f, data.layout.nodes.find { it.nodeId == "n2" }!!.scale, 0.001f)
        assertEquals(1.5f, data.viewport.scale, 0.001f)
        assertEquals(10f, data.viewport.offsetX, 0.001f)
        assertEquals(0, getStarMapGraphCallCount)
    }

    @Test
    fun revisionAndPhase_preservedThroughRepositoryChain() {
        getStarMapGraphCallCount = 0
        val dto = makePhasedSnapshotDto(
            loadPhase = "PrefetchNearbyObjects", packageRevision = 5u, complete = false, sinceRevision = 3u,
            nodes = listOf(makeNodeDto("n1", "A"))
        )
        val bridge = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto))
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        val result = repo.getStarmapPhasedSnapshot("sm1")
        assertTrue(result is BridgeResult.Success)
        val data = (result as BridgeResult.Success).data.data

        assertEquals("PrefetchNearbyObjects", data.loadPhase)
        assertEquals(5uL, data.packageRevision)
        assertEquals(3uL, data.sinceRevision)
        assertFalse(data.complete)
        assertEquals(0, getStarMapGraphCallCount)
    }

    @Test
    fun fullProgressiveLoadingSequence_neverCallsGetStarMapGraph() {
        getStarMapGraphCallCount = 0
        val dto1 = makePhasedSnapshotDto(
            loadPhase = "CurrentViewportObjects", packageRevision = 1u,
            nodes = listOf(makeNodeDto("n1", "InView")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = StarMapViewportDto(1f, 0f, 0f, 800f, 600f)
        )
        val dto2 = makePhasedSnapshotDto(
            loadPhase = "PrefetchNearbyObjects", packageRevision = 2u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 200f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            ))
        )
        val dto3 = makePhasedSnapshotDto(
            loadPhase = "BackgroundFullLoad", packageRevision = 3u, complete = true,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby"), makeNodeDto("n3", "Far")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 200f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n3", x = 350f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            ))
        )

        val bridge = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto1))
        val cache = StarMapSnapshotCache()
        val repo = StarMapRepository(bridge, cache)

        val r1 = repo.getStarmapPhasedSnapshot("sm1")
        assertTrue(r1 is BridgeResult.Success)
        assertEquals(1, (r1 as BridgeResult.Success).data.data.graph.nodes.size)

        val bridge2 = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto2))
        val repo2 = StarMapRepository(bridge2, cache)
        val r2 = repo2.getStarmapPhasedSnapshot("sm1", sinceRevision = 1u)
        assertTrue(r2 is BridgeResult.Success)
        assertEquals(2, (r2 as BridgeResult.Success).data.data.graph.nodes.size)

        val bridge3 = createFakeBridge(phasedSnapshotResults = mutableMapOf("sm1" to dto3))
        val repo3 = StarMapRepository(bridge3, cache)
        val r3 = repo3.getStarmapPhasedSnapshot("sm1", targetPhase = "BackgroundFullLoad", sinceRevision = 2u)
        assertTrue(r3 is BridgeResult.Success)
        val finalData = (r3 as BridgeResult.Success).data.data
        assertEquals(3, finalData.graph.nodes.size)
        assertTrue(finalData.complete)

        assertEquals("getStarMapGraph must never be called in full progressive sequence", 0, getStarMapGraphCallCount)
    }
}
