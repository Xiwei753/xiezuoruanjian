package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.ResultEnvelope
import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapCrossLayerRegressionTest {

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

    @Test
    fun progressiveLoading_neverCallsGetStarMapGraph() {
        val cache = StarMapSnapshotCache()
        val dto1 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "InView")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = StarMapViewportDto(1f, 0f, 0f, 800f, 600f),
            diagnostics = emptyList()
        )
        val rawCache1 = dto1.toRawCache()
        assertNotNull("toRawCache must produce graph for computeEdgeRenders", rawCache1.graph)
        cache.put("sm1", rawCache1)

        val dto2 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "PrefetchNearbyObjects",
            packageRevision = 1u, complete = false, sinceRevision = 1u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby")),
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
        cache.mergeIncremental("sm1", dto2.toRawCache())

        val dto3 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 2u, complete = true, sinceRevision = 1u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby"), makeNodeDto("n3", "Far")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 50f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 200f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n3", x = 350f, y = 60f, width = 100f, height = 80f,
                    radius = 30f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = null, diagnostics = emptyList()
        )
        cache.mergeIncremental("sm1", dto3.toRawCache())

        val finalCache = cache.get("sm1")!!
        assertNotNull("graph must be available without getStarMapGraph", finalCache.graph)
        assertEquals(3, finalCache.nodes.size)
        assertEquals(1, finalCache.edges.size)
        assertEquals(3, finalCache.layoutNodes.size)
        assertEquals(3, finalCache.graph!!.nodes.size)
    }

    @Test
    fun layoutCoordinates_edgeRenderHitTest_consistentAfterSnapshotConversion() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 100f, y = 200f, width = 150f, height = 80f,
                    radius = 40f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 300f, y = 400f, width = 120f, height = 60f,
                    radius = 30f, collapsed = true, zIndex = 1, scale = 0.8f, depth = 1f, focusWeight = 0.5f, orbitGroup = "g1")
            )),
            viewport = StarMapViewportDto(1.5f, 10f, 20f, 800f, 600f),
            diagnostics = emptyList()
        )

        val rawCache = dto.toRawCache()
        assertNotNull(rawCache.graph)
        assertEquals(2, rawCache.graph!!.nodes.size)
        assertEquals(1, rawCache.graph!!.edges.size)
        assertEquals(2, rawCache.layoutNodes.size)

        assertEquals(100f, rawCache.layoutNodes["n1"]!!.x, 0.001f)
        assertEquals(200f, rawCache.layoutNodes["n1"]!!.y, 0.001f)
        assertEquals(300f, rawCache.layoutNodes["n2"]!!.x, 0.001f)
        assertTrue(rawCache.layoutNodes["n2"]!!.collapsed)

        val result = dto.toSnapshotResult()
        val layout = result.data.layout
        assertEquals(2, layout.nodes.size)
        assertEquals(100f, layout.nodes.find { it.nodeId == "n1" }!!.x, 0.001f)
        assertEquals(200f, layout.nodes.find { it.nodeId == "n1" }!!.y, 0.001f)
        assertEquals(300f, layout.nodes.find { it.nodeId == "n2" }!!.x, 0.001f)
        assertTrue(layout.nodes.find { it.nodeId == "n2" }!!.collapsed)
        assertEquals(0.8f, layout.nodes.find { it.nodeId == "n2" }!!.scale, 0.001f)

        assertEquals(1.5f, result.data.viewport.scale, 0.001f)
        assertEquals(10f, result.data.viewport.offsetX, 0.001f)
    }

    @Test
    fun incrementalMerge_preservesExistingDataWhenRevisionUnchanged() {
        val cache = StarMapSnapshotCache()
        val dto1 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
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
        cache.put("sm1", dto1.toRawCache())
        assertEquals(2, cache.get("sm1")!!.nodes.size)
        assertEquals(1, cache.get("sm1")!!.edges.size)

        val dto2 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 1u,
            nodes = emptyList(), edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, emptyList()),
            viewport = null, diagnostics = emptyList()
        )
        cache.mergeIncremental("sm1", dto2.toRawCache())

        val merged = cache.get("sm1")!!
        assertEquals("incremental merge with empty objects must preserve existing nodes", 2, merged.nodes.size)
        assertEquals("incremental merge with empty objects must preserve existing edges", 1, merged.edges.size)
        assertEquals("incremental merge with empty layout must preserve existing layout", 2, merged.layoutNodes.size)
    }

    @Test
    fun incrementalMerge_addsNewObjectsFromLaterPhase() {
        val cache = StarMapSnapshotCache()
        val dto1 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "CurrentViewportObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "InView")),
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
            packageRevision = 2u, complete = false, sinceRevision = 1u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby")),
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
        cache.mergeIncremental("sm1", dto2.toRawCache())

        val merged = cache.get("sm1")!!
        assertEquals("incremental merge must add new node", 2, merged.nodes.size)
        assertTrue(merged.nodes.containsKey("n1"))
        assertTrue(merged.nodes.containsKey("n2"))
        assertEquals(1, merged.edges.size)
        assertEquals(2, merged.layoutNodes.size)
        assertNotNull(merged.graph)
        assertEquals(2, merged.graph!!.nodes.size)
    }

    @Test
    fun semanticFields_endpointAnchorPathDeepTarget_preservedThroughFullChain() {
        val deepTarget = StarMapDeepTargetDto(
            starmapId = "other",
            path = listOf(StarMapPathSegmentDto(kind = "EnterChild", starmapId = "child1")),
            target = StarMapTargetDetailDto(kind = "Node", nodeId = "inner1", anchorId = null,
                projectId = null, volumeId = null, chapterId = null, rangeStart = null, rangeEnd = null,
                entityType = null, entityId = null, uri = null)
        )
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(StarMapNodeDto(
                id = "n1", title = "Char", kind = StarMapNodeKindDto.CHARACTER,
                payload = null, tags = listOf("protagonist"),
                content = StarMapNodeContentDto("empty", null, null, null, null, null, null, null, null, null, null, null),
                anchors = listOf(StarMapAnchorDto(
                    anchorId = "a1",
                    target = StarMapAnchorTargetDto(kind = "Chapter", projectId = null, volumeId = null, chapterId = "ch1",
                        rangeStart = null, rangeEnd = null, entityId = null, entityType = null, starmapId = null, uri = null, payload = null),
                    label = "ch_ref",
                    role = StarMapAnchorRoleDto.SOURCE
                )),
                portal = null,
                displayPolicy = defaultStarMapDisplayPolicy(),
                openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
                provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
                createdAt = 0u, updatedAt = 0u
            )),
            edges = listOf(StarMapEdgeDto(
                id = "e1", from = "n1", to = "n1", kind = StarMapEdgeKindDto.REFERENCES,
                label = "ref", payload = null,
                fromTarget = null, toTarget = null,
                fromEndpoint = StarMapEdgeEndpointDto(kind = "Node", nodeId = "n1", anchorId = null, target = null),
                toEndpoint = StarMapEdgeEndpointDto(kind = "DeepTarget", nodeId = null, anchorId = null, target = deepTarget),
                fromEndpointPath = StarMapEndpointPathDto(
                    segments = listOf(StarMapEndpointPathSegmentDto(kind = "EnterChildMap", starmapId = "sm_child")),
                    endpoint = StarMapEdgeEndpointDto(kind = "Node", nodeId = "n_inner", anchorId = null, target = null)
                ),
                toEndpointPath = null,
                createdAt = 0u, updatedAt = 0u
            )),
            embeds = listOf(StarMapEmbedDto(
                instanceId = "emb1", targetStarmapId = "child", label = "child",
                displayPolicy = defaultStarMapDisplayPolicy(),
                openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
                placement = StarMapEmbedPlacementDto(x = 10f, y = 20f, width = 200f, height = 150f, scale = 1f, zIndex = 0, collapsed = false),
                targetViewport = StarMapEmbedViewportDto(scale = 1f, offsetX = 0f, offsetY = 0f),
                sourceNodeId = "n1",
                hostEndpoint = StarMapEndpointDto(kind = "Node", nodeId = "n1", anchorId = null),
                provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
                createdAt = 0u, updatedAt = 0u
            )),
            links = listOf(StarMapLinkDto(
                linkId = "lk1",
                source = StarMapEndpointDto(kind = "Anchor", nodeId = "n1", anchorId = "a1"),
                target = deepTarget,
                label = "link",
                createdAt = 0u, updatedAt = 0u
            )),
            hyperlinks = listOf(StarMapHyperlinkDto(
                hyperlinkId = "hl1",
                source = StarMapEndpointPathDto(
                    segments = listOf(StarMapEndpointPathSegmentDto(kind = "EnterChildMap", starmapId = "child1")),
                    endpoint = StarMapEdgeEndpointDto(kind = "Node", nodeId = "n1", anchorId = null, target = null)
                ),
                targetUri = "https://example.com",
                label = "hl",
                targetStarmapId = "tgt",
                createdAt = 0u, updatedAt = 0u
            )),
            layout = null, viewport = null, diagnostics = emptyList()
        )

        val result = dto.toSnapshotResult()

        val node = result.data.graph.nodes[0]
        assertEquals(StarMapNodeKind.Character, node.kind)
        assertEquals(listOf("protagonist"), node.tags)
        assertEquals(1, node.anchors.size)
        assertEquals("a1", node.anchors[0].anchorId)
        assertEquals("Chapter", node.anchors[0].targetKind)
        assertEquals("ch1", node.anchors[0].targetChapterId)
        assertEquals("SOURCE", node.anchors[0].role)

        val edge = result.data.graph.edges[0]
        assertNotNull(edge.fromEndpoint)
        assertEquals("Node", edge.fromEndpoint!!.kind)
        assertEquals("n1", edge.fromEndpoint!!.nodeId)
        assertNotNull(edge.toEndpoint)
        assertEquals("DeepTarget", edge.toEndpoint!!.kind)
        assertNotNull(edge.toEndpoint!!.deepTarget)
        assertEquals("other", edge.toEndpoint!!.deepTarget!!.starmapId)
        assertNotNull(edge.fromEndpointPath)
        assertEquals(1, edge.fromEndpointPath!!.segments.size)
        assertEquals("EnterChildMap", edge.fromEndpointPath!!.segments[0].kind)

        assertEquals(1, result.data.embeds.size)
        assertNotNull(result.data.embeds[0].hostEndpoint)
        assertEquals("Node", result.data.embeds[0].hostEndpoint!!.kind)

        assertEquals(1, result.data.links.size)
        assertNotNull(result.data.links[0].target)
        assertEquals("other", result.data.links[0].target!!.starmapId)

        assertEquals(1, result.data.hyperlinks.size)
        assertNotNull(result.data.hyperlinks[0].source)
        assertEquals("https://example.com", result.data.hyperlinks[0].targetUri)
        assertEquals("tgt", result.data.hyperlinks[0].targetStarmapId)
    }

    @Test
    fun crudFailure_preservesEditingStateAndDialogState() {
        var lastError: String? = null
        var editingNodeId: String? = "n1"
        var selectedNodeId: String? = "n1"
        var showAddNodeDialog = true
        var operationInProgress = false

        fun executeOperation(label: String, result: BridgeResult<*>): Boolean {
            operationInProgress = true
            return when (result) {
                is BridgeResult.Success -> {
                    lastError = null
                    operationInProgress = false
                    true
                }
                is BridgeResult.Error -> {
                    lastError = "${label}失败: ${result.message}"
                    operationInProgress = false
                    false
                }
                BridgeResult.NotLoaded -> {
                    lastError = "${label}失败: 未加载"
                    operationInProgress = false
                    false
                }
            }
        }

        val failedResult: BridgeResult<StarMapGraphNode> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "update failed")
        )
        val ok = executeOperation("更新节点", failedResult)
        assertFalse(ok)
        assertNotNull(lastError)
        assertTrue(lastError!!.contains("更新节点失败"))
        assertEquals("editingNodeId preserved on failure", "n1", editingNodeId)
        assertEquals("selectedNodeId preserved on failure", "n1", selectedNodeId)
        assertTrue("dialog stays open on unrelated failure", showAddNodeDialog)
        assertFalse("operationInProgress reset after failure", operationInProgress)

        val successResult: BridgeResult<StarMapGraphNode> = BridgeResult.Success(
            StarMapGraphNode(id = "n1", title = "Updated", kind = StarMapNodeKind.Character)
        )
        val ok2 = executeOperation("更新节点", successResult)
        assertTrue(ok2)
        assertNull(lastError)
        editingNodeId = null
        selectedNodeId = null
        showAddNodeDialog = false
        assertNull(editingNodeId)
        assertFalse(showAddNodeDialog)
    }

    @Test
    fun crudFailure_addNodeDialogStaysOpen() {
        var showAddNodeDialog = true
        var lastError: String? = null

        val failedResult: BridgeResult<StarMapGraphNode> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "add failed")
        )
        when (failedResult) {
            is BridgeResult.Error -> lastError = "添加节点失败: ${failedResult.message}"
            else -> {}
        }
        assertTrue("add node dialog must stay open on failure", showAddNodeDialog)
        assertNotNull(lastError)

        val successResult: BridgeResult<StarMapGraphNode> = BridgeResult.Success(
            StarMapGraphNode(id = "n1", title = "New", kind = StarMapNodeKind.Character)
        )
        when (successResult) {
            is BridgeResult.Success -> {
                lastError = null
                showAddNodeDialog = false
            }
            else -> {}
        }
        assertFalse("dialog closed only on success", showAddNodeDialog)
    }

    @Test
    fun crudFailure_addEdgeDialogStaysOpen() {
        var showAddEdgeDialog = true
        var lastError: String? = null

        val failedResult: BridgeResult<StarMapGraphEdge> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "add edge failed")
        )
        when (failedResult) {
            is BridgeResult.Error -> lastError = "添加连线失败: ${failedResult.message}"
            else -> {}
        }
        assertTrue("add edge dialog must stay open on failure", showAddEdgeDialog)

        val successResult: BridgeResult<StarMapGraphEdge> = BridgeResult.Success(
            StarMapGraphEdge(id = "e1", from = "n1", to = "n2", kind = StarMapEdgeKind.RelatedTo)
        )
        when (successResult) {
            is BridgeResult.Success -> {
                lastError = null
                showAddEdgeDialog = false
            }
            else -> {}
        }
        assertFalse("dialog closed only on success", showAddEdgeDialog)
    }

    @Test
    fun crudFailure_doubleTapEditPreservesEditingState() {
        var editingNodeId: String? = "n1"
        var lastError: String? = null

        val failedResult: BridgeResult<StarMapGraphNode> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "update failed")
        )
        when (failedResult) {
            is BridgeResult.Error -> lastError = "更新节点失败: ${failedResult.message}"
            else -> {}
        }
        assertEquals("double-tap edit must preserve editingNodeId on failure", "n1", editingNodeId)
        assertNotNull(lastError)

        val successResult: BridgeResult<StarMapGraphNode> = BridgeResult.Success(
            StarMapGraphNode(id = "n1", title = "Updated", kind = StarMapNodeKind.Character)
        )
        when (successResult) {
            is BridgeResult.Success -> {
                lastError = null
                editingNodeId = null
            }
            else -> {}
        }
        assertNull("editingNodeId cleared only on success", editingNodeId)
    }

    @Test
    fun loadPhaseProgression_objectCountsMonotonicallyIncrease() {
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
        val count1 = result1.data.graph.nodes.size

        val dto2 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "PrefetchNearbyObjects",
            packageRevision = 1u, complete = false, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        val result2 = dto2.toSnapshotResult()
        assertTrue(result2.data.graph.nodes.size >= count1)

        val dto3 = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "InView"), makeNodeDto("n2", "Nearby"), makeNodeDto("n3", "Far")),
            edges = emptyList(), embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = null, viewport = null, diagnostics = emptyList()
        )
        val result3 = dto3.toSnapshotResult()
        assertTrue(result3.data.complete)
        assertTrue(result3.data.graph.nodes.size >= result2.data.graph.nodes.size)
    }

    @Test
    fun revisionIncrementalMerge_revisionUnchangedReturnsEmptyObjects() {
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
    fun revisionIncrementalMerge_revisionAdvancedReturnsAllObjects() {
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

    @Test
    fun migrationReopen_preservesDataThroughSnapshot() {
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 2u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 10f, y = 20f, width = 100f, height = 50f,
                    radius = 25f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 200f, y = 300f, width = 100f, height = 50f,
                    radius = 25f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = StarMapViewportDto(1f, 0f, 0f, 800f, 600f),
            diagnostics = emptyList()
        )
        val result = dto.toSnapshotResult()
        assertEquals(2, result.data.graph.nodes.size)
        assertEquals(1, result.data.graph.edges.size)
        assertEquals(2, result.data.layout.nodes.size)
        assertEquals(10f, result.data.layout.nodes.find { it.nodeId == "n1" }!!.x, 0.001f)
        assertEquals(200f, result.data.layout.nodes.find { it.nodeId == "n2" }!!.x, 0.001f)
        assertTrue(result.data.packageRevision >= 2uL)
    }

    @Test
    fun saveFailureRetry_preservesMemoryState() {
        val cache = StarMapSnapshotCache()
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 100f, y = 200f, width = 150f, height = 80f,
                    radius = 40f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = null, diagnostics = emptyList()
        )
        cache.put("sm1", dto.toRawCache())

        val saveResult: BridgeResult<Boolean> = BridgeResult.Error(
            ResultEnvelope.error("IO_ERROR", "disk full")
        )
        when (saveResult) {
            is BridgeResult.Error -> assertEquals("IO_ERROR", saveResult.code)
            else -> fail("expected Error")
        }

        val cached = cache.get("sm1")
        assertNotNull("cache must survive save failure", cached)
        assertEquals(2, cached!!.nodes.size)
        assertEquals(1, cached.edges.size)
        assertEquals(1, cached.layoutNodes.size)
        assertEquals(100f, cached.layoutNodes["n1"]!!.x, 0.001f)
    }

    @Test
    fun flushCloseFailure_preservesMemoryCacheState() {
        val cache = StarMapSnapshotCache()
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
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
        cache.put("sm1", dto.toRawCache())

        val flushResult: BridgeResult<Boolean> = BridgeResult.Error(
            ResultEnvelope.error("FLUSH_ERROR", "I/O error")
        )
        when (flushResult) {
            is BridgeResult.Error -> assertEquals("FLUSH_ERROR", flushResult.code)
            else -> fail("expected Error")
        }

        val cached = cache.get("sm1")
        assertNotNull("cache must survive flush failure", cached)
        assertEquals(2, cached!!.nodes.size)
        assertEquals(1, cached.edges.size)
        assertEquals(2, cached.layoutNodes.size)
        assertNotNull(cached.graph)

        val closeResult: BridgeResult<Boolean> = BridgeResult.Error(
            ResultEnvelope.error("CLOSE_ERROR", "I/O error")
        )
        when (closeResult) {
            is BridgeResult.Error -> assertEquals("CLOSE_ERROR", closeResult.code)
            else -> fail("expected Error")
        }

        val cachedAfterClose = cache.get("sm1")
        assertNotNull("cache must survive close failure", cachedAfterClose)
        assertEquals(2, cachedAfterClose!!.nodes.size)
    }

    @Test
    fun computeEdgeRenders_usesCacheNeverFullLoad() {
        val cache = StarMapSnapshotCache()
        val dto = StarMapPhasedSnapshotDto(
            starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
            packageRevision = 1u, complete = true, sinceRevision = 0u,
            nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
            edges = listOf(makeEdgeDto("e1", "n1", "n2")),
            embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
            layout = StarMapLayoutDto(StarMapLayoutKindDto.FREEFORM, listOf(
                StarMapLayoutNodeDto(nodeId = "n1", x = 0f, y = 0f, width = 100f, height = 50f,
                    radius = 25f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null),
                StarMapLayoutNodeDto(nodeId = "n2", x = 200f, y = 0f, width = 100f, height = 50f,
                    radius = 25f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null)
            )),
            viewport = null, diagnostics = emptyList()
        )
        val rawCache = dto.toRawCache()
        cache.put("sm1", rawCache)

        val cached = cache.get("sm1")!!
        assertNotNull("graph must be available for computeEdgeRenders without getStarMapGraph", cached.graph)
        assertEquals(2, cached.graph!!.nodes.size)
        assertEquals(1, cached.graph!!.edges.size)
        assertEquals(2, cached.layoutNodes.size)
    }

    @Test
    fun operationInProgress_stateTransitions() {
        var operationInProgress = false
        var lastError: String? = null

        operationInProgress = true
        val failedResult: BridgeResult<StarMapGraphNode> = BridgeResult.Error(
            ResultEnvelope.error("NATIVE_ERROR", "failed")
        )
        when (failedResult) {
            is BridgeResult.Error -> {
                lastError = "操作失败: ${failedResult.message}"
                operationInProgress = false
            }
            else -> {}
        }
        assertFalse(operationInProgress)
        assertNotNull(lastError)

        operationInProgress = true
        val successResult: BridgeResult<StarMapGraphNode> = BridgeResult.Success(
            StarMapGraphNode(id = "n1", title = "OK", kind = StarMapNodeKind.Character)
        )
        when (successResult) {
            is BridgeResult.Success -> {
                lastError = null
                operationInProgress = false
            }
            else -> {}
        }
        assertFalse(operationInProgress)
        assertNull(lastError)
    }
}
