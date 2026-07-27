package com.xiwei.sujian.data

import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapEdgeKind
import com.xiwei.sujian.model.StarMapEdgeRenderData
import com.xiwei.sujian.model.StarMapGraphData
import com.xiwei.sujian.model.StarMapGraphEdge
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapLayoutData
import com.xiwei.sujian.model.StarMapLayoutKind
import com.xiwei.sujian.model.StarMapLayoutNodeData
import com.xiwei.sujian.model.StarMapMeta
import com.xiwei.sujian.model.StarMapMotionPolicyData
import com.xiwei.sujian.model.StarMapNodeKind
import com.xiwei.sujian.model.StarMapViewportData
import com.google.gson.Gson
import uniffi.writer_core.StarMapDisplayPolicyDto
import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapEdgeKindDto
import uniffi.writer_core.StarMapEdgeRenderDto
import uniffi.writer_core.StarMapEmbedDto
import uniffi.writer_core.StarMapEmbedPatchInputDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapLayoutDto
import uniffi.writer_core.StarMapLayoutKindDto
import uniffi.writer_core.StarMapLayoutNodeDto
import uniffi.writer_core.StarMapLinkDto
import uniffi.writer_core.StarMapLinkPatchInputDto
import uniffi.writer_core.StarMapMetaDto
import uniffi.writer_core.StarMapMotionPolicyDto
import uniffi.writer_core.StarMapNodeContentDto
import uniffi.writer_core.StarMapNodeDto
import uniffi.writer_core.StarMapNodeKindDto
import uniffi.writer_core.StarMapOpenBehaviorDto
import uniffi.writer_core.StarMapProvenanceDto
import uniffi.writer_core.StarMapReferenceDto
import uniffi.writer_core.StarMapReviewStatusDto
import uniffi.writer_core.StarMapSourceKindDto
import uniffi.writer_core.StarMapViewportDto

private val starMapPayloadGson = Gson()

private data class StarMapRawCache(
    val graph: StarMapGraphDto? = null,
    val nodes: MutableMap<String, StarMapNodeDto> = mutableMapOf(),
    val edges: MutableMap<String, StarMapEdgeDto> = mutableMapOf(),
    val layoutNodes: MutableMap<String, StarMapLayoutNodeDto> = mutableMapOf()
)

/**
 * 星图 领域 Bridge。
 *
 * 从 AppServiceBridge 拆出，负责星图相关操作。
 * 通过 WriterAppServiceHolder 直接调用 Core 服务，不再依赖 AppServiceBridge。
 *
 * 提供两层 API：
 * - DTO 层（listStarMaps / getStarMapGraph 等）：返回原始 DTO，供 AppServiceBridge 门面委托
 * - Model 层（listStarmaps / getStarmapGraph 等）：返回 Android 端模型，供 UI 层使用
 */
class StarMapBridge internal constructor(private val holder: WriterAppServiceHolder) {
    companion object {
        private const val TAG = "StarMapBridge"
    }

    private val rawCacheByStarmapId = mutableMapOf<String, StarMapRawCache>()

    // ── DTO 层 API（供 AppServiceBridge 门面委托） ──

    fun listStarMaps(): BridgeResult<List<StarMapMetaDto>> = holder.wrapResult {
        holder.service.listStarmaps()
    }

    fun getStarMapGraph(starmapId: String): BridgeResult<StarMapGraphDto> = holder.wrapResult {
        holder.service.getStarmapGraph(starmapId)
    }

    fun createStarMap(title: String, desc: String): BridgeResult<StarMapMetaDto> = holder.wrapResult {
        holder.service.createStarmap(title, desc)
    }

    fun addStarMapNode(starmapId: String, node: StarMapNodeDto, x: Float, y: Float): BridgeResult<StarMapNodeDto> = holder.wrapResult {
        holder.service.addStarmapNode(starmapId, node, x, y)
    }

    fun saveStarMapLayout(starmapId: String, layout: StarMapLayoutDto): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.saveStarmapLayout(starmapId, layout)
    }

    fun getStarMapViewport(starmapId: String): BridgeResult<StarMapViewportDto> = holder.wrapResult {
        holder.service.getStarmapViewport(starmapId)
    }

    fun saveStarMapViewport(starmapId: String, viewport: StarMapViewportDto): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.saveStarmapViewport(starmapId, viewport)
    }

    fun computeStarMapEdgeRenders(graph: StarMapGraphDto, layout: StarMapLayoutDto): BridgeResult<List<StarMapEdgeRenderDto>> = holder.wrapResult {
        holder.service.computeStarmapEdgeRenders(graph, layout)
    }

    fun hitTestStarMapNode(layout: StarMapLayoutDto, x: Float, y: Float): BridgeResult<String?> = holder.wrapResult {
        holder.service.hitTestStarmapNode(layout, x, y)
    }

    fun addStarmapEmbed(starmapId: String, embed: StarMapEmbedDto): BridgeResult<StarMapEmbedDto> = holder.wrapResult {
        holder.service.addStarmapEmbed(starmapId, embed)
    }

    fun updateStarmapEmbed(starmapId: String, instanceId: String, patch: StarMapEmbedPatchInputDto): BridgeResult<StarMapEmbedDto> = holder.wrapResult {
        holder.service.updateStarmapEmbed(starmapId, instanceId, patch)
    }

    fun deleteStarmapEmbed(starmapId: String, instanceId: String): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.deleteStarmapEmbed(starmapId, instanceId)
    }

    fun addStarmapLink(starmapId: String, link: StarMapLinkDto): BridgeResult<StarMapLinkDto> = holder.wrapResult {
        holder.service.addStarmapLink(starmapId, link)
    }

    fun updateStarmapLink(starmapId: String, linkId: String, patch: StarMapLinkPatchInputDto): BridgeResult<StarMapLinkDto> = holder.wrapResult {
        holder.service.updateStarmapLink(starmapId, linkId, patch)
    }

    fun deleteStarmapLink(starmapId: String, linkId: String): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.deleteStarmapLink(starmapId, linkId)
    }

    fun findStarmapReferences(targetStarmapId: String): BridgeResult<List<StarMapReferenceDto>> = holder.wrapResult {
        holder.service.findStarmapReferences(targetStarmapId)
    }

    fun getStarMapMotionPolicy(): BridgeResult<StarMapMotionPolicyData> {
        return holder.wrapResult {
            val dto = holder.service.getStarmapMotionPolicy()
            StarMapMotionPolicyData(
                enabled = dto.enabled,
                idleWobbleEnabled = dto.idleWobbleEnabled,
                idleAmplitudeVp = dto.idleAmplitudeVp,
                idlePeriodMs = dto.idlePeriodMs.toInt(),
                dragLiftScale = dto.dragLiftScale,
                dragShadowBoost = dto.dragShadowBoost,
                settleDurationMs = dto.settleDurationMs.toInt(),
                reduceMotion = dto.reduceMotion
            )
        }
    }

    // ── Model 层 API（供 UI 层使用，保留向后兼容） ──

    fun listStarmaps(): BridgeResult<List<StarMapMeta>> {
        return when (val result = listStarMaps()) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.map { it.toModel() })
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun createStarmap(title: String, desc: String): BridgeResult<StarMapMeta> {
        return when (val result = createStarMap(title, desc)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun getStarmapGraph(starmapId: String): BridgeResult<StarMapData> {
        return when (val result = getStarMapGraph(starmapId)) {
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
        return when (val result = addStarMapNode(starmapId, node.toDto(cache.nodes[node.id]), x, y)) {
            is BridgeResult.Success -> {
                rawCacheByStarmapId.getOrPut(starmapId) { StarMapRawCache() }
                    .nodes[result.data.id] = result.data
                BridgeResult.Success(result.data.toGraphNode())
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun updateStarmapNode(starmapId: String, nodeId: String, title: String? = null, kind: StarMapNodeKind? = null, tags: List<String>? = null): BridgeResult<StarMapGraphNode> {
        return BridgeResult.NotLoaded
    }

    fun deleteStarmapNode(starmapId: String, nodeId: String): BridgeResult<Boolean> {
        return BridgeResult.NotLoaded
    }

    fun addStarmapEdge(starmapId: String, from: String, to: String, kind: StarMapEdgeKind = StarMapEdgeKind.RelatedTo, label: String? = null): BridgeResult<StarMapGraphEdge> {
        return BridgeResult.NotLoaded
    }

    fun deleteStarmapEdge(starmapId: String, edgeId: String): BridgeResult<Boolean> {
        return BridgeResult.NotLoaded
    }

    fun saveStarmapLayout(starmapId: String, layout: StarMapLayoutData): BridgeResult<Boolean> {
        val cache = when (val cacheResult = refreshRawCache(starmapId)) {
            is BridgeResult.Success -> cacheResult.data
            is BridgeResult.Error -> return BridgeResult.Error(cacheResult.envelope)
            BridgeResult.NotLoaded -> return BridgeResult.NotLoaded
        }
        val dto = layout.toDto(cache)
        return when (val result = saveStarMapLayout(starmapId, dto)) {
            is BridgeResult.Success -> {
                cache.layoutNodes.clear()
                cache.layoutNodes.putAll(dto.nodes.associateBy { it.nodeId })
                BridgeResult.Success(result.data)
            }
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun getStarmapViewport(starmapId: String): BridgeResult<StarMapViewportData> {
        return when (val result = getStarMapViewport(starmapId)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun saveStarmapViewport(starmapId: String, viewport: StarMapViewportData): BridgeResult<Boolean> {
        return saveStarMapViewport(starmapId, viewport.toDto())
    }

    fun computeEdgeRenders(data: StarMapData): BridgeResult<List<StarMapEdgeRenderData>> {
        val cache = when (val cacheResult = getRawCache(data.graph.starmapId)) {
            is BridgeResult.Success -> cacheResult.data
            is BridgeResult.Error -> return BridgeResult.Error(cacheResult.envelope)
            BridgeResult.NotLoaded -> return BridgeResult.NotLoaded
        }
        val graph = cache.graph ?: return BridgeResult.Error(
            ResultEnvelope.error("STAR_MAP_CACHE_MISSING", "Raw starmap graph is not available")
        )
        return when (val result = computeStarMapEdgeRenders(graph, data.layout.toDto(cache))) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.map { it.toModel() })
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun hitTestStarmapNode(data: StarMapData, x: Float, y: Float): BridgeResult<String?> {
        val cache = when (val cacheResult = getRawCache(data.graph.starmapId)) {
            is BridgeResult.Success -> cacheResult.data
            is BridgeResult.Error -> return BridgeResult.Error(cacheResult.envelope)
            BridgeResult.NotLoaded -> return BridgeResult.NotLoaded
        }
        return hitTestStarMapNode(data.layout.toDto(cache), x, y)
    }

    fun getMotionPolicy(): BridgeResult<StarMapMotionPolicyData> = getStarMapMotionPolicy()

    fun flushStarmapStore(starmapId: String): BridgeResult<Boolean> {
        return try {
            holder.service.flushStarmapStore(starmapId)
            BridgeResult.Success(true)
        } catch (e: Exception) {
            BridgeResult.Error(ResultEnvelope.error("FLUSH_ERROR", "Failed to flush starmap store: ${e.message}"))
        }
    }

    fun closeStarmapStore(starmapId: String): BridgeResult<Boolean> {
        return try {
            holder.service.closeStarmapStore(starmapId)
            BridgeResult.Success(true)
        } catch (e: Exception) {
            BridgeResult.Error(ResultEnvelope.error("CLOSE_ERROR", "Failed to close starmap store: ${e.message}"))
        }
    }

    fun flushAllStarmapStores(): BridgeResult<Boolean> {
        return try {
            holder.service.flushAllStarmapStores()
            BridgeResult.Success(true)
        } catch (e: Exception) {
            BridgeResult.Error(ResultEnvelope.error("FLUSH_ALL_ERROR", "Failed to flush all starmap stores: ${e.message}"))
        }
    }

    // ── 内部缓存 ──

    private fun refreshRawCache(starmapId: String): BridgeResult<StarMapRawCache> {
        return when (val result = getStarMapGraph(starmapId)) {
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

    private fun getRawCache(starmapId: String): BridgeResult<StarMapRawCache> {
        rawCacheByStarmapId[starmapId]?.let { return BridgeResult.Success(it) }
        return refreshRawCache(starmapId)
    }
}

// ── DTO ↔ Model 转换 ──

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
    graph = this,
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

private fun StarMapEdgeRenderDto.toModel(): StarMapEdgeRenderData = StarMapEdgeRenderData(
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

private fun StarMapViewportDto.toModel(): StarMapViewportData = StarMapViewportData(
    scale = scale,
    offsetX = offsetX,
    offsetY = offsetY,
    width = width,
    height = height
)

private fun StarMapViewportData.toDto(): StarMapViewportDto = StarMapViewportDto(
    scale = scale,
    offsetX = offsetX,
    offsetY = offsetY,
    width = width,
    height = height
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

private fun StarMapEdgeKind.toDto(): StarMapEdgeKindDto = when (this) {
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
private fun String?.toPayloadMap(): Map<String, Any>? {
    if (isNullOrBlank()) return null
    return try {
        starMapPayloadGson.fromJson(this, Map::class.java) as? Map<String, Any>
    } catch (_: Exception) {
        null
    }
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
