package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.StarMapAnchorData
import com.xiwei.sujian.model.StarMapDeepTargetData
import com.xiwei.sujian.model.StarMapDisplayPolicyData
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapPathSegmentData
import com.xiwei.sujian.model.StarMapPortalData
import com.xiwei.sujian.model.StarMapProvenanceData
import com.xiwei.sujian.model.StarMapTargetDetailData
import uniffi.writer_core.StarMapAnchorDto
import uniffi.writer_core.StarMapAnchorRoleDto
import uniffi.writer_core.StarMapAnchorTargetDto
import uniffi.writer_core.StarMapDeepTargetDto
import uniffi.writer_core.StarMapDisplayPolicyDto
import uniffi.writer_core.StarMapNodeContentDto
import uniffi.writer_core.StarMapNodeDto
import uniffi.writer_core.StarMapOpenBehaviorDto
import uniffi.writer_core.StarMapPathSegmentDto
import uniffi.writer_core.StarMapPortalDto
import uniffi.writer_core.StarMapPortalModeDto
import uniffi.writer_core.StarMapPortalPreviewPolicyDto
import uniffi.writer_core.StarMapProvenanceDto
import uniffi.writer_core.StarMapReviewStatusDto
import uniffi.writer_core.StarMapSourceKindDto
import uniffi.writer_core.StarMapTargetDetailDto

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
        contentLabel = content?.label,
        contentChapterId = content?.chapterId,
        contentUri = content?.uri,
        anchors = anchors.map { it.toAnchorModel() },
        portal = portal?.toPortalModel(),
        openBehavior = openBehavior?.name,
        displayPolicy = displayPolicy.toDisplayPolicyModel(),
        provenance = provenance.toProvenanceModel(),
        createdAt = createdAt.toLong(),
        updatedAt = updatedAt.toLong(),
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
        targetUri = target.uri,
        label = label,
        role = role.name,
    )

internal fun StarMapPortalDto.toPortalModel(): StarMapPortalData =
    StarMapPortalData(
        targetStarmapId = targetStarmapId,
        deepTarget = deepTarget?.toModel(),
        mode = mode.name,
        previewPolicy = previewPolicy.name,
    )

internal fun StarMapDisplayPolicyDto.toDisplayPolicyModel(): StarMapDisplayPolicyData =
    StarMapDisplayPolicyData(
        importance = importance,
        minVisibleScale = minVisibleScale,
        titleScale = titleScale,
        summaryScale = summaryScale,
        detailScale = detailScale,
        maxPreviewChars = maxPreviewChars.toInt(),
        minReadablePx = minReadablePx,
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
    val dtoContent = contentToDto()
    val dtoAnchors = anchors.map { it.anchorToDto() }
    val dtoPortal = portal?.portalToDto()
    val dtoDisplayPolicy = displayPolicy?.displayPolicyToDto() ?: base?.displayPolicy ?: defaultStarMapDisplayPolicy()
    val dtoOpenBehavior = openBehavior?.toOpenBehaviorDto() ?: base?.openBehavior ?: StarMapOpenBehaviorDto.INSPECTOR
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
            displayPolicy = dtoDisplayPolicy,
            openBehavior = dtoOpenBehavior,
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
            displayPolicy = dtoDisplayPolicy,
            openBehavior = dtoOpenBehavior,
            provenance = dtoProvenance,
            createdAt = dtoCreatedAt,
            updatedAt = dtoUpdatedAt,
        )
    }
}

internal fun StarMapGraphNode.contentToDto(): StarMapNodeContentDto =
    StarMapNodeContentDto(
        kind = contentKind ?: "note",
        summary = contentSummary,
        body = contentBody,
        projectId = contentProjectId,
        volumeId = contentVolumeId,
        chapterId = contentChapterId,
        rangeStart = contentRangeStart?.toUInt(),
        rangeEnd = contentRangeEnd?.toUInt(),
        entityType = contentEntityType,
        entityId = contentEntityId,
        uri = contentUri,
        label = contentLabel,
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
                uri = targetUri,
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
        targetStarmapId = targetStarmapId,
        deepTarget = deepTarget?.toDeepTargetDto(),
        mode =
            when (mode) {
                "PREVIEW_INLINE" -> StarMapPortalModeDto.PREVIEW_INLINE
                "REFERENCE_ONLY" -> StarMapPortalModeDto.REFERENCE_ONLY
                else -> StarMapPortalModeDto.ENTER_CHILD
            },
        previewPolicy =
            when (previewPolicy) {
                "ALWAYS" -> StarMapPortalPreviewPolicyDto.ALWAYS
                "NEVER" -> StarMapPortalPreviewPolicyDto.NEVER
                else -> StarMapPortalPreviewPolicyDto.AUTO
            },
    )

internal fun StarMapDisplayPolicyData.displayPolicyToDto(): StarMapDisplayPolicyDto =
    StarMapDisplayPolicyDto(
        importance = importance,
        minVisibleScale = minVisibleScale,
        titleScale = titleScale,
        summaryScale = summaryScale,
        detailScale = detailScale,
        maxPreviewChars = maxPreviewChars.toUInt(),
        minReadablePx = minReadablePx,
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

internal fun String.toOpenBehaviorDto(): StarMapOpenBehaviorDto =
    when (this) {
        "EXPAND_CARD" -> StarMapOpenBehaviorDto.EXPAND_CARD
        "WRITING_MODE" -> StarMapOpenBehaviorDto.WRITING_MODE
        "JUMP_TO_ANCHOR" -> StarMapOpenBehaviorDto.JUMP_TO_ANCHOR
        "ENTER_PORTAL" -> StarMapOpenBehaviorDto.ENTER_PORTAL
        "CUSTOM" -> StarMapOpenBehaviorDto.CUSTOM
        else -> StarMapOpenBehaviorDto.INSPECTOR
    }

internal fun StarMapDeepTargetData.toDeepTargetDto(): StarMapDeepTargetDto =
    StarMapDeepTargetDto(
        starmapId = starmapId,
        path = path.map { it.toPathSegmentDto() },
        target = target.toTargetDetailDto(),
    )

internal fun StarMapPathSegmentData.toPathSegmentDto(): StarMapPathSegmentDto =
    StarMapPathSegmentDto(
        kind = kind,
        starmapId = starmapId,
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
        uri = uri,
    )
