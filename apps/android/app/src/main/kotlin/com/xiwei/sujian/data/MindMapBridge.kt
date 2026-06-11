package com.xiwei.sujian.data

/**
 * LEGACY: MindMap Bridge — 已废弃，仅保留用于旧数据迁移兼容。正式图谱路线为 StarMapBridge。
 */

import com.xiwei.sujian.model.MindMapBounds
import com.xiwei.sujian.model.MindMapEdge
import com.xiwei.sujian.model.MindMapNode
import com.xiwei.sujian.model.MindMapNodeKind
import com.xiwei.sujian.model.MindMapSnapshot
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
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
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
    MindMapNodeKindDto.PROJECT -> MindMapNodeKind.Project
    MindMapNodeKindDto.VOLUME -> MindMapNodeKind.Volume
    MindMapNodeKindDto.CHAPTER -> MindMapNodeKind.Chapter
    MindMapNodeKindDto.TEXT_ANCHOR -> MindMapNodeKind.TextAnchor
    MindMapNodeKindDto.CHARACTER -> MindMapNodeKind.Character
    MindMapNodeKindDto.EVENT -> MindMapNodeKind.Event
    MindMapNodeKindDto.LOCATION -> MindMapNodeKind.Location
    MindMapNodeKindDto.ITEM -> MindMapNodeKind.Item
    MindMapNodeKindDto.CONCEPT -> MindMapNodeKind.Concept
    MindMapNodeKindDto.THEME -> MindMapNodeKind.Theme
    MindMapNodeKindDto.NOTE -> MindMapNodeKind.Note
    MindMapNodeKindDto.ORGANIZATION -> MindMapNodeKind.Organization
    MindMapNodeKindDto.TIMELINE -> MindMapNodeKind.Timeline
    MindMapNodeKindDto.PLOT -> MindMapNodeKind.Plot
    MindMapNodeKindDto.FORESHADOWING -> MindMapNodeKind.Foreshadowing
    MindMapNodeKindDto.CUSTOM -> MindMapNodeKind.Custom
}
