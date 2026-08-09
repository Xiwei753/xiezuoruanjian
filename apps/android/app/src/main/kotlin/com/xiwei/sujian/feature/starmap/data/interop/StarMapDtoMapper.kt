package com.xiwei.sujian.feature.starmap.data.interop

import com.google.gson.Gson
import com.xiwei.sujian.feature.starmap.data.StarMapRawCache
import com.xiwei.sujian.feature.starmap.model.StarMapData
import com.xiwei.sujian.feature.starmap.model.StarMapDeepTargetData
import com.xiwei.sujian.feature.starmap.model.StarMapDisplayPolicyData
import com.xiwei.sujian.feature.starmap.model.StarMapEdgeRenderData
import com.xiwei.sujian.feature.starmap.model.StarMapEmbedData
import com.xiwei.sujian.feature.starmap.model.StarMapEmbedPlacementData
import com.xiwei.sujian.feature.starmap.model.StarMapEmbedViewportData
import com.xiwei.sujian.feature.starmap.model.StarMapEndpointData
import com.xiwei.sujian.feature.starmap.model.StarMapGraphData
import com.xiwei.sujian.feature.starmap.model.StarMapHyperlinkData
import com.xiwei.sujian.feature.starmap.model.StarMapLayoutData
import com.xiwei.sujian.feature.starmap.model.StarMapLayoutKind
import com.xiwei.sujian.feature.starmap.model.StarMapLinkData
import com.xiwei.sujian.feature.starmap.model.StarMapLoadDiagnostic
import com.xiwei.sujian.feature.starmap.model.StarMapMeta
import com.xiwei.sujian.feature.starmap.model.StarMapMotionPolicyData
import com.xiwei.sujian.feature.starmap.model.StarMapPathSegmentData
import com.xiwei.sujian.feature.starmap.model.StarMapProvenanceData
import com.xiwei.sujian.feature.starmap.model.StarMapTargetDetailData
import com.xiwei.sujian.feature.starmap.model.StarMapViewportData
import uniffi.writer_core.LoadDiagnosticDto
import uniffi.writer_core.StarMapDisplayPolicyDto
import uniffi.writer_core.StarMapEdgeRenderDto
import uniffi.writer_core.StarMapEmbedDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapHyperlinkDto
import uniffi.writer_core.StarMapLayoutKindDto
import uniffi.writer_core.StarMapLinkDto
import uniffi.writer_core.StarMapMetaDto
import uniffi.writer_core.StarMapMotionPolicyDto
import uniffi.writer_core.StarMapViewportDto

internal val starMapPayloadGson = Gson()

internal fun StarMapMetaDto.toModel(): StarMapMeta =
    StarMapMeta(
        starmapId = starmapId,
        title = title,
        description = description,
        projectId = projectId,
        parentStarmapId = parentStarmapId,
        isMainForProject = isMainForProject,
        accentColor = accentColor,
        createdAt = createdAt.toLong(),
        updatedAt = updatedAt.toLong(),
        nodeCount = nodeCount.toInt(),
        edgeCount = edgeCount.toInt(),
        linkedChapterCount = linkedChapterCount.toInt(),
        childStarmapCount = childStarmapCount.toInt(),
    )

internal fun StarMapGraphDto.toRawCache(): StarMapRawCache =
    StarMapRawCache(
        graph = this,
        nodes = nodes.associateByTo(mutableMapOf()) { it.id },
        edges = edges.associateByTo(mutableMapOf()) { it.id },
        embeds = embeds.associateByTo(mutableMapOf()) { it.instanceId },
        links = links.associateByTo(mutableMapOf()) { it.linkId },
        hyperlinks = hyperlinks.associateByTo(mutableMapOf()) { it.hyperlinkId },
    )

internal fun StarMapGraphDto.toModel(cache: StarMapRawCache? = null): StarMapData =
    StarMapData(
        graph =
            StarMapGraphData(
                schemaVersion = schemaVersion.toInt(),
                id = id,
                starmapId = starmapId,
                title = title,
                nodes = nodes.map { it.toGraphNode() },
                edges = edges.map { it.toGraphEdge() },
                createdAt = createdAt.toLong(),
                updatedAt = updatedAt.toLong(),
            ),
        layout =
            cache?.let { c ->
                val layoutNodes = c.layoutNodes.values.map { it.toModel() }
                if (layoutNodes.isNotEmpty()) {
                    StarMapLayoutData(kind = StarMapLayoutKind.Freeform, nodes = layoutNodes)
                } else {
                    null
                }
            } ?: StarMapLayoutData(kind = StarMapLayoutKind.Freeform, nodes = emptyList()),
    )

internal fun StarMapEdgeRenderDto.toModel(): StarMapEdgeRenderData =
    StarMapEdgeRenderData(
        edgeId = edgeId,
        fromCx = fromCx,
        fromCy = fromCy,
        toCx = toCx,
        toCy = toCy,
        startX = startX,
        startY = startY,
        endX = endX,
        endY = endY,
        offsetX = offsetX,
        offsetY = offsetY,
        arrowTipX = arrowTipX,
        arrowTipY = arrowTipY,
        arrowLeftX = arrowLeftX,
        arrowLeftY = arrowLeftY,
        arrowRightX = arrowRightX,
        arrowRightY = arrowRightY,
        labelX = labelX,
        labelY = labelY,
        hasBidirectional = hasBidirectional,
    )

internal fun StarMapViewportDto.toModel(): StarMapViewportData =
    StarMapViewportData(
        scale = scale,
        offsetX = offsetX,
        offsetY = offsetY,
        width = width,
        height = height,
    )

internal fun StarMapViewportData.toDto(): StarMapViewportDto =
    StarMapViewportDto(
        scale = scale,
        offsetX = offsetX,
        offsetY = offsetY,
        width = width,
        height = height,
    )

internal fun StarMapLayoutKind.toDto(): StarMapLayoutKindDto =
    when (this) {
        StarMapLayoutKind.Freeform -> StarMapLayoutKindDto.FREEFORM
        StarMapLayoutKind.AutoRadial -> StarMapLayoutKindDto.AUTO_RADIAL
        StarMapLayoutKind.Custom -> StarMapLayoutKindDto.CUSTOM
    }

@Suppress("UNCHECKED_CAST")
internal fun String?.toPayloadMap(): Map<String, Any>? {
    if (isNullOrBlank()) return null
    return try {
        starMapPayloadGson.fromJson(this, Map::class.java) as? Map<String, Any>
    } catch (_: Exception) {
        null
    }
}

internal fun defaultStarMapDisplayPolicy() =
    StarMapDisplayPolicyDto(
        importance = 1f,
        minVisibleScale = 0f,
        titleScale = 1f,
        summaryScale = 1f,
        detailScale = 1f,
        maxPreviewChars = 120u,
        minReadablePx = 12f,
    )

internal fun StarMapEmbedDto.toModel(): StarMapEmbedData =
    StarMapEmbedData(
        instanceId = instanceId,
        targetStarmapId = targetStarmapId,
        label = label,
        sourceNodeId = sourceNodeId,
        hostEndpoint =
            hostEndpoint?.let {
                StarMapEndpointData(
                    kind = it.kind,
                    nodeId = it.nodeId,
                    anchorId = it.anchorId,
                )
            },
        displayPolicy =
            StarMapDisplayPolicyData(
                importance = displayPolicy.importance,
                minVisibleScale = displayPolicy.minVisibleScale,
                titleScale = displayPolicy.titleScale,
                summaryScale = displayPolicy.summaryScale,
                detailScale = displayPolicy.detailScale,
                maxPreviewChars = displayPolicy.maxPreviewChars.toInt(),
                minReadablePx = displayPolicy.minReadablePx,
            ),
        openBehavior = openBehavior.name,
        provenance =
            StarMapProvenanceData(
                source = provenance.source.name,
                sourceId = provenance.sourceId,
                generatedBy = provenance.generatedBy,
                promptId = provenance.promptId,
                reviewStatus = provenance.reviewStatus.name,
                createdFromAnchor = provenance.createdFromAnchor,
            ),
        placement =
            StarMapEmbedPlacementData(
                x = placement.x,
                y = placement.y,
                width = placement.width,
                height = placement.height,
                scale = placement.scale,
                zIndex = placement.zIndex,
                collapsed = placement.collapsed,
            ),
        targetViewport =
            StarMapEmbedViewportData(
                scale = targetViewport.scale,
                offsetX = targetViewport.offsetX,
                offsetY = targetViewport.offsetY,
            ),
    )

internal fun StarMapLinkDto.toModel(): StarMapLinkData =
    StarMapLinkData(
        linkId = linkId,
        source =
            StarMapEndpointData(
                kind = source.kind,
                nodeId = source.nodeId,
                anchorId = source.anchorId,
            ),
        target = target.toModel(),
        label = label,
    )

internal fun StarMapHyperlinkDto.toModel(): StarMapHyperlinkData =
    StarMapHyperlinkData(
        hyperlinkId = hyperlinkId,
        source = source.toModel(),
        targetUri = targetUri,
        label = label,
        targetStarmapId = targetStarmapId,
    )

internal fun LoadDiagnosticDto.toModel(): StarMapLoadDiagnostic =
    StarMapLoadDiagnostic(
        kind = kind,
        objectType = objectType,
        objectId = objectId,
        detail = detail,
    )

internal fun uniffi.writer_core.StarMapDeepTargetDto.toModel(): StarMapDeepTargetData =
    StarMapDeepTargetData(
        starmapId = starmapId,
        path = path.map { StarMapPathSegmentData(kind = it.kind, starmapId = it.starmapId) },
        target = target.toModel(),
    )

internal fun uniffi.writer_core.StarMapTargetDetailDto.toModel(): StarMapTargetDetailData =
    StarMapTargetDetailData(
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

internal fun StarMapMotionPolicyDto.toModel(): StarMapMotionPolicyData =
    StarMapMotionPolicyData(
        enabled = enabled,
        idleWobbleEnabled = idleWobbleEnabled,
        idleAmplitudeVp = idleAmplitudeVp,
        idlePeriodMs = idlePeriodMs.toInt(),
        dragLiftScale = dragLiftScale,
        dragShadowBoost = dragShadowBoost,
        settleDurationMs = settleDurationMs.toInt(),
        reduceMotion = reduceMotion,
    )
