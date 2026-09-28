package com.xiwei.sujian.feature.starmap.data.interop

import com.xiwei.sujian.feature.starmap.data.StarMapRawCache
import com.xiwei.sujian.feature.starmap.data.model.StarMapData
import com.xiwei.sujian.feature.starmap.data.model.StarMapGraphData
import com.xiwei.sujian.feature.starmap.data.model.StarMapPhasedSnapshotResult
import uniffi.writer_core.PhasedSnapshotRequestDto
import uniffi.writer_core.StarMapGraphDto
import uniffi.writer_core.StarMapPhasedSnapshotDto

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
                schemaVersion = schemaVersion,
                starmapId = starmapId,
                nodes = nodes,
                edges = edges,
                embeds = embeds,
                links = links,
                hyperlinks = hyperlinks,
            ),
        nodes = nodes.associateByTo(mutableMapOf()) { it.id },
        edges = edges.associateByTo(mutableMapOf()) { it.id },
        embeds = embeds.associateByTo(mutableMapOf()) { it.instanceId },
        links = links.associateByTo(mutableMapOf()) { it.linkId },
        hyperlinks = hyperlinks.associateByTo(mutableMapOf()) { it.hyperlinkId },
        loadPhase = loadPhase,
        packageRevision = packageRevision,
        sinceRevision = sinceRevision,
        complete = complete,
        diagnostics = diagnostics.map { it.toModel() },
        deletedNodeIds = deletedNodeIds.toMutableSet(),
        deletedEdgeIds = deletedEdgeIds.toMutableSet(),
        deletedEmbedIds = deletedEmbedIds.toMutableSet(),
        deletedLinkIds = deletedLinkIds.toMutableSet(),
        deletedHyperlinkIds = deletedHyperlinkIds.toMutableSet(),
    )

internal fun StarMapPhasedSnapshotDto.toSnapshotResult(): StarMapPhasedSnapshotResult {
    val data =
        StarMapData(
            graph =
                StarMapGraphData(
                    schemaVersion = schemaVersion.toInt(),
                    starmapId = starmapId,
                    nodes = nodes.map { it.toGraphNode() },
                    edges = edges.map { it.toGraphEdge() },
                ),
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
