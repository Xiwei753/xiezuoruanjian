package com.xiwei.sujian.feature.starmap.data.interop

import com.xiwei.sujian.feature.starmap.data.model.StarMapAnchorData
import com.xiwei.sujian.feature.starmap.data.model.StarMapGraphNode
import com.xiwei.sujian.feature.starmap.data.model.StarMapNodeContentData
import com.xiwei.sujian.feature.starmap.data.model.StarMapNodeStyleData
import com.xiwei.sujian.feature.starmap.data.model.StarMapPathSegmentData
import com.xiwei.sujian.feature.starmap.data.model.StarMapPointData
import com.xiwei.sujian.feature.starmap.data.model.StarMapPortalData
import com.xiwei.sujian.feature.starmap.data.model.StarMapProvenanceData
import com.xiwei.sujian.feature.starmap.data.model.StarMapTargetDetailData
import com.xiwei.sujian.feature.starmap.data.model.StarMapTargetPathData
import uniffi.writer_core.StarMapAnchorDto
import uniffi.writer_core.StarMapAnchorRoleDto
import uniffi.writer_core.StarMapAnchorTargetDto
import uniffi.writer_core.StarMapNodeContentDto
import uniffi.writer_core.StarMapNodeDto
import uniffi.writer_core.StarMapNodeStyleDto
import uniffi.writer_core.StarMapPathSegmentDto
import uniffi.writer_core.StarMapPointDto
import uniffi.writer_core.StarMapPortalDto
import uniffi.writer_core.StarMapProvenanceDto
import uniffi.writer_core.StarMapReviewStatusDto
import uniffi.writer_core.StarMapSourceKindDto
import uniffi.writer_core.StarMapTargetDetailDto
import uniffi.writer_core.StarMapTargetPathDto

internal fun StarMapNodeDto.toGraphNode(): StarMapGraphNode =
    StarMapGraphNode(
        id = id,
        title = title,
        kind = kind.toModel(),
        payload = payload.toPayloadMap(),
        tags = tags,
        contentKind = content?.kind,
        contentBody = content?.body,
        contentSummary = content?.summary,
        contentProjectId = content?.projectId,
        contentVolumeId = content?.volumeId,
        contentRangeStart = content?.rangeStart?.toInt(),
        contentRangeEnd = content?.rangeEnd?.toInt(),
        contentEntityType = content?.entityType,
        contentEntityId = content?.entityId,
        contentChapterId = content?.chapterId,
        anchors = anchors.map { it.toAnchorModel() },
        portal = portal?.toPortalModel(),
        position = position.toModel(),
        style = style.toModel(),
        provenance = provenance.toProvenanceModel(),
        createdAt = createdAt.toLong(),
        updatedAt = updatedAt.toLong(),
    )

internal fun StarMapPointDto.toModel(): StarMapPointData =
    StarMapPointData(
        x = x.toFloat(),
        y = y.toFloat(),
    )

internal fun StarMapNodeStyleDto.toModel(): StarMapNodeStyleData =
    StarMapNodeStyleData(
        fillColor = fillColor,
    )

internal fun StarMapPointData.toPointDto(): StarMapPointDto =
    StarMapPointDto(
        x = x.toDouble(),
        y = y.toDouble(),
    )

internal fun StarMapAnchorDto.toAnchorModel(): StarMapAnchorData =
    StarMapAnchorData(
        anchorId = anchorId,
        targetKind = target.kind,
        targetProjectId = target.projectId,
        targetVolumeId = target.volumeId,
        targetRangeStart = target.rangeStart?.toInt(),
        targetRangeEnd = target.rangeEnd?.toInt(),
        targetEntityType = target.entityType,
        targetPayload = target.payload,
        targetChapterId = target.chapterId,
        targetEntityId = target.entityId,
        targetStarmapId = target.starmapId,
        label = label,
        role = role.name,
    )

internal fun StarMapPortalDto.toPortalModel(): StarMapPortalData =
    StarMapPortalData(
        destinationStarmapId = destinationStarmapId,
        destinationTarget = destinationTarget?.toModel(),
    )

internal fun StarMapProvenanceDto.toProvenanceModel(): StarMapProvenanceData =
    StarMapProvenanceData(
        source = source.name,
        sourceId = sourceId,
        generatedBy = generatedBy,
        promptId = promptId,
        reviewStatus = reviewStatus.name,
        createdFromAnchor = createdFromAnchor,
    )

internal fun StarMapGraphNode.toDto(base: StarMapNodeDto?): StarMapNodeDto {
    val resolvedPayload = payload?.let { starMapPayloadGson.toJson(it) } ?: base?.payload
    val dtoKind = kind.toDto()
    val dtoCreatedAt = createdAt.toULong()
    val dtoUpdatedAt = updatedAt.toULong()
    val dtoContent = contentToDto(base?.content)
    val dtoAnchors = anchors.map { it.anchorToDto() }
    val dtoPortal = portal?.portalToDto()
    val dtoPosition = StarMapPointDto(x = position.x.toDouble(), y = position.y.toDouble())
    val dtoStyle = StarMapNodeStyleDto(fillColor = style.fillColor)
    val dtoProvenance =
        provenance?.provenanceToDto() ?: base?.provenance ?: StarMapProvenanceDto(
            source = StarMapSourceKindDto.HUMAN,
            sourceId = null,
            generatedBy = null,
            promptId = null,
            reviewStatus = StarMapReviewStatusDto.ACCEPTED,
            createdFromAnchor = null,
        )

    return if (base != null) {
        base.copy(
            id = id,
            title = title,
            kind = dtoKind,
            payload = resolvedPayload,
            tags = tags,
            content = dtoContent,
            anchors = dtoAnchors,
            portal = dtoPortal,
            position = dtoPosition,
            style = dtoStyle,
            provenance = dtoProvenance,
            createdAt = dtoCreatedAt,
            updatedAt = dtoUpdatedAt,
        )
    } else {
        StarMapNodeDto(
            id = id,
            title = title,
            kind = dtoKind,
            payload = resolvedPayload,
            tags = tags,
            content = dtoContent,
            anchors = dtoAnchors,
            portal = dtoPortal,
            position = dtoPosition,
            style = dtoStyle,
            provenance = dtoProvenance,
            createdAt = dtoCreatedAt,
            updatedAt = dtoUpdatedAt,
        )
    }
}

internal fun StarMapGraphNode.contentToDto(base: StarMapNodeContentDto? = null): StarMapNodeContentDto {
    val suppliedKind = contentKind?.takeIf { it.isNotBlank() }
    // Issue #984：没有提供任何内容信息时沿用 Core 原内容，避免把已有
    // chapterRef/entityRef 覆盖成 empty。
    if (suppliedKind == null && contentBody == null) {
        base?.let { return it }
    }
    // contentKind 是 Core 内容类型（empty/inline/chapterRef/entityRef），
    // 不是节点种类 Note。只有旧数据缺少 contentKind 时才按 body 推断 inline。
    val resolvedKind =
        suppliedKind
            ?: if (contentBody != null) {
                StarMapNodeContentData.TYPE_INLINE
            } else {
                StarMapNodeContentData.TYPE_EMPTY
            }
    return StarMapNodeContentDto(
        kind = resolvedKind,
        summary = contentSummary,
        body = contentBody,
        projectId = contentProjectId,
        volumeId = contentVolumeId,
        chapterId = contentChapterId,
        rangeStart = contentRangeStart?.toUInt(),
        rangeEnd = contentRangeEnd?.toUInt(),
        entityType = contentEntityType,
        entityId = contentEntityId,
    )
}

/** Issue #984：编辑正文时只走 typed DTO，不经 JSON。 */
internal fun StarMapNodeContentData.toContentDto(): StarMapNodeContentDto =
    StarMapNodeContentDto(
        kind = type,
        summary = summary,
        body = body,
        projectId = null,
        volumeId = null,
        chapterId = null,
        rangeStart = null,
        rangeEnd = null,
        entityType = null,
        entityId = null,
    )

internal fun StarMapAnchorData.anchorToDto(): StarMapAnchorDto =
    StarMapAnchorDto(
        anchorId = anchorId,
        target =
            StarMapAnchorTargetDto(
                kind = targetKind,
                projectId = targetProjectId,
                volumeId = targetVolumeId,
                chapterId = targetChapterId,
                rangeStart = targetRangeStart?.toUInt(),
                rangeEnd = targetRangeEnd?.toUInt(),
                entityId = targetEntityId,
                entityType = targetEntityType,
                starmapId = targetStarmapId,
                payload = targetPayload,
            ),
        label = label,
        role =
            when (role) {
                "DESTINATION" -> StarMapAnchorRoleDto.DESTINATION
                "REFERENCE" -> StarMapAnchorRoleDto.REFERENCE
                "CUSTOM" -> StarMapAnchorRoleDto.CUSTOM
                else -> StarMapAnchorRoleDto.SOURCE
            },
    )

internal fun StarMapPortalData.portalToDto(): StarMapPortalDto =
    StarMapPortalDto(
        destinationStarmapId = destinationStarmapId,
        destinationTarget = destinationTarget?.toTargetDetailDto(),
    )

internal fun StarMapProvenanceData.provenanceToDto(): StarMapProvenanceDto =
    StarMapProvenanceDto(
        source =
            when (source) {
                "AI" -> StarMapSourceKindDto.AI
                "IMPORT" -> StarMapSourceKindDto.IMPORT
                "PLUGIN" -> StarMapSourceKindDto.PLUGIN
                "SYSTEM" -> StarMapSourceKindDto.SYSTEM
                "UNKNOWN" -> StarMapSourceKindDto.UNKNOWN
                else -> StarMapSourceKindDto.HUMAN
            },
        sourceId = sourceId,
        generatedBy = generatedBy,
        promptId = promptId,
        reviewStatus =
            when (reviewStatus) {
                "DRAFT" -> StarMapReviewStatusDto.DRAFT
                "NEEDS_REVIEW" -> StarMapReviewStatusDto.NEEDS_REVIEW
                "REJECTED" -> StarMapReviewStatusDto.REJECTED
                "UNKNOWN" -> StarMapReviewStatusDto.UNKNOWN
                else -> StarMapReviewStatusDto.ACCEPTED
            },
        createdFromAnchor = createdFromAnchor,
    )

internal fun StarMapTargetPathData.toTargetPathDto(): StarMapTargetPathDto =
    StarMapTargetPathDto(
        starmapId = starmapId,
        segments = segments.map { it.toPathSegmentDto() },
        target = target.toTargetDetailDto(),
    )

internal fun StarMapPathSegmentData.toPathSegmentDto(): StarMapPathSegmentDto =
    StarMapPathSegmentDto(
        kind = kind,
        instanceId = instanceId,
        nodeId = nodeId,
    )

internal fun StarMapTargetDetailData.toTargetDetailDto(): StarMapTargetDetailDto =
    StarMapTargetDetailDto(
        kind = kind,
        nodeId = nodeId,
        anchorId = anchorId,
        projectId = projectId,
        volumeId = volumeId,
        chapterId = chapterId,
        rangeStart = rangeStart,
        rangeEnd = rangeEnd,
        entityType = entityType,
        entityId = entityId,
    )
