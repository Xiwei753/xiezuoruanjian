package com.xiwei.sujian.feature.starmap.data.interop

import com.xiwei.sujian.feature.starmap.data.model.StarMapEdgeEndpointData
import com.xiwei.sujian.feature.starmap.data.model.StarMapEndpointPathData
import com.xiwei.sujian.feature.starmap.data.model.StarMapEndpointPathSegmentData
import com.xiwei.sujian.feature.starmap.data.model.StarMapGraphEdge
import uniffi.writer_core.StarMapEdgeDto
import uniffi.writer_core.StarMapEdgeEndpointDto
import uniffi.writer_core.StarMapEndpointPathDto

internal fun StarMapEdgeDto.toGraphEdge(): StarMapGraphEdge =
    StarMapGraphEdge(
        id = id,
        from = from ?: "",
        to = to ?: "",
        kind = kind.toModel(),
        label = label,
        payload = payload.toPayloadMap(),
        fromTarget = fromTarget?.toModel(),
        toTarget = toTarget?.toModel(),
        fromEndpoint = fromEndpoint?.toModel(),
        toEndpoint = toEndpoint?.toModel(),
        fromEndpointPath = fromEndpointPath?.toModel(),
        toEndpointPath = toEndpointPath?.toModel(),
        createdAt = createdAt.toLong(),
        updatedAt = updatedAt.toLong(),
    )

internal fun StarMapEdgeEndpointDto.toModel(): StarMapEdgeEndpointData =
    StarMapEdgeEndpointData(
        kind = kind,
        nodeId = nodeId,
        anchorId = anchorId,
        deepTarget = target?.toModel(),
    )

internal fun StarMapEndpointPathDto.toModel(): StarMapEndpointPathData =
    StarMapEndpointPathData(
        segments = segments.map { StarMapEndpointPathSegmentData(kind = it.kind, starmapId = it.starmapId) },
        endpoint = endpoint.toModel(),
    )
