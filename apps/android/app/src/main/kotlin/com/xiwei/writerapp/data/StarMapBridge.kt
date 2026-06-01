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
        val result = appService.listStarMaps()
        if (result is BridgeResult.Success) {
            return BridgeResult.Success(result.data.map { it.toModel() })
        } else if (result is BridgeResult.Error) {
            return BridgeResult.Error(result.envelope)
        } else {
            return BridgeResult.NotLoaded
        }
    }

    fun createStarmap(title: String, desc: String): BridgeResult<StarMapMeta> {
        val result = appService.createStarMap(title, desc)
        if (result is BridgeResult.Success) {
            return BridgeResult.Success(result.data.toModel())
        } else if (result is BridgeResult.Error) {
            return BridgeResult.Error(result.envelope)
        } else {
            return BridgeResult.NotLoaded
        }
    }

    fun getStarmapGraph(starmapId: String): BridgeResult<StarMapData> {
        val result = appService.getStarMapGraph(starmapId)
        if (result is BridgeResult.Success) {
            return try {
                BridgeResult.Success(result.data.toModel())
            } catch (e: Exception) {
                BridgeResult.Error(ResultEnvelope.error("CONVERSION_ERROR", "Failed to convert starmap graph: ${e.message}"))
            }
        } else if (result is BridgeResult.Error) {
            return BridgeResult.Error(result.envelope)
        } else {
            return BridgeResult.NotLoaded
        }
    }

    fun addStarmapNode(starmapId: String, nodeJson: String, x: Float, y: Float): BridgeResult<StarMapGraphNode> {
        val result = appService.addStarMapNode(starmapId, nodeJson, x, y)
        if (result is BridgeResult.Success) {
            return BridgeResult.Success(result.data.toGraphNode())
        } else if (result is BridgeResult.Error) {
            return BridgeResult.Error(result.envelope)
        } else {
            return BridgeResult.NotLoaded
        }
    }

    fun saveStarmapLayout(starmapId: String, layoutJson: String): BridgeResult<Boolean> = appService.saveStarMapLayout(starmapId, layoutJson)

    fun addStarmapEmbed(starmapId: String, embedJson: String): BridgeResult<String> {
        val result = appService.addStarmapEmbed(starmapId, embedJson)
        if (result is BridgeResult.Success) {
            return BridgeResult.Success(result.data.toString())
        } else if (result is BridgeResult.Error) {
            return BridgeResult.Error(result.envelope)
        } else {
            return BridgeResult.NotLoaded
        }
    }

    fun updateStarmapEmbed(starmapId: String, instanceId: String, patchJson: String): BridgeResult<String> {
        val result = appService.updateStarmapEmbed(starmapId, instanceId, patchJson)
        if (result is BridgeResult.Success) {
            return BridgeResult.Success(result.data.toString())
        } else if (result is BridgeResult.Error) {
            return BridgeResult.Error(result.envelope)
        } else {
            return BridgeResult.NotLoaded
        }
    }

    fun deleteStarmapEmbed(starmapId: String, instanceId: String): BridgeResult<Boolean> = appService.deleteStarmapEmbed(starmapId, instanceId)

    fun addStarmapLink(starmapId: String, linkJson: String): BridgeResult<String> {
        val result = appService.addStarmapLink(starmapId, linkJson)
        if (result is BridgeResult.Success) {
            return BridgeResult.Success(result.data.toString())
        } else if (result is BridgeResult.Error) {
            return BridgeResult.Error(result.envelope)
        } else {
            return BridgeResult.NotLoaded
        }
    }

    fun updateStarmapLink(starmapId: String, linkId: String, patchJson: String): BridgeResult<String> {
        val result = appService.updateStarmapLink(starmapId, linkId, patchJson)
        if (result is BridgeResult.Success) {
            return BridgeResult.Success(result.data.toString())
        } else if (result is BridgeResult.Error) {
            return BridgeResult.Error(result.envelope)
        } else {
            return BridgeResult.NotLoaded
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
