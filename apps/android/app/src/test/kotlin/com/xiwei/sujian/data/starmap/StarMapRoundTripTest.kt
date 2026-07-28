package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.writer_core.*

class StarMapRoundTripTest {

    @Test
    fun dtoToModelToDto_contentFullFields_roundTrip() {
        val originalDto = StarMapNodeDto(
            id = "n1",
            title = "Chapter Node",
            kind = StarMapNodeKindDto.CHAPTER,
            payload = null,
            tags = listOf("tag1", "tag2"),
            content = StarMapNodeContentDto(
                kind = "chapter",
                summary = "summary text",
                body = "body text",
                projectId = "proj1",
                volumeId = "vol1",
                chapterId = "ch1",
                rangeStart = 10u,
                rangeEnd = 200u,
                entityType = "paragraph",
                entityId = "ent1",
                uri = "uri1",
                label = "content label"
            ),
            anchors = emptyList(),
            portal = null,
            displayPolicy = defaultStarMapDisplayPolicy(),
            openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
            provenance = StarMapProvenanceDto(
                source = StarMapSourceKindDto.HUMAN,
                sourceId = null,
                generatedBy = null,
                promptId = null,
                reviewStatus = StarMapReviewStatusDto.ACCEPTED,
                createdFromAnchor = null
            ),
            createdAt = 100u,
            updatedAt = 200u
        )

        val model = originalDto.toGraphNode()
        val roundTrippedDto = model.toDto(null)

        assertEquals("n1", roundTrippedDto.id)
        assertEquals("Chapter Node", roundTrippedDto.title)
        assertEquals(StarMapNodeKindDto.CHAPTER, roundTrippedDto.kind)
        assertEquals(listOf("tag1", "tag2"), roundTrippedDto.tags)
        assertEquals("chapter", roundTrippedDto.content.kind)
        assertEquals("summary text", roundTrippedDto.content.summary)
        assertEquals("body text", roundTrippedDto.content.body)
        assertEquals("proj1", roundTrippedDto.content.projectId)
        assertEquals("vol1", roundTrippedDto.content.volumeId)
        assertEquals("ch1", roundTrippedDto.content.chapterId)
        assertEquals(10u, roundTrippedDto.content.rangeStart)
        assertEquals(200u, roundTrippedDto.content.rangeEnd)
        assertEquals("paragraph", roundTrippedDto.content.entityType)
        assertEquals("ent1", roundTrippedDto.content.entityId)
        assertEquals("uri1", roundTrippedDto.content.uri)
        assertEquals("content label", roundTrippedDto.content.label)
        assertEquals(100u.toULong(), roundTrippedDto.createdAt)
        assertEquals(200u.toULong(), roundTrippedDto.updatedAt)
    }

    @Test
    fun dtoToModelToDto_anchorTargetFullFields_roundTrip() {
        val originalDto = StarMapNodeDto(
            id = "n1",
            title = "Char",
            kind = StarMapNodeKindDto.CHARACTER,
            payload = null,
            tags = emptyList(),
            content = StarMapNodeContentDto("note", null, null, null, null, null, null, null, null, null, null, null),
            anchors = listOf(
                StarMapAnchorDto(
                    anchorId = "a1",
                    target = StarMapAnchorTargetDto(
                        kind = "Chapter",
                        projectId = "projA",
                        volumeId = "volA",
                        chapterId = "chA",
                        rangeStart = 5u,
                        rangeEnd = 50u,
                        entityId = "entA",
                        entityType = "scene",
                        starmapId = "smA",
                        uri = "uriA",
                        payload = "payloadA"
                    ),
                    label = "anchor label",
                    role = StarMapAnchorRoleDto.REFERENCE
                )
            ),
            portal = null,
            displayPolicy = defaultStarMapDisplayPolicy(),
            openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
            provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
            createdAt = 0u,
            updatedAt = 0u
        )

        val model = originalDto.toGraphNode()
        val roundTrippedDto = model.toDto(null)

        assertEquals(1, roundTrippedDto.anchors.size)
        val anchor = roundTrippedDto.anchors[0]
        assertEquals("a1", anchor.anchorId)
        assertEquals("Chapter", anchor.target.kind)
        assertEquals("projA", anchor.target.projectId)
        assertEquals("volA", anchor.target.volumeId)
        assertEquals("chA", anchor.target.chapterId)
        assertEquals(5u, anchor.target.rangeStart)
        assertEquals(50u, anchor.target.rangeEnd)
        assertEquals("entA", anchor.target.entityId)
        assertEquals("scene", anchor.target.entityType)
        assertEquals("smA", anchor.target.starmapId)
        assertEquals("uriA", anchor.target.uri)
        assertEquals("payloadA", anchor.target.payload)
        assertEquals("anchor label", anchor.label)
        assertEquals(StarMapAnchorRoleDto.REFERENCE, anchor.role)
    }

    @Test
    fun dtoToModelToDto_provenance_roundTrip() {
        val originalDto = StarMapNodeDto(
            id = "n1",
            title = "AI Node",
            kind = StarMapNodeKindDto.CONCEPT,
            payload = null,
            tags = emptyList(),
            content = StarMapNodeContentDto("note", null, null, null, null, null, null, null, null, null, null, null),
            anchors = emptyList(),
            portal = null,
            displayPolicy = defaultStarMapDisplayPolicy(),
            openBehavior = StarMapOpenBehaviorDto.WRITING_MODE,
            provenance = StarMapProvenanceDto(
                source = StarMapSourceKindDto.AI,
                sourceId = "src1",
                generatedBy = "gen1",
                promptId = "prompt1",
                reviewStatus = StarMapReviewStatusDto.NEEDS_REVIEW,
                createdFromAnchor = "anchor1"
            ),
            createdAt = 0u,
            updatedAt = 0u
        )

        val model = originalDto.toGraphNode()
        val roundTrippedDto = model.toDto(null)

        assertEquals(StarMapSourceKindDto.AI, roundTrippedDto.provenance.source)
        assertEquals("src1", roundTrippedDto.provenance.sourceId)
        assertEquals("gen1", roundTrippedDto.provenance.generatedBy)
        assertEquals("prompt1", roundTrippedDto.provenance.promptId)
        assertEquals(StarMapReviewStatusDto.NEEDS_REVIEW, roundTrippedDto.provenance.reviewStatus)
        assertEquals("anchor1", roundTrippedDto.provenance.createdFromAnchor)
        assertEquals(StarMapOpenBehaviorDto.WRITING_MODE, roundTrippedDto.openBehavior)
    }

    @Test
    fun dtoToModelToDto_portal_roundTrip() {
        val originalDto = StarMapNodeDto(
            id = "n1",
            title = "Portal",
            kind = StarMapNodeKindDto.CHARACTER,
            payload = null,
            tags = emptyList(),
            content = StarMapNodeContentDto("note", null, null, null, null, null, null, null, null, null, null, null),
            anchors = emptyList(),
            portal = StarMapPortalDto(
                targetStarmapId = "child_sm",
                deepTarget = StarMapDeepTargetDto(
                    starmapId = "child_sm",
                    path = listOf(StarMapPathSegmentDto(kind = "EnterChild", starmapId = "child1")),
                    target = StarMapTargetDetailDto(
                        kind = "Node",
                        nodeId = "inner1",
                        anchorId = null,
                        projectId = null,
                        volumeId = null,
                        chapterId = null,
                        rangeStart = null,
                        rangeEnd = null,
                        entityType = null,
                        entityId = null,
                        uri = null
                    )
                ),
                mode = StarMapPortalModeDto.ENTER_CHILD,
                previewPolicy = StarMapPortalPreviewPolicyDto.AUTO
            ),
            displayPolicy = defaultStarMapDisplayPolicy(),
            openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
            provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
            createdAt = 0u,
            updatedAt = 0u
        )

        val model = originalDto.toGraphNode()
        val roundTrippedDto = model.toDto(null)

        assertNotNull(roundTrippedDto.portal)
        assertEquals("child_sm", roundTrippedDto.portal!!.targetStarmapId)
        assertNotNull(roundTrippedDto.portal!!.deepTarget)
        assertEquals("child_sm", roundTrippedDto.portal!!.deepTarget!!.starmapId)
        assertEquals(1, roundTrippedDto.portal!!.deepTarget!!.path.size)
        assertEquals("EnterChild", roundTrippedDto.portal!!.deepTarget!!.path[0].kind)
        assertEquals("child1", roundTrippedDto.portal!!.deepTarget!!.path[0].starmapId)
        assertEquals("Node", roundTrippedDto.portal!!.deepTarget!!.target.kind)
        assertEquals("inner1", roundTrippedDto.portal!!.deepTarget!!.target.nodeId)
        assertEquals(StarMapPortalModeDto.ENTER_CHILD, roundTrippedDto.portal!!.mode)
        assertEquals(StarMapPortalPreviewPolicyDto.AUTO, roundTrippedDto.portal!!.previewPolicy)
    }

    @Test
    fun modelToDtoToModel_fullFields_roundTrip() {
        val originalModel = StarMapGraphNode(
            id = "n1",
            title = "Full Node",
            kind = StarMapNodeKind.Event,
            payload = null,
            tags = listOf("t1"),
            contentKind = "chapter",
            contentBody = "body",
            contentSummary = "summary",
            contentProjectId = "proj1",
            contentVolumeId = "vol1",
            contentRangeStart = 10,
            contentRangeEnd = 200,
            contentEntityType = "paragraph",
            contentEntityId = "ent1",
            contentLabel = "clabel",
            contentChapterId = "ch1",
            contentUri = "uri1",
            anchors = listOf(
                StarMapAnchorData(
                    anchorId = "a1",
                    targetKind = "Chapter",
                    targetProjectId = "projA",
                    targetVolumeId = "volA",
                    targetRangeStart = 5,
                    targetRangeEnd = 50,
                    targetEntityType = "scene",
                    targetPayload = "payloadA",
                    targetChapterId = "chA",
                    targetEntityId = "entA",
                    targetStarmapId = "smA",
                    targetUri = "uriA",
                    label = "alabel",
                    role = "REFERENCE"
                )
            ),
            portal = StarMapPortalData(
                targetStarmapId = "child_sm",
                deepTarget = StarMapDeepTargetData(
                    starmapId = "child_sm",
                    path = listOf(StarMapPathSegmentData(kind = "EnterChild", starmapId = "child1")),
                    target = StarMapTargetDetailData(
                        kind = "Node",
                        nodeId = "inner1",
                        anchorId = null,
                        projectId = null,
                        volumeId = null,
                        chapterId = null,
                        rangeStart = null,
                        rangeEnd = null,
                        entityType = null,
                        entityId = null,
                        uri = null
                    )
                ),
                mode = "ENTER_CHILD",
                previewPolicy = "AUTO"
            ),
            openBehavior = "WRITING_MODE",
            displayPolicy = StarMapDisplayPolicyData(
                importance = 2f,
                minVisibleScale = 0.1f,
                titleScale = 1.5f,
                summaryScale = 1.2f,
                detailScale = 1.1f,
                maxPreviewChars = 200,
                minReadablePx = 14f
            ),
            provenance = StarMapProvenanceData(
                source = "AI",
                sourceId = "src1",
                generatedBy = "gen1",
                promptId = "prompt1",
                reviewStatus = "NEEDS_REVIEW",
                createdFromAnchor = "anchor1"
            ),
            createdAt = 100,
            updatedAt = 200
        )

        val dto = originalModel.toDto(null)
        val roundTrippedModel = dto.toGraphNode()

        assertEquals("n1", roundTrippedModel.id)
        assertEquals("Full Node", roundTrippedModel.title)
        assertEquals(StarMapNodeKind.Event, roundTrippedModel.kind)
        assertEquals(listOf("t1"), roundTrippedModel.tags)
        assertEquals("chapter", roundTrippedModel.contentKind)
        assertEquals("body", roundTrippedModel.contentBody)
        assertEquals("summary", roundTrippedModel.contentSummary)
        assertEquals("proj1", roundTrippedModel.contentProjectId)
        assertEquals("vol1", roundTrippedModel.contentVolumeId)
        assertEquals(10, roundTrippedModel.contentRangeStart)
        assertEquals(200, roundTrippedModel.contentRangeEnd)
        assertEquals("paragraph", roundTrippedModel.contentEntityType)
        assertEquals("ent1", roundTrippedModel.contentEntityId)
        assertEquals("clabel", roundTrippedModel.contentLabel)
        assertEquals("ch1", roundTrippedModel.contentChapterId)
        assertEquals("uri1", roundTrippedModel.contentUri)

        assertEquals(1, roundTrippedModel.anchors.size)
        val anchor = roundTrippedModel.anchors[0]
        assertEquals("a1", anchor.anchorId)
        assertEquals("Chapter", anchor.targetKind)
        assertEquals("projA", anchor.targetProjectId)
        assertEquals("volA", anchor.targetVolumeId)
        assertEquals(5, anchor.targetRangeStart)
        assertEquals(50, anchor.targetRangeEnd)
        assertEquals("scene", anchor.targetEntityType)
        assertEquals("payloadA", anchor.targetPayload)
        assertEquals("chA", anchor.targetChapterId)
        assertEquals("entA", anchor.targetEntityId)
        assertEquals("smA", anchor.targetStarmapId)
        assertEquals("uriA", anchor.targetUri)
        assertEquals("alabel", anchor.label)
        assertEquals("REFERENCE", anchor.role)

        assertNotNull(roundTrippedModel.portal)
        assertEquals("child_sm", roundTrippedModel.portal!!.targetStarmapId)
        assertNotNull(roundTrippedModel.portal!!.deepTarget)
        assertEquals("child_sm", roundTrippedModel.portal!!.deepTarget!!.starmapId)
        assertEquals("ENTER_CHILD", roundTrippedModel.portal!!.mode)
        assertEquals("AUTO", roundTrippedModel.portal!!.previewPolicy)

        assertEquals("WRITING_MODE", roundTrippedModel.openBehavior)

        assertNotNull(roundTrippedModel.displayPolicy)
        assertEquals(2f, roundTrippedModel.displayPolicy!!.importance, 0.001f)
        assertEquals(0.1f, roundTrippedModel.displayPolicy!!.minVisibleScale, 0.001f)
        assertEquals(200, roundTrippedModel.displayPolicy!!.maxPreviewChars)

        assertNotNull(roundTrippedModel.provenance)
        assertEquals("AI", roundTrippedModel.provenance!!.source)
        assertEquals("src1", roundTrippedModel.provenance!!.sourceId)
        assertEquals("gen1", roundTrippedModel.provenance!!.generatedBy)
        assertEquals("prompt1", roundTrippedModel.provenance!!.promptId)
        assertEquals("NEEDS_REVIEW", roundTrippedModel.provenance!!.reviewStatus)
        assertEquals("anchor1", roundTrippedModel.provenance!!.createdFromAnchor)

        assertEquals(100, roundTrippedModel.createdAt)
        assertEquals(200, roundTrippedModel.updatedAt)
    }

    @Test
    fun toDto_withBase_overwritesContentAnchorsPortalProvenance() {
        val baseDto = StarMapNodeDto(
            id = "old_id",
            title = "Old Title",
            kind = StarMapNodeKindDto.NOTE,
            payload = null,
            tags = listOf("old_tag"),
            content = StarMapNodeContentDto("note", "old_summary", "old_body", null, null, null, null, null, null, null, null, null),
            anchors = listOf(
                StarMapAnchorDto(
                    anchorId = "old_a",
                    target = StarMapAnchorTargetDto("Chapter", null, null, null, null, null, null, null, null, null, null),
                    label = null,
                    role = StarMapAnchorRoleDto.SOURCE
                )
            ),
            portal = null,
            displayPolicy = defaultStarMapDisplayPolicy(),
            openBehavior = StarMapOpenBehaviorDto.INSPECTOR,
            provenance = StarMapProvenanceDto(StarMapSourceKindDto.HUMAN, null, null, null, StarMapReviewStatusDto.ACCEPTED, null),
            createdAt = 0u,
            updatedAt = 0u
        )

        val model = StarMapGraphNode(
            id = "n1",
            title = "New Title",
            kind = StarMapNodeKind.Character,
            contentKind = "chapter",
            contentBody = "new body",
            contentSummary = "new summary",
            contentProjectId = "proj1",
            contentVolumeId = "vol1",
            contentRangeStart = 10,
            contentRangeEnd = 100,
            contentEntityType = "paragraph",
            contentEntityId = "ent1",
            contentLabel = "new label",
            contentChapterId = "ch1",
            anchors = listOf(
                StarMapAnchorData(
                    anchorId = "a1",
                    targetKind = "Chapter",
                    targetProjectId = "projA",
                    targetVolumeId = "volA",
                    targetRangeStart = 5,
                    targetRangeEnd = 50,
                    targetEntityType = "scene",
                    targetPayload = "payloadA",
                    targetChapterId = "chA",
                    label = "new anchor",
                    role = "DESTINATION"
                )
            ),
            provenance = StarMapProvenanceData(
                source = "AI",
                sourceId = "src1",
                generatedBy = null,
                promptId = null,
                reviewStatus = "DRAFT",
                createdFromAnchor = null
            ),
            openBehavior = "WRITING_MODE"
        )

        val resultDto = model.toDto(baseDto)

        assertEquals("n1", resultDto.id)
        assertEquals("New Title", resultDto.title)
        assertEquals(StarMapNodeKindDto.CHARACTER, resultDto.kind)
        assertEquals("chapter", resultDto.content.kind)
        assertEquals("new body", resultDto.content.body)
        assertEquals("new summary", resultDto.content.summary)
        assertEquals("proj1", resultDto.content.projectId)
        assertEquals("vol1", resultDto.content.volumeId)
        assertEquals(10u, resultDto.content.rangeStart)
        assertEquals(100u, resultDto.content.rangeEnd)
        assertEquals("paragraph", resultDto.content.entityType)
        assertEquals("ent1", resultDto.content.entityId)
        assertEquals("new label", resultDto.content.label)
        assertEquals("ch1", resultDto.content.chapterId)

        assertEquals(1, resultDto.anchors.size)
        assertEquals("a1", resultDto.anchors[0].anchorId)
        assertEquals("projA", resultDto.anchors[0].target.projectId)
        assertEquals("volA", resultDto.anchors[0].target.volumeId)
        assertEquals(5u, resultDto.anchors[0].target.rangeStart)
        assertEquals(50u, resultDto.anchors[0].target.rangeEnd)
        assertEquals("scene", resultDto.anchors[0].target.entityType)
        assertEquals("payloadA", resultDto.anchors[0].target.payload)
        assertEquals("new anchor", resultDto.anchors[0].label)
        assertEquals(StarMapAnchorRoleDto.DESTINATION, resultDto.anchors[0].role)

        assertEquals(StarMapSourceKindDto.AI, resultDto.provenance.source)
        assertEquals("src1", resultDto.provenance.sourceId)
        assertEquals(StarMapReviewStatusDto.DRAFT, resultDto.provenance.reviewStatus)
        assertEquals(StarMapOpenBehaviorDto.WRITING_MODE, resultDto.openBehavior)
    }
}
