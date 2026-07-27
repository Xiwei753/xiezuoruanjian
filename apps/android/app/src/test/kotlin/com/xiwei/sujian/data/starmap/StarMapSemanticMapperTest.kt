package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapSemanticMapperTest {

    @Test
    fun edgeEndpointDto_toModel_preservesNodeEndpoint() {
        val dto = StarMapEdgeEndpointDto(kind = "Node", nodeId = "n1", anchorId = null, target = null)
        val model = dto.toModel()
        assertEquals("Node", model.kind)
        assertEquals("n1", model.nodeId)
        assertNull(model.anchorId)
        assertNull(model.deepTarget)
    }

    @Test
    fun edgeEndpointDto_toModel_preservesAnchorEndpoint() {
        val dto = StarMapEdgeEndpointDto(kind = "Anchor", nodeId = "n2", anchorId = "a1", target = null)
        val model = dto.toModel()
        assertEquals("Anchor", model.kind)
        assertEquals("n2", model.nodeId)
        assertEquals("a1", model.anchorId)
    }

    @Test
    fun edgeEndpointDto_toModel_preservesDeepTargetEndpoint() {
        val deepTarget = StarMapDeepTargetDto(
            starmapId = "other",
            path = listOf(StarMapPathSegmentDto(kind = "EnterChild", starmapId = "child1")),
            target = StarMapTargetDetailDto(kind = "Node", nodeId = "inner1", anchorId = null,
                projectId = null, volumeId = null, chapterId = null, rangeStart = null, rangeEnd = null,
                entityType = null, entityId = null, uri = null)
        )
        val dto = StarMapEdgeEndpointDto(kind = "DeepTarget", nodeId = null, anchorId = null, target = deepTarget)
        val model = dto.toModel()
        assertEquals("DeepTarget", model.kind)
        assertNotNull(model.deepTarget)
        assertEquals("other", model.deepTarget!!.starmapId)
        assertEquals(1, model.deepTarget!!.path.size)
        assertEquals("EnterChild", model.deepTarget!!.path[0].kind)
        assertEquals("child1", model.deepTarget!!.path[0].starmapId)
        assertEquals("Node", model.deepTarget!!.target.kind)
        assertEquals("inner1", model.deepTarget!!.target.nodeId)
    }

    @Test
    fun endpointPathDto_toModel_preservesSegmentsAndEndpoint() {
        val dto = StarMapEndpointPathDto(
            segments = listOf(StarMapEndpointPathSegmentDto(kind = "EnterChildMap", starmapId = "sm_child")),
            endpoint = StarMapEdgeEndpointDto(kind = "Node", nodeId = "n_inner", anchorId = null, target = null)
        )
        val model = dto.toModel()
        assertEquals(1, model.segments.size)
        assertEquals("EnterChildMap", model.segments[0].kind)
        assertEquals("sm_child", model.segments[0].starmapId)
        assertEquals("Node", model.endpoint.kind)
        assertEquals("n_inner", model.endpoint.nodeId)
    }

    @Test
    fun edgeDto_toGraphEdge_preservesStructuredEndpoints() {
        val dto = StarMapEdgeDto(
            id = "e1", from = "n1", to = "n2", kind = StarMapEdgeKindDto.REFERENCES,
            label = "ref", payload = null,
            fromTarget = null, toTarget = null,
            fromEndpoint = StarMapEdgeEndpointDto(kind = "Node", nodeId = "n1", anchorId = null, target = null),
            toEndpoint = StarMapEdgeEndpointDto(kind = "Anchor", nodeId = "n2", anchorId = "a1", target = null),
            fromEndpointPath = null, toEndpointPath = null,
            createdAt = 0u, updatedAt = 0u
        )
        val model = dto.toGraphEdge()
        assertNotNull(model.fromEndpoint)
        assertEquals("Node", model.fromEndpoint!!.kind)
        assertEquals("n1", model.fromEndpoint!!.nodeId)
        assertNotNull(model.toEndpoint)
        assertEquals("Anchor", model.toEndpoint!!.kind)
        assertEquals("n2", model.toEndpoint!!.nodeId)
        assertEquals("a1", model.toEndpoint!!.anchorId)
    }

    @Test
    fun linkDto_toModel_preservesSourceEndpointAndDeepTarget() {
        val dto = StarMapLinkDto(
            linkId = "lk1",
            source = StarMapEndpointDto(kind = "Anchor", nodeId = "n1", anchorId = "a1"),
            target = StarMapDeepTargetDto(
                starmapId = "other",
                path = emptyList(),
                target = StarMapTargetDetailDto(kind = "Starmap", nodeId = null, anchorId = null,
                    projectId = null, volumeId = null, chapterId = null, rangeStart = null, rangeEnd = null,
                    entityType = null, entityId = null, uri = null)
            ),
            label = "link",
            createdAt = 0u, updatedAt = 0u
        )
        val model = dto.toModel()
        assertEquals("lk1", model.linkId)
        assertEquals("Anchor", model.source.kind)
        assertEquals("n1", model.source.nodeId)
        assertEquals("a1", model.source.anchorId)
        assertNotNull(model.target)
        assertEquals("other", model.target!!.starmapId)
        assertEquals("Starmap", model.target!!.target.kind)
    }

    @Test
    fun hyperlinkDto_toModel_preservesSourcePath() {
        val dto = StarMapHyperlinkDto(
            hyperlinkId = "hl1",
            source = StarMapEndpointPathDto(
                segments = listOf(StarMapEndpointPathSegmentDto(kind = "EnterChildMap", starmapId = "child1")),
                endpoint = StarMapEdgeEndpointDto(kind = "Node", nodeId = "n1", anchorId = null, target = null)
            ),
            targetUri = "https://example.com",
            label = "hl",
            targetStarmapId = "tgt",
            createdAt = 0u, updatedAt = 0u
        )
        val model = dto.toModel()
        assertEquals("hl1", model.hyperlinkId)
        assertNotNull(model.source)
        assertEquals(1, model.source!!.segments.size)
        assertEquals("EnterChildMap", model.source!!.segments[0].kind)
        assertEquals("child1", model.source!!.segments[0].starmapId)
        assertEquals("https://example.com", model.targetUri)
        assertEquals("tgt", model.targetStarmapId)
    }

    @Test
    fun embedDto_toModel_preservesHostEndpointAndSemantics() {
        val dto = StarMapEmbedDto(
            instanceId = "emb1", targetStarmapId = "child", label = "child",
            displayPolicy = defaultStarMapDisplayPolicy(),
            openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
            placement = StarMapEmbedPlacementDto(x = 10.0f, y = 20.0f, width = 200.0f, height = 150.0f, scale = 1.0f, zIndex = 0, collapsed = false),
            targetViewport = StarMapEmbedViewportDto(scale = 1.0f, offsetX = 0.0f, offsetY = 0.0f),
            sourceNodeId = "n1",
            hostEndpoint = StarMapEndpointDto(kind = "Node", nodeId = "n1", anchorId = null),
            provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
            createdAt = 0u, updatedAt = 0u
        )
        val model = dto.toModel()
        assertEquals("emb1", model.instanceId)
        assertEquals("n1", model.sourceNodeId)
        assertNotNull(model.hostEndpoint)
        assertEquals("Node", model.hostEndpoint!!.kind)
        assertEquals("n1", model.hostEndpoint!!.nodeId)
        assertNotNull(model.displayPolicy)
        assertEquals(1.0f, model.displayPolicy!!.importance, 0.001f)
        assertEquals("INSPECTOR", model.openBehavior)
        assertNotNull(model.provenance)
        assertEquals("HUMAN", model.provenance!!.source)
    }

    @Test
    fun nodeDto_toGraphNode_preservesAnchorSemantics() {
        val dto = StarMapNodeDto(
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
        )
        val model = dto.toGraphNode()
        assertEquals(StarMapNodeKind.Character, model.kind)
        assertEquals(listOf("protagonist"), model.tags)
        assertEquals(1, model.anchors.size)
        assertEquals("a1", model.anchors[0].anchorId)
        assertEquals("Chapter", model.anchors[0].targetKind)
        assertEquals("ch1", model.anchors[0].targetChapterId)
        assertEquals("ch_ref", model.anchors[0].label)
        assertEquals("SOURCE", model.anchors[0].role)
    }
}
