package com.xiwei.sujian.feature.starmap.data.interop

import com.google.gson.Gson
import com.xiwei.sujian.feature.starmap.data.StarMapRawCache
import com.xiwei.sujian.feature.starmap.data.model.StarMapData
import com.xiwei.sujian.feature.starmap.data.model.StarMapEmbedData
import com.xiwei.sujian.feature.starmap.data.model.StarMapGraphData
import com.xiwei.sujian.feature.starmap.data.model.StarMapHyperlinkData
import com.xiwei.sujian.feature.starmap.data.model.StarMapLinkData
import com.xiwei.sujian.feature.starmap.data.model.StarMapLoadDiagnostic
import com.xiwei.sujian.feature.starmap.data.model.StarMapMeta
import com.xiwei.sujian.feature.starmap.data.model.StarMapPathSegmentData
import com.xiwei.sujian.feature.starmap.data.model.StarMapProvenanceData
import com.xiwei.sujian.feature.starmap.data.model.StarMapTargetDetailData
import com.xiwei.sujian.feature.starmap.data.model.StarMapTargetPathData
import uniffi.writer_core.LoadDiagnosticDto
import uniffi.writer_core.StarMapEmbedDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapHyperlinkDto
import uniffi.writer_core.StarMapLinkDto
import uniffi.writer_core.StarMapMetaDto
import uniffi.writer_core.StarMapPathSegmentDto
import uniffi.writer_core.StarMapTargetPathDto

internal val starMapPayloadGson = Gson()

internal fun StarMapMetaDto.toModel(): StarMapMeta =
    StarMapMeta(
        starmapId = starmapId,
        title = title,
        description = description,
        projectId = projectId,
        accentColor = accentColor,
        createdAt = createdAt.toLong(),
        updatedAt = updatedAt.toLong(),
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

/**
 * 整图映射。
 *
 * Core 收口后布局/视口/边几何都不再由 Core 下发，节点坐标唯一真相是
 * StarMapGraphNode.position，因此这里不再合成任何显示层数据。
 */
internal fun StarMapGraphDto.toModel(): StarMapData =
    StarMapData(
        graph =
            StarMapGraphData(
                schemaVersion = schemaVersion.toInt(),
                starmapId = starmapId,
                nodes = nodes.map { it.toGraphNode() },
                edges = edges.map { it.toGraphEdge() },
            ),
    )

@Suppress("UNCHECKED_CAST")
internal fun String?.toPayloadMap(): Map<String, Any>? {
    if (isNullOrBlank()) return null
    return try {
        starMapPayloadGson.fromJson(this, Map::class.java) as? Map<String, Any>
    } catch (_: Exception) {
        null
    }
}

internal fun StarMapEmbedDto.toModel(): StarMapEmbedData =
    StarMapEmbedData(
        instanceId = instanceId,
        targetStarmapId = targetStarmapId,
        label = label,
        hostPath = hostPath.toModel(),
        provenance =
            StarMapProvenanceData(
                source = provenance.source.name,
                sourceId = provenance.sourceId,
                generatedBy = provenance.generatedBy,
                promptId = provenance.promptId,
                reviewStatus = provenance.reviewStatus.name,
                createdFromAnchor = provenance.createdFromAnchor,
            ),
        position = position.toModel(),
    )

internal fun StarMapLinkDto.toModel(): StarMapLinkData =
    StarMapLinkData(
        linkId = linkId,
        source = source.toModel(),
        target = target.toModel(),
        label = label,
    )

internal fun StarMapHyperlinkDto.toModel(): StarMapHyperlinkData =
    StarMapHyperlinkData(
        hyperlinkId = hyperlinkId,
        source = source.toModel(),
        targetUri = targetUri,
        label = label,
    )

internal fun LoadDiagnosticDto.toModel(): StarMapLoadDiagnostic =
    StarMapLoadDiagnostic(
        kind = kind,
        objectType = objectType,
        objectId = objectId,
        detail = detail,
    )

internal fun StarMapTargetPathDto.toModel(): StarMapTargetPathData =
    StarMapTargetPathData(
        starmapId = starmapId,
        segments = segments.map { it.toModel() },
        target = target.toModel(),
    )

internal fun StarMapPathSegmentDto.toModel(): StarMapPathSegmentData =
    StarMapPathSegmentData(
        kind = kind,
        instanceId = instanceId,
        nodeId = nodeId,
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
