package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.MindMapBounds
import com.xiwei.writerapp.model.MindMapEdge
import com.xiwei.writerapp.model.MindMapNode
import com.xiwei.writerapp.model.MindMapNodeKind
import com.xiwei.writerapp.model.MindMapSnapshot
import uniffi.writer_core.MindMapBoundsDto
import uniffi.writer_core.MindMapNodeKindDto
import uniffi.writer_core.MindMapSnapshotDto
import uniffi.writer_core.MindMapSnapshotEdgeDto
import uniffi.writer_core.MindMapSnapshotNodeDto

class MindMapBridge(private val appService: AppServiceBridge) {

    fun getMindMapSnapshot(projectId: String): BridgeResult<MindMapSnapshot> {
        return when (val result = appService.getMindMapSnapshot(projectId)) {
            is BridgeResult.Success -> {
                try {
                    BridgeResult.Success(result.data.toModel())
                } catch (e: Exception) {
                    BridgeResult.Error(ResultEnvelope.error("CONVERSION_ERROR", "Failed to convert mindmap snapshot: ${e.message}"))
                }
            }
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }
}

private fun MindMapSnapshotDto.toModel(): MindMapSnapshot = MindMapSnapshot(
    projectId = projectId,
    layoutKind = layoutKind,
    nodes = nodes.map { it.toModel() },
    edges = edges.map { it.toModel() },
    bounds = bounds.toModel(),
    generatedAt = generatedAt.toLong()
)

private fun MindMapSnapshotNodeDto.toModel(): MindMapNode = MindMapNode(
    id = id,
    title = title,
    kind = kind.toModel(),
    x = x,
    y = y,
    width = width,
    height = height,
    radius = radius,
    collapsed = collapsed,
    anchorCount = anchorCount.toInt(),
    brokenLink = brokenLink,
    tags = tags
)

private fun MindMapSnapshotEdgeDto.toModel(): MindMapEdge = MindMapEdge(
    id = id,
    from = from,
    to = to,
    kind = kind,
    label = label
)

private fun MindMapBoundsDto.toModel(): MindMapBounds = MindMapBounds(
    minX = minX,
    minY = minY,
    maxX = maxX,
    maxY = maxY
)

private fun MindMapNodeKindDto.toModel(): MindMapNodeKind = when (this) {
    MindMapNodeKindDto.Project -> MindMapNodeKind.Project
    MindMapNodeKindDto.Volume -> MindMapNodeKind.Volume
    MindMapNodeKindDto.Chapter -> MindMapNodeKind.Chapter
    MindMapNodeKindDto.TextAnchor -> MindMapNodeKind.TextAnchor
    MindMapNodeKindDto.Character -> MindMapNodeKind.Character
    MindMapNodeKindDto.Event -> MindMapNodeKind.Event
    MindMapNodeKindDto.Location -> MindMapNodeKind.Location
    MindMapNodeKindDto.Item -> MindMapNodeKind.Item
    MindMapNodeKindDto.Concept -> MindMapNodeKind.Concept
    MindMapNodeKindDto.Theme -> MindMapNodeKind.Theme
    MindMapNodeKindDto.Note -> MindMapNodeKind.Note
    MindMapNodeKindDto.Organization -> MindMapNodeKind.Organization
    MindMapNodeKindDto.Timeline -> MindMapNodeKind.Timeline
    MindMapNodeKindDto.Plot -> MindMapNodeKind.Plot
    MindMapNodeKindDto.Foreshadowing -> MindMapNodeKind.Foreshadowing
    MindMapNodeKindDto.Custom -> MindMapNodeKind.Custom
}
