package com.xiwei.writerapp.data

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
import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapEdgeKindDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapLayoutDto
import uniffi.writer_core.StarMapLayoutKindDto
import uniffi.writer_core.StarMapLayoutNodeDto
import uniffi.writer_core.StarMapMetaDto
import uniffi.writer_core.StarMapNodeDto
import uniffi.writer_core.StarMapNodeKindDto

class StarMapBridge(private val appService: AppServiceBridge) {

    fun listStarmaps(): BridgeResult<List<StarMapMeta>> {
        return when (val result = appService.listStarMaps()) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.map { it.toModel() })
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }

    fun createStarmap(title: String, desc: String): BridgeResult<StarMapMeta> {
        return when (val result = appService.createStarMap(title, desc)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }

    fun getStarmapGraph(starmapId: String): BridgeResult<StarMapData> {
        return when (val result = appService.getStarMapGraph(starmapId)) {
            is BridgeResult.Success -> {
                try {
                    BridgeResult.Success(result.data.toModel())
                } catch (e: Exception) {
                    BridgeResult.Error(ResultEnvelope.error("CONVERSION_ERROR", "Failed to convert starmap graph: ${e.message}"))
                }
            }
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }

    fun addStarmapNode(starmapId: String, nodeJson: String, x: Float, y: Float): BridgeResult<StarMapGraphNode> {
        return when (val result = appService.addStarMapNode(starmapId, nodeJson, x, y)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toGraphNode())
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }

    fun saveStarmapLayout(starmapId: String, layoutJson: String): BridgeResult<Boolean> = appService.saveStarMapLayout(starmapId, layoutJson)

    fun addStarmapEmbed(starmapId: String, embedJson: String): BridgeResult<String> {
        return when (val result = appService.addStarmapEmbed(starmapId, embedJson)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toString())
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }

    fun updateStarmapEmbed(starmapId: String, instanceId: String, patchJson: String): BridgeResult<String> {
        return when (val result = appService.updateStarmapEmbed(starmapId, instanceId, patchJson)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toString())
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }

    fun deleteStarmapEmbed(starmapId: String, instanceId: String): BridgeResult<Boolean> = appService.deleteStarmapEmbed(starmapId, instanceId)

    fun addStarmapLink(starmapId: String, linkJson: String): BridgeResult<String> {
        return when (val result = appService.addStarmapLink(starmapId, linkJson)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toString())
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }

    fun updateStarmapLink(starmapId: String, linkId: String, patchJson: String): BridgeResult<String> {
        return when (val result = appService.updateStarmapLink(starmapId, linkId, patchJson)) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toString())
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }

    fun deleteStarmapLink(starmapId: String, linkId: String): BridgeResult<Boolean> = appService.deleteStarmapLink(starmapId, linkId)

    fun findStarmapReferences(targetStarmapId: String): BridgeResult<List<uniffi.writer_core.StarMapReferenceDto>> {
        return appService.findStarmapReferences(targetStarmapId)
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
    payload = null,
    tags = tags,
    createdAt = createdAt.toLong(),
    updatedAt = updatedAt.toLong()
)

private fun StarMapEdgeDto.toGraphEdge(): StarMapGraphEdge = StarMapGraphEdge(
    id = id,
    from = from ?: "",
    to = to ?: "",
    kind = kind.toModel(),
    label = label,
    payload = null,
    createdAt = createdAt.toLong(),
    updatedAt = updatedAt.toLong()
)

private fun StarMapNodeKindDto.toModel(): StarMapNodeKind = when (this) {
    StarMapNodeKindDto.Character -> StarMapNodeKind.Character
    StarMapNodeKindDto.Event -> StarMapNodeKind.Event
    StarMapNodeKindDto.Location -> StarMapNodeKind.Location
    StarMapNodeKindDto.Item -> StarMapNodeKind.Item
    StarMapNodeKindDto.Concept -> StarMapNodeKind.Concept
    StarMapNodeKindDto.Theme -> StarMapNodeKind.Theme
    StarMapNodeKindDto.Note -> StarMapNodeKind.Note
    StarMapNodeKindDto.Organization -> StarMapNodeKind.Organization
    StarMapNodeKindDto.Timeline -> StarMapNodeKind.Timeline
    StarMapNodeKindDto.Plot -> StarMapNodeKind.Plot
    StarMapNodeKindDto.Foreshadowing -> StarMapNodeKind.Foreshadowing
    StarMapNodeKindDto.Chapter -> StarMapNodeKind.Chapter
    StarMapNodeKindDto.Custom -> StarMapNodeKind.Custom
}

private fun StarMapEdgeKindDto.toModel(): StarMapEdgeKind = when (this) {
    StarMapEdgeKindDto.Contains -> StarMapEdgeKind.Contains
    StarMapEdgeKindDto.References -> StarMapEdgeKind.References
    StarMapEdgeKindDto.AppearsIn -> StarMapEdgeKind.AppearsIn
    StarMapEdgeKindDto.Causes -> StarMapEdgeKind.Causes
    StarMapEdgeKindDto.RelatedTo -> StarMapEdgeKind.RelatedTo
    StarMapEdgeKindDto.LocatedAt -> StarMapEdgeKind.LocatedAt
    StarMapEdgeKindDto.CharacterRelation -> StarMapEdgeKind.CharacterRelation
    StarMapEdgeKindDto.Timeline -> StarMapEdgeKind.Timeline
    StarMapEdgeKindDto.Foreshadows -> StarMapEdgeKind.Foreshadows
    StarMapEdgeKindDto.Resolves -> StarMapEdgeKind.Resolves
    StarMapEdgeKindDto.DependsOn -> StarMapEdgeKind.DependsOn
    StarMapEdgeKindDto.ConflictsWith -> StarMapEdgeKind.ConflictsWith
    StarMapEdgeKindDto.Custom -> StarMapEdgeKind.Custom
}
