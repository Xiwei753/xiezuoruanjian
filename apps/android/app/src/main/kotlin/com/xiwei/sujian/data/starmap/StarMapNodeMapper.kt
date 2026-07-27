package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.StarMapAnchorData
import com.xiwei.sujian.model.StarMapDisplayPolicyData
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapNodeKind
import uniffi.writer_core.StarMapDisplayPolicyDto
import uniffi.writer_core.StarMapNodeContentDto
import uniffi.writer_core.StarMapNodeDto
import uniffi.writer_core.StarMapNodeKindDto
import uniffi.writer_core.StarMapOpenBehaviorDto
import uniffi.writer_core.StarMapProvenanceDto
import uniffi.writer_core.StarMapReviewStatusDto
import uniffi.writer_core.StarMapSourceKindDto

internal fun StarMapNodeDto.toGraphNode(): StarMapGraphNode = StarMapGraphNode(
    id = id,
    title = title,
    kind = kind.toModel(),
    payload = payload.toPayloadMap(),
    tags = tags,
    contentKind = content?.kind,
    anchors = anchors.map { StarMapAnchorData(anchorId = it.anchorId, label = it.label, role = it.role.name) },
    displayPolicy = StarMapDisplayPolicyData(
        importance = displayPolicy.importance,
        minVisibleScale = displayPolicy.minVisibleScale,
        titleScale = displayPolicy.titleScale,
        summaryScale = displayPolicy.summaryScale,
        detailScale = displayPolicy.detailScale,
        maxPreviewChars = displayPolicy.maxPreviewChars.toInt(),
        minReadablePx = displayPolicy.minReadablePx
    ),
    provenance = com.xiwei.sujian.model.StarMapProvenanceData(
        source = provenance.source.name,
        reviewStatus = provenance.reviewStatus.name
    ),
    createdAt = createdAt.toLong(),
    updatedAt = updatedAt.toLong()
)

internal fun StarMapGraphNode.toDto(base: StarMapNodeDto?): StarMapNodeDto {
    val resolvedPayload = payload?.let { starMapPayloadGson.toJson(it) } ?: base?.payload
    val dtoKind = kind.toDto()
    val dtoCreatedAt = createdAt.toULong()
    val dtoUpdatedAt = updatedAt.toULong()

    return if (base != null) base.copy(
        id = id,
        title = title,
        kind = dtoKind,
        payload = resolvedPayload,
        tags = tags,
        createdAt = dtoCreatedAt,
        updatedAt = dtoUpdatedAt
    ) else toNewDefaultNoteNodeDto(
        payload = resolvedPayload,
        dtoKind = dtoKind,
        dtoCreatedAt = dtoCreatedAt,
        dtoUpdatedAt = dtoUpdatedAt
    )
}

internal fun StarMapGraphNode.toNewDefaultNoteNodeDto(
    payload: String?,
    dtoKind: StarMapNodeKindDto,
    dtoCreatedAt: ULong,
    dtoUpdatedAt: ULong
): StarMapNodeDto = StarMapNodeDto(
    id = id,
    title = title,
    kind = dtoKind,
    payload = payload,
    tags = tags,
    content = StarMapNodeContentDto(
        kind = "note",
        summary = null,
        body = null,
        projectId = null,
        volumeId = null,
        chapterId = null,
        rangeStart = null,
        rangeEnd = null,
        entityType = null,
        entityId = null,
        uri = null,
        label = null
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
    createdAt = dtoCreatedAt,
    updatedAt = dtoUpdatedAt
)
