package com.xiwei.sujian.feature.starmap.data.interop

import com.xiwei.sujian.feature.starmap.model.StarMapData
import com.xiwei.sujian.feature.starmap.model.StarMapGraphData
import com.xiwei.sujian.feature.starmap.model.StarMapLayoutData
import com.xiwei.sujian.feature.starmap.model.StarMapLayoutKind
import com.xiwei.sujian.feature.starmap.model.StarMapPhasedSnapshotResult
import uniffi.writer_core.PhasedSnapshotRequestDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapPhasedSnapshotDto
import com.xiwei.sujian.feature.starmap.data.StarMapRawCache

internal fun PhasedSnapshotRequestDto.Companion.create(
    targetPhase: String = "PrefetchNearbyObjects",
    sinceRevision: ULong = 0u,
): PhasedSnapshotRequestDto =
    PhasedSnapshotRequestDto(
        targetPhase = targetPhase,
        sinceRevision = sinceRevision,
    )

internal fun StarMapPhasedSnapshotDto.toRawCache(): StarMapRawCache =
    StarMapRawCache(
        graph =
            StarMapGraphDto(
                schemaVersion = 1u,
                id = starmapId,
                starmapId = starmapId,
                title = title,
                nodes = nodes,
                edges = edges,
                embeds = embeds,
                links = links,
                hyperlinks = hyperlinks,
                createdAt = 0u,
                updatedAt = 0u,
            ),
        nodes = nodes.associateByTo(mutableMapOf()) { it.id },
        edges = edges.associateByTo(mutableMapOf()) { it.id },
        embeds = embeds.associateByTo(mutableMapOf()) { it.instanceId },
        links = links.associateByTo(mutableMapOf()) { it.linkId },
        hyperlinks = hyperlinks.associateByTo(mutableMapOf()) { it.hyperlinkId },
        layoutNodes = layout?.nodes?.associateByTo(mutableMapOf()) { it.nodeId } ?: mutableMapOf(),
        layoutKind = layout?.kind ?: uniffi.writer_core.StarMapLayoutKindDto.FREEFORM,
        loadPhase = loadPhase,
        packageRevision = packageRevision,
        sinceRevision = sinceRevision,
        complete = complete,
        viewport = viewport,
        diagnostics = diagnostics.map { it.toModel() },
        deletedNodeIds = deletedNodeIds.toMutableSet(),
        deletedEdgeIds = deletedEdgeIds.toMutableSet(),
        deletedEmbedIds = deletedEmbedIds.toMutableSet(),
        deletedLinkIds = deletedLinkIds.toMutableSet(),
        deletedHyperlinkIds = deletedHyperlinkIds.toMutableSet(),
    )

internal fun StarMapPhasedSnapshotDto.toSnapshotResult(): StarMapPhasedSnapshotResult {
    val layoutData =
        layout?.toModel() ?: StarMapLayoutData(
            kind = StarMapLayoutKind.Freeform,
            nodes = emptyList(),
        )
    val data =
        StarMapData(
            graph =
                StarMapGraphData(
                    schemaVersion = 0,
                    id = starmapId,
                    starmapId = starmapId,
                    title = title,
                    nodes = nodes.map { it.toGraphNode() },
                    edges = edges.map { it.toGraphEdge() },
                    createdAt = 0L,
                    updatedAt = 0L,
                ),
            layout = layoutData,
            viewport = viewport?.toModel() ?: com.xiwei.sujian.feature.starmap.model.StarMapViewportData(),
            embeds = embeds.map { it.toModel() },
            links = links.map { it.toModel() },
            hyperlinks = hyperlinks.map { it.toModel() },
            loadPhase = loadPhase,
            packageRevision = packageRevision,
            sinceRevision = sinceRevision,
            complete = complete,
        )
    return StarMapPhasedSnapshotResult(
        data = data,
        diagnostics = diagnostics.map { it.toModel() },
    )
}
