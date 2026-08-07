package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapSnapshotConsistencyTest {
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
    fun layoutCoordinates_preservedThroughDtoToRawCacheToModel() {
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
                edges = listOf(makeEdgeDto("e1", "n1", "n2")),
                embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout =
                    StarMapLayoutDto(
                        StarMapLayoutKindDto.FREEFORM,
                        listOf(
                            StarMapLayoutNodeDto(
                                nodeId = "n1", x = 100.0f, y = 200.0f, width = 150.0f, height = 80.0f,
                                radius = 40.0f, collapsed = false, zIndex = 0, scale = 1.0f, depth = 0.0f, focusWeight = 1.0f, orbitGroup = null,
                            ),
                            StarMapLayoutNodeDto(
                                nodeId = "n2", x = 300.0f, y = 400.0f, width = 120.0f, height = 60.0f,
                                radius = 30.0f, collapsed = true, zIndex = 1, scale = 0.8f, depth = 1.0f, focusWeight = 0.5f, orbitGroup = "group1",
                            ),
                        ),
                    ),
                viewport = StarMapViewportDto(scale = 1.5f, offsetX = 10f, offsetY = 20f, width = 800f, height = 600f),
                diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )

        val rawCache = dto.toRawCache()
        assertEquals(2, rawCache.layoutNodes.size)
        assertTrue(rawCache.layoutNodes.containsKey("n1"))
        assertTrue(rawCache.layoutNodes.containsKey("n2"))
        assertEquals(100.0f, rawCache.layoutNodes["n1"]!!.x, 0.001f)
        assertEquals(200.0f, rawCache.layoutNodes["n1"]!!.y, 0.001f)
        assertEquals(150.0f, rawCache.layoutNodes["n1"]!!.width, 0.001f)
        assertEquals(300.0f, rawCache.layoutNodes["n2"]!!.x, 0.001f)
        assertTrue(rawCache.layoutNodes["n2"]!!.collapsed)
        assertEquals(0.8f, rawCache.layoutNodes["n2"]!!.scale, 0.001f)
        assertEquals("group1", rawCache.layoutNodes["n2"]!!.orbitGroup)

        val result = dto.toSnapshotResult()
        val layout = result.data.layout
        assertEquals(2, layout.nodes.size)
        val n1Layout = layout.nodes.find { it.nodeId == "n1" }!!
        assertEquals(100.0f, n1Layout.x, 0.001f)
        assertEquals(200.0f, n1Layout.y, 0.001f)
        assertEquals(150.0f, n1Layout.width, 0.001f)
        assertEquals(40.0f, n1Layout.radius, 0.001f)
        val n2Layout = layout.nodes.find { it.nodeId == "n2" }!!
        assertEquals(300.0f, n2Layout.x, 0.001f)
        assertTrue(n2Layout.collapsed)
        assertEquals(0.8f, n2Layout.scale, 0.001f)
        assertEquals(1.0f, n2Layout.depth, 0.001f)
        assertEquals(0.5f, n2Layout.focusWeight, 0.001f)
        assertEquals("group1", n2Layout.orbitGroup)
    }

    @Test
    fun edgeRenderData_availableFromCacheForComputeEdgeRenders() {
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes = listOf(makeNodeDto("n1", "A"), makeNodeDto("n2", "B")),
                edges = listOf(makeEdgeDto("e1", "n1", "n2")),
                embeds = emptyList(), links = emptyList(), hyperlinks = emptyList(),
                layout =
                    StarMapLayoutDto(
                        StarMapLayoutKindDto.FREEFORM,
                        listOf(
                            StarMapLayoutNodeDto(
                                nodeId = "n1", x = 0f, y = 0f, width = 100f, height = 50f,
                                radius = 25f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null,
                            ),
                            StarMapLayoutNodeDto(
                                nodeId = "n2", x = 200f, y = 0f, width = 100f, height = 50f,
                                radius = 25f, collapsed = false, zIndex = 0, scale = 1f, depth = 0f, focusWeight = 1f, orbitGroup = null,
                            ),
                        ),
                    ),
                viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )

        val rawCache = dto.toRawCache()
        assertNotNull("graph must be available for computeEdgeRenders", rawCache.graph)
        assertEquals(1, rawCache.graph!!.edges.size)
        assertEquals("e1", rawCache.graph!!.edges[0].id)
        assertEquals(2, rawCache.layoutNodes.size)
    }

    @Test
    fun incrementalSnapshot_preservesLayoutAndViewport() {
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 5u, complete = true, sinceRevision = 5u,
                nodes = emptyList(), edges = emptyList(), embeds = emptyList(),
                links = emptyList(), hyperlinks = emptyList(),
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
                viewport = StarMapViewportDto(2f, 10f, 20f, 800f, 600f),
                diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
            )

        val result = dto.toSnapshotResult()
        assertEquals(0, result.data.graph.nodes.size)
        assertEquals(1, result.data.layout.nodes.size)
        assertEquals(50f, result.data.layout.nodes[0].x, 0.001f)
        assertEquals(60f, result.data.layout.nodes[0].y, 0.001f)
        assertEquals(2f, result.data.viewport.scale, 0.001f)
        assertEquals(10f, result.data.viewport.offsetX, 0.001f)
    }

    @Test
    fun semanticFields_preservedThroughFullChain() {
        val deepTarget =
            StarMapDeepTargetDto(
                starmapId = "other",
                path = listOf(StarMapPathSegmentDto(kind = "EnterChild", starmapId = "child1")),
                target =
                    StarMapTargetDetailDto(
                        kind = "Node", nodeId = "inner1", anchorId = null,
                        projectId = null, volumeId = null, chapterId = null, rangeStart = null, rangeEnd = null,
                        entityType = null, entityId = null, uri = null,
                    ),
            )
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
                nodes =
                    listOf(
                        StarMapNodeDto(
                            id = "n1", title = "Char", kind = StarMapNodeKindDto.CHARACTER,
                            payload = null, tags = listOf("protagonist"),
                            content = StarMapNodeContentDto("empty", null, null, null, null, null, null, null, null, null, null, null),
                            anchors =
                                listOf(
                                    StarMapAnchorDto(
                                        anchorId = "a1",
                                        target =
                                            StarMapAnchorTargetDto(
                                                kind = "Chapter", projectId = null, volumeId = null, chapterId = "ch1",
                                                rangeStart = null, rangeEnd = null, entityId = null, entityType = null, starmapId = null, uri = null, payload = null,
                                            ),
                                        label = "ch_ref",
                                        role = StarMapAnchorRoleDto.SOURCE,
                                    ),
                                ),
                            portal = null,
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
                        ),
                    ),
                edges =
                    listOf(
                        StarMapEdgeDto(
                            id = "e1", from = "n1", to = "n1", kind = StarMapEdgeKindDto.REFERENCES,
                            label = "ref", payload = null,
                            fromTarget = null, toTarget = null,
                            fromEndpoint =
                                StarMapEdgeEndpointDto(
                                    kind = "Node",
                                    nodeId = "n1",
                                    anchorId = null,
                                    target = null,
                                ),
                            toEndpoint =
                                StarMapEdgeEndpointDto(
                                    kind = "DeepTarget",
                                    nodeId = null,
                                    anchorId = null,
                                    target = deepTarget,
                                ),
                            fromEndpointPath =
                                StarMapEndpointPathDto(
                                    segments =
                                        listOf(
                                            StarMapEndpointPathSegmentDto(
                                                kind = "EnterChildMap",
                                                starmapId = "sm_child",
                                            ),
                                        ),
                                    endpoint =
                                        StarMapEdgeEndpointDto(
                                            kind = "Node",
                                            nodeId = "n_inner",
                                            anchorId = null,
                                            target = null,
                                        ),
                                ),
                            toEndpointPath = null,
                            createdAt = 0u, updatedAt = 0u,
                        ),
                    ),
                embeds =
                    listOf(
                        StarMapEmbedDto(
                            instanceId = "emb1", targetStarmapId = "child", label = "child",
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
                            source = StarMapEndpointDto(kind = "Anchor", nodeId = "n1", anchorId = "a1"),
                            target = deepTarget,
                            label = "link",
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
                                    segments =
                                        listOf(
                                            StarMapEndpointPathSegmentDto(kind = "EnterChildMap", starmapId = "child1"),
                                        ),
                                    endpoint =
                                        StarMapEdgeEndpointDto(
                                            kind = "Node",
                                            nodeId = "n1",
                                            anchorId = null,
                                            target = null,
                                        ),
                                ),
                            targetUri = "https://example.com",
                            label = "hl",
                            targetStarmapId = "tgt",
                            createdAt = 0u,
                            updatedAt = 0u,
                        ),
                    ),
                layout = null, viewport = null, diagnostics = emptyList(),
                deletedNodeIds = emptyList(), deletedEdgeIds = emptyList(), deletedEmbedIds = emptyList(), deletedLinkIds = emptyList(), deletedHyperlinkIds = emptyList(),
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
        val embed = result.data.embeds[0]
        assertEquals("emb1", embed.instanceId)
        assertNotNull(embed.hostEndpoint)
        assertEquals("Node", embed.hostEndpoint!!.kind)

        assertEquals(1, result.data.links.size)
        val link = result.data.links[0]
        assertEquals("lk1", link.linkId)
        assertEquals("Anchor", link.source.kind)
        assertNotNull(link.target)
        assertEquals("other", link.target!!.starmapId)

        assertEquals(1, result.data.hyperlinks.size)
        val hl = result.data.hyperlinks[0]
        assertEquals("hl1", hl.hyperlinkId)
        assertNotNull(hl.source)
        assertEquals(1, hl.source!!.segments.size)
        assertEquals("https://example.com", hl.targetUri)
        assertEquals("tgt", hl.targetStarmapId)
    }

    @Test
    fun rawCache_toSnapshotResult_matchesDto_toSnapshotResult() {
        val dto =
            StarMapPhasedSnapshotDto(
                starmapId = "sm1", title = "T", loadPhase = "BackgroundFullLoad",
                packageRevision = 1u, complete = true, sinceRevision = 0u,
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
        val fromDto = dto.toSnapshotResult()
        val fromCache = dto.toRawCache().toSnapshotResult()

        assertEquals(fromDto.data.graph.nodes.size, fromCache.data.graph.nodes.size)
        assertEquals(fromDto.data.graph.edges.size, fromCache.data.graph.edges.size)
        assertEquals(fromDto.data.layout.nodes.size, fromCache.data.layout.nodes.size)
        assertEquals(fromDto.data.embeds.size, fromCache.data.embeds.size)
        assertEquals(fromDto.data.links.size, fromCache.data.links.size)
        assertEquals(fromDto.data.hyperlinks.size, fromCache.data.hyperlinks.size)
        assertEquals(fromDto.data.loadPhase, fromCache.data.loadPhase)
        assertEquals(fromDto.data.packageRevision, fromCache.data.packageRevision)
        assertEquals(fromDto.data.complete, fromCache.data.complete)
    }
}
