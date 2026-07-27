package com.xiwei.sujian.data.starmap

import com.google.gson.Gson
import com.xiwei.sujian.model.StarMapAnchorData
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapDisplayPolicyData
import com.xiwei.sujian.model.StarMapEdgeKind
import com.xiwei.sujian.model.StarMapEdgeRenderData
import com.xiwei.sujian.model.StarMapEmbedData
import com.xiwei.sujian.model.StarMapEmbedPlacementData
import com.xiwei.sujian.model.StarMapEmbedViewportData
import com.xiwei.sujian.model.StarMapGraphData
import com.xiwei.sujian.model.StarMapGraphEdge
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapHyperlinkData
import com.xiwei.sujian.model.StarMapLayoutData
import com.xiwei.sujian.model.StarMapLayoutKind
import com.xiwei.sujian.model.StarMapLayoutNodeData
import com.xiwei.sujian.model.StarMapLinkData
import com.xiwei.sujian.model.StarMapLoadDiagnostic
import com.xiwei.sujian.model.StarMapMeta
import com.xiwei.sujian.model.StarMapNodeKind
import com.xiwei.sujian.model.StarMapPhasedSnapshotResult
import com.xiwei.sujian.model.StarMapProvenanceData
import com.xiwei.sujian.model.StarMapViewportData
import uniffi.writer_core.LoadDiagnosticDto
import uniffi.writer_core.PhasedSnapshotRequestDto
import uniffi.writer_core.StarMapDisplayPolicyDto
import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapEdgeKindDto
import uniffi.writer_core.StarMapEdgeRenderDto
import uniffi.writer_core.StarMapEmbedDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapHyperlinkDto
import uniffi.writer_core.StarMapLayoutDto
import uniffi.writer_core.StarMapLayoutKindDto
import uniffi.writer_core.StarMapLayoutNodeDto
import uniffi.writer_core.StarMapLinkDto
import uniffi.writer_core.StarMapMetaDto
import uniffi.writer_core.StarMapNodeContentDto
import uniffi.writer_core.StarMapNodeDto
import uniffi.writer_core.StarMapNodeKindDto
import uniffi.writer_core.StarMapOpenBehaviorDto
import uniffi.writer_core.StarMapPhasedSnapshotDto
import uniffi.writer_core.StarMapProvenanceDto
import uniffi.writer_core.StarMapReviewStatusDto
import uniffi.writer_core.StarMapSourceKindDto
import uniffi.writer_core.StarMapViewportDto

internal val starMapPayloadGson = Gson()

internal fun StarMapMetaDto.toModel(): StarMapMeta = StarMapMeta(
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
    childStarmapCount = childStarmapCount.toInt()
)

internal fun StarMapGraphDto.toRawCache(): StarMapRawCache = StarMapRawCache(
    graph = this,
    nodes = nodes.associateByTo(mutableMapOf()) { it.id },
    edges = edges.associateByTo(mutableMapOf()) { it.id }
)

internal fun StarMapGraphDto.toModel(): StarMapData = StarMapData(
    graph = StarMapGraphData(
        schemaVersion = schemaVersion.toInt(),
        id = id,
        starmapId = starmapId,
        title = title,
        nodes = nodes.map { it.toGraphNode() },
        edges = edges.map { it.toGraphEdge() },
        createdAt = createdAt.toLong(),
        updatedAt = updatedAt.toLong()
    ),
    layout = StarMapLayoutData(
        kind = StarMapLayoutKind.Freeform,
        nodes = emptyList()
    )
)

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
    provenance = StarMapProvenanceData(
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

internal fun StarMapEdgeDto.toGraphEdge(): StarMapGraphEdge = StarMapGraphEdge(
    id = id,
    from = from ?: "",
    to = to ?: "",
    kind = kind.toModel(),
    label = label,
    payload = payload.toPayloadMap(),
    fromEndpoint = fromEndpoint?.let { "${it.kind}:${it.nodeId ?: ""}" },
    toEndpoint = toEndpoint?.let { "${it.kind}:${it.nodeId ?: ""}" },
    fromEndpointPath = fromEndpointPath?.let { ep ->
        ep.segments.joinToString("→") { it.starmapId ?: "" } + "→${ep.endpoint.kind}:${ep.endpoint.nodeId ?: ""}"
    },
    toEndpointPath = toEndpointPath?.let { ep ->
        ep.segments.joinToString("→") { it.starmapId ?: "" } + "→${ep.endpoint.kind}:${ep.endpoint.nodeId ?: ""}"
    },
    createdAt = createdAt.toLong(),
    updatedAt = updatedAt.toLong()
)

internal fun StarMapLayoutData.toDto(cache: StarMapRawCache?): StarMapLayoutDto = StarMapLayoutDto(
    kind = kind.toDto(),
    nodes = nodes.map { it.toDto(cache?.layoutNodes?.get(it.nodeId)) }
)

internal fun StarMapLayoutNodeData.toDto(base: StarMapLayoutNodeDto?): StarMapLayoutNodeDto = base?.copy(
    nodeId = nodeId,
    x = x,
    y = y,
    width = width,
    height = height,
    radius = radius,
    collapsed = collapsed,
    zIndex = zIndex,
    scale = scale,
    depth = depth,
    focusWeight = focusWeight,
    orbitGroup = orbitGroup
) ?: StarMapLayoutNodeDto(
    nodeId = nodeId,
    x = x,
    y = y,
    width = width,
    height = height,
    radius = radius,
    collapsed = collapsed,
    zIndex = zIndex,
    scale = scale,
    depth = depth,
    focusWeight = focusWeight,
    orbitGroup = orbitGroup
)

internal fun StarMapEdgeRenderDto.toModel(): StarMapEdgeRenderData = StarMapEdgeRenderData(
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
    hasBidirectional = hasBidirectional
)

internal fun StarMapViewportDto.toModel(): StarMapViewportData = StarMapViewportData(
    scale = scale,
    offsetX = offsetX,
    offsetY = offsetY,
    width = width,
    height = height
)

internal fun StarMapViewportData.toDto(): StarMapViewportDto = StarMapViewportDto(
    scale = scale,
    offsetX = offsetX,
    offsetY = offsetY,
    width = width,
    height = height
)

internal fun StarMapNodeKindDto.toModel(): StarMapNodeKind = when (this) {
    StarMapNodeKindDto.CHARACTER -> StarMapNodeKind.Character
    StarMapNodeKindDto.EVENT -> StarMapNodeKind.Event
    StarMapNodeKindDto.LOCATION -> StarMapNodeKind.Location
    StarMapNodeKindDto.ITEM -> StarMapNodeKind.Item
    StarMapNodeKindDto.CONCEPT -> StarMapNodeKind.Concept
    StarMapNodeKindDto.THEME -> StarMapNodeKind.Theme
    StarMapNodeKindDto.NOTE -> StarMapNodeKind.Note
    StarMapNodeKindDto.ORGANIZATION -> StarMapNodeKind.Organization
    StarMapNodeKindDto.TIMELINE -> StarMapNodeKind.Timeline
    StarMapNodeKindDto.PLOT -> StarMapNodeKind.Plot
    StarMapNodeKindDto.FORESHADOWING -> StarMapNodeKind.Foreshadowing
    StarMapNodeKindDto.CHAPTER -> StarMapNodeKind.Chapter
    StarMapNodeKindDto.CUSTOM -> StarMapNodeKind.Custom
}

internal fun StarMapNodeKind.toDto(): StarMapNodeKindDto = when (this) {
    StarMapNodeKind.Character -> StarMapNodeKindDto.CHARACTER
    StarMapNodeKind.Event -> StarMapNodeKindDto.EVENT
    StarMapNodeKind.Location -> StarMapNodeKindDto.LOCATION
    StarMapNodeKind.Item -> StarMapNodeKindDto.ITEM
    StarMapNodeKind.Concept -> StarMapNodeKindDto.CONCEPT
    StarMapNodeKind.Theme -> StarMapNodeKindDto.THEME
    StarMapNodeKind.Note -> StarMapNodeKindDto.NOTE
    StarMapNodeKind.Organization -> StarMapNodeKindDto.ORGANIZATION
    StarMapNodeKind.Timeline -> StarMapNodeKindDto.TIMELINE
    StarMapNodeKind.Plot -> StarMapNodeKindDto.PLOT
    StarMapNodeKind.Foreshadowing -> StarMapNodeKindDto.FORESHADOWING
    StarMapNodeKind.Chapter -> StarMapNodeKindDto.CHAPTER
    StarMapNodeKind.Custom -> StarMapNodeKindDto.CUSTOM
}

internal fun StarMapLayoutKind.toDto(): StarMapLayoutKindDto = when (this) {
    StarMapLayoutKind.Freeform -> StarMapLayoutKindDto.FREEFORM
    StarMapLayoutKind.AutoRadial -> StarMapLayoutKindDto.AUTO_RADIAL
    StarMapLayoutKind.Custom -> StarMapLayoutKindDto.CUSTOM
}

internal fun StarMapEdgeKindDto.toModel(): StarMapEdgeKind = when (this) {
    StarMapEdgeKindDto.CONTAINS -> StarMapEdgeKind.Contains
    StarMapEdgeKindDto.REFERENCES -> StarMapEdgeKind.References
    StarMapEdgeKindDto.APPEARS_IN -> StarMapEdgeKind.AppearsIn
    StarMapEdgeKindDto.CAUSES -> StarMapEdgeKind.Causes
    StarMapEdgeKindDto.RELATED_TO -> StarMapEdgeKind.RelatedTo
    StarMapEdgeKindDto.LOCATED_AT -> StarMapEdgeKind.LocatedAt
    StarMapEdgeKindDto.CHARACTER_RELATION -> StarMapEdgeKind.CharacterRelation
    StarMapEdgeKindDto.TIMELINE -> StarMapEdgeKind.Timeline
    StarMapEdgeKindDto.FORESHADOWS -> StarMapEdgeKind.Foreshadows
    StarMapEdgeKindDto.RESOLVES -> StarMapEdgeKind.Resolves
    StarMapEdgeKindDto.DEPENDS_ON -> StarMapEdgeKind.DependsOn
    StarMapEdgeKindDto.CONFLICTS_WITH -> StarMapEdgeKind.ConflictsWith
    StarMapEdgeKindDto.CUSTOM -> StarMapEdgeKind.Custom
}

internal fun StarMapEdgeKind.toDto(): StarMapEdgeKindDto = when (this) {
    StarMapEdgeKind.Contains -> StarMapEdgeKindDto.CONTAINS
    StarMapEdgeKind.References -> StarMapEdgeKindDto.REFERENCES
    StarMapEdgeKind.AppearsIn -> StarMapEdgeKindDto.APPEARS_IN
    StarMapEdgeKind.Causes -> StarMapEdgeKindDto.CAUSES
    StarMapEdgeKind.RelatedTo -> StarMapEdgeKindDto.RELATED_TO
    StarMapEdgeKind.LocatedAt -> StarMapEdgeKindDto.LOCATED_AT
    StarMapEdgeKind.CharacterRelation -> StarMapEdgeKindDto.CHARACTER_RELATION
    StarMapEdgeKind.Timeline -> StarMapEdgeKindDto.TIMELINE
    StarMapEdgeKind.Foreshadows -> StarMapEdgeKindDto.FORESHADOWS
    StarMapEdgeKind.Resolves -> StarMapEdgeKindDto.RESOLVES
    StarMapEdgeKind.DependsOn -> StarMapEdgeKindDto.DEPENDS_ON
    StarMapEdgeKind.ConflictsWith -> StarMapEdgeKindDto.CONFLICTS_WITH
    StarMapEdgeKind.Custom -> StarMapEdgeKindDto.CUSTOM
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

internal fun defaultStarMapDisplayPolicy() = StarMapDisplayPolicyDto(
    importance = 1f,
    minVisibleScale = 0f,
    titleScale = 1f,
    summaryScale = 1f,
    detailScale = 1f,
    maxPreviewChars = 120u,
    minReadablePx = 12f
)

internal fun PhasedSnapshotRequestDto.Companion.create(
    targetPhase: String = "PrefetchNearbyObjects",
    sinceRevision: ULong = 0u
): PhasedSnapshotRequestDto = PhasedSnapshotRequestDto(
    targetPhase = targetPhase,
    sinceRevision = sinceRevision
)

internal fun StarMapPhasedSnapshotDto.toRawCache(): StarMapRawCache = StarMapRawCache(
    graph = StarMapGraphDto(
        schemaVersion = 1u,
        id = starmapId,
        starmapId = starmapId,
        title = title,
        nodes = nodes,
        edges = edges,
        embeds = embeds,
        links = links,
        createdAt = 0u,
        updatedAt = 0u
    ),
    nodes = nodes.associateByTo(mutableMapOf()) { it.id },
    edges = edges.associateByTo(mutableMapOf()) { it.id },
    layoutNodes = layout?.nodes?.associateByTo(mutableMapOf()) { it.nodeId } ?: mutableMapOf()
)

internal fun StarMapPhasedSnapshotDto.toSnapshotResult(): StarMapPhasedSnapshotResult {
    val layoutData = layout?.toModel() ?: StarMapLayoutData(
        kind = StarMapLayoutKind.Freeform,
        nodes = emptyList()
    )
    val data = StarMapData(
        graph = StarMapGraphData(
            schemaVersion = 0,
            id = starmapId,
            starmapId = starmapId,
            title = title,
            nodes = nodes.map { it.toGraphNode() },
            edges = edges.map { it.toGraphEdge() },
            createdAt = 0L,
            updatedAt = 0L
        ),
        layout = layoutData,
        viewport = viewport?.toModel() ?: StarMapViewportData(),
        embeds = embeds.map { it.toModel() },
        links = links.map { it.toModel() },
        hyperlinks = hyperlinks.map { it.toModel() },
        loadPhase = loadPhase,
        packageRevision = packageRevision,
        complete = complete
    )
    return StarMapPhasedSnapshotResult(
        data = data,
        diagnostics = diagnostics.map { it.toModel() }
    )
}

internal fun StarMapLayoutDto.toModel(): StarMapLayoutData = StarMapLayoutData(
    kind = when (kind) {
        StarMapLayoutKindDto.FREEFORM -> StarMapLayoutKind.Freeform
        StarMapLayoutKindDto.AUTO_RADIAL -> StarMapLayoutKind.AutoRadial
        StarMapLayoutKindDto.CUSTOM -> StarMapLayoutKind.Custom
    },
    nodes = nodes.map { it.toModel() }
)

internal fun StarMapLayoutNodeDto.toModel(): StarMapLayoutNodeData = StarMapLayoutNodeData(
    nodeId = nodeId,
    x = x,
    y = y,
    width = width,
    height = height,
    radius = radius,
    collapsed = collapsed,
    zIndex = zIndex,
    scale = scale,
    depth = depth,
    focusWeight = focusWeight,
    orbitGroup = orbitGroup
)

internal fun StarMapEmbedDto.toModel(): StarMapEmbedData = StarMapEmbedData(
    instanceId = instanceId,
    targetStarmapId = targetStarmapId,
    label = label,
    sourceNodeId = sourceNodeId,
    placement = StarMapEmbedPlacementData(
        x = placement.x,
        y = placement.y,
        width = placement.width,
        height = placement.height,
        scale = placement.scale,
        zIndex = placement.zIndex,
        collapsed = placement.collapsed
    ),
    targetViewport = StarMapEmbedViewportData(
        scale = targetViewport.scale,
        offsetX = targetViewport.offsetX,
        offsetY = targetViewport.offsetY
    )
)

internal fun StarMapLinkDto.toModel(): StarMapLinkData = StarMapLinkData(
    linkId = linkId,
    sourceNodeId = when (source.kind) {
        "Node" -> source.nodeId ?: ""
        "Anchor" -> source.nodeId ?: ""
        else -> ""
    },
    targetStarmapId = target.starmapId,
    label = label
)

internal fun StarMapHyperlinkDto.toModel(): StarMapHyperlinkData = StarMapHyperlinkData(
    hyperlinkId = hyperlinkId,
    targetUri = targetUri,
    label = label,
    targetStarmapId = targetStarmapId
)

internal fun LoadDiagnosticDto.toModel(): StarMapLoadDiagnostic = StarMapLoadDiagnostic(
    kind = kind,
    objectType = objectType,
    objectId = objectId,
    detail = detail
)
