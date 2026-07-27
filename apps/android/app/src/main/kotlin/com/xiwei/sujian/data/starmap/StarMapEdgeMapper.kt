package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.StarMapGraphEdge
import uniffi.writer_core.StarMapEdgeDto

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
