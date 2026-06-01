package com.xiwei.writerapp.data

import com.google.gson.Gson
import com.xiwei.writerapp.model.StarMapData
import com.xiwei.writerapp.model.StarMapEdgeKind
import com.xiwei.writerapp.model.StarMapGraphData
import com.xiwei.writerapp.model.StarMapGraphEdge
import com.xiwei.writerapp.model.StarMapGraphNode
import com.xiwei.writerapp.model.StarMapLayoutData
import com.xiwei.writerapp.model.StarMapLayoutKind
import com.xiwei.writerapp.model.StarMapLayoutNodeData
import com.xiwei.writerapp.model.StarMapMeta
import com.xiwei.writerapp.model.StarMapNodeKind
import uniffi.writer_core.StarMapDisplayPolicyDto
import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapEdgeKindDto
import uniffi.writer_core.StarMapEmbedDto
import uniffi.writer_core.StarMapEmbedPatchInputDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapLayoutDto
import uniffi.writer_core.StarMapLayoutKindDto
import uniffi.writer_core.StarMapLayoutNodeDto
import uniffi.writer_core.StarMapLinkDto
import uniffi.writer_core.StarMapLinkPatchInputDto
import uniffi.writer_core.StarMapMetaDto
import uniffi.writer_core.StarMapNodeContentDto
import uniffi.writer_core.StarMapNodeDto
import uniffi.writer_core.StarMapNodeKindDto
import uniffi.writer_core.StarMapOpenBehaviorDto
import uniffi.writer_core.StarMapProvenanceDto
import uniffi.writer_core.StarMapReviewStatusDto
import uniffi.writer_core.StarMapSourceKindDto

private val starMapPayloadGson = Gson()

private data class StarMapRawCache(
    val nodes: MutableMap<String, StarMapNodeDto> = mutableMapOf(),
    val edges: MutableMap<String, StarMapEdgeDto> = mutableMapOf(),
    val layoutNodes: MutableMap<String, StarMapLayoutNodeDto> = mutableMapOf()
)

class StarMapBridge(private val appService: AppServiceBridge) {
    private val rawCacheByStarmapId = mutableMapOf<String, StarMapRawCache>()

    fun listStarmaps(): BridgeResult<List<StarMapMeta>> {
        return when (val result = appService.listStarMaps()) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.map { it.toModel() })
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun createStarmap(title: String, desc: String): BridgeResult<StarMapMeta> {
        return when (val result = appService.createStarMap(title, desc)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun getStarmapGraph(starmapId: String): BridgeResult<StarMapData> {
        return when (val result = appService.getStarMapGraph(starmapId)) {
            is BridgeResult.Success -> {
                try {
                    rawCacheByStarmapId[starmapId] = result.data.toRawCache()
                    BridgeResult.Success(result.data.toModel())
                } catch (e: Exception) {
                    BridgeResult.Error(ResultEnvelope.error("CONVERSION_ERROR", "Failed to convert starmap graph: ${e.message}"))
                }
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun addStarmapNode(starmapId: String, node: StarMapGraphNode, x: Float = 0f, y: Float = 0f): BridgeResult<StarMapGraphNode> {
        val cache = when (val cacheResult = refreshRawCache(starmapId)) {
            is BridgeResult.Success -> cacheResult.data
            is BridgeResult.Error -> return BridgeResult.Error(cacheResult.envelope)
            BridgeResult.NotLoaded -> return BridgeResult.NotLoaded
        }
        return when (val result = appService.addStarMapNode(starmapId, node.toDto(cache.nodes[node.id]), x, y)) {
            is BridgeResult.Success -> {
                rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }
                    .nodes[result.data.id] = result.data
                BridgeResult.Success(result.data.toGraphNode())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun saveStarmapLayout(starmapId: String, layout: StarMapLayoutData): BridgeResult<Boolean> {
        val cache = when (val cacheResult = refreshRawCache(starmapId)) {
            is BridgeResult.Success -> cacheResult.data
            is BridgeResult.Error -> return BridgeResult.Error(cacheResult.envelope)
            BridgeResult.NotLoaded -> return BridgeResult.NotLoaded
        }
        val dto = layout.toDto(cache)
        return when (val result = appService.saveStarMapLayout(starmapId, dto)) {
            is BridgeResult.Success -> {
                cache.layoutNodes.clear()
                cache.layoutNodes.putAll(dto.nodes.associateBy { it.nodeId })
                BridgeResult.Success(result.data)
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun addStarmapEmbed(starmapId: String, embed: StarMapEmbedDto): BridgeResult<StarMapEmbedDto> {
        return when (val result = appService.addStarmapEmbed(starmapId, embed)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data)
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun updateStarmapEmbed(starmapId: String, instanceId: String, patch: StarMapEmbedPatchInputDto): BridgeResult<StarMapEmbedDto> {
        return when (val result = appService.updateStarmapEmbed(starmapId, instanceId, patch)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data)
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun deleteStarmapEmbed(starmapId: String, instanceId: String): BridgeResult<Boolean> = appService.deleteStarmapEmbed(starmapId, instanceId)

    fun addStarmapLink(starmapId: String, link: StarMapLinkDto): BridgeResult<StarMapLinkDto> {
        return when (val result = appService.addStarmapLink(starmapId, link)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data)
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun updateStarmapLink(starmapId: String, linkId: String, patch: StarMapLinkPatchInputDto): BridgeResult<StarMapLinkDto> {
        return when (val result = appService.updateStarmapLink(starmapId, linkId, patch)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data)
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun deleteStarmapLink(starmapId: String, linkId: String): BridgeResult<Boolean> = appService.deleteStarmapLink(starmapId, linkId)

    fun findStarmapReferences(targetStarmapId: String): BridgeResult<List<uniffi.writer_core.StarMapReferenceDto>> {
        return appService.findStarmapReferences(targetStarmapId)
    }

    private fun refreshRawCache(starmapId: String): BridgeResult<StarMapRawCache> {
        return when (val result = appService.getStarMapGraph(starmapId)) {
            is BridgeResult.Success -> {
                try {
                    val cache = result.data.toRawCache()
                    rawCacheByStarmapId[starmapId] = cache
                    BridgeResult.Success(cache)
                } catch (e: Exception) {
                    BridgeResult.Error(ResultEnvelope.error("CONVERSION_ERROR", "Failed to cache starmap graph: ${e.message}"))
                }
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }
}

private fun StarMapMetaDto.toModel(): StarMapMeta = StarMapMeta(
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

private fun StarMapGraphDto.toRawCache(): StarMapRawCache = StarMapRawCache(
    nodes = nodes.associateByTo(mutableMapOf()) { it.id },
    edges = edges.associateByTo(mutableMapOf()) { it.id }
)

private fun StarMapGraphDto.toModel(): StarMapData = StarMapData(
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

private fun StarMapNodeDto.toGraphNode(): StarMapGraphNode = StarMapGraphNode(
    id = id,
    title = title,
    kind = kind.toModel(),
    payload = payload.toPayloadMap(),
    tags = tags,
    createdAt = createdAt.toLong(),
    updatedAt = updatedAt.toLong()
)

private fun StarMapGraphNode.toDto(base: StarMapNodeDto?): StarMapNodeDto {
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

private fun StarMapGraphNode.toNewDefaultNoteNodeDto(
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

private fun StarMapEdgeDto.toGraphEdge(): StarMapGraphEdge = StarMapGraphEdge(
    id = id,
    from = from ?: "",
    to = to ?: "",
    kind = kind.toModel(),
    label = label,
    payload = payload.toPayloadMap(),
    createdAt = createdAt.toLong(),
    updatedAt = updatedAt.toLong()
)

private fun StarMapLayoutData.toDto(cache: StarMapRawCache?): StarMapLayoutDto = StarMapLayoutDto(
    kind = kind.toDto(),
    nodes = nodes.map { it.toDto(cache?.layoutNodes?.get(it.nodeId)) }
)

private fun StarMapLayoutNodeData.toDto(base: StarMapLayoutNodeDto?): StarMapLayoutNodeDto = base?.copy(
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

private fun StarMapNodeKindDto.toModel(): StarMapNodeKind = when (this) {
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

private fun StarMapNodeKind.toDto(): StarMapNodeKindDto = when (this) {
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

private fun StarMapLayoutKind.toDto(): StarMapLayoutKindDto = when (this) {
    StarMapLayoutKind.Freeform -> StarMapLayoutKindDto.FREEFORM
    StarMapLayoutKind.AutoRadial -> StarMapLayoutKindDto.AUTO_RADIAL
    StarMapLayoutKind.Custom -> StarMapLayoutKindDto.CUSTOM
}

private fun StarMapEdgeKindDto.toModel(): StarMapEdgeKind = when (this) {
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

private fun defaultStarMapDisplayPolicy() = StarMapDisplayPolicyDto(
    importance = 1f,
    minVisibleScale = 0f,
    titleScale = 1f,
    summaryScale = 1f,
    detailScale = 1f,
    maxPreviewChars = 120u,
    minReadablePx = 12f
)

@Suppress("UNCHECKED_CAST")
private fun String?.toPayloadMap(): Map<String, Any>? {
    if (isNullOrBlank()) return null
    return try {
        starMapPayloadGson.fromJson(this, Map::class.java) as? Map<String, Any>
    } catch (_: Exception) {
        null
    }
}
