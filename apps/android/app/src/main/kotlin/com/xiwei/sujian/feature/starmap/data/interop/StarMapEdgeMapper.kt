package com.xiwei.sujian.feature.starmap.data.interop

import com.xiwei.sujian.feature.starmap.data.model.StarMapGraphEdge
import uniffi.writer_core.StarMapEdgeDto

internal fun StarMapEdgeDto.toGraphEdge(): StarMapGraphEdge =
    StarMapGraphEdge(
        id = id,
        from = from.toModel(),
        to = to.toModel(),
        kind = kind.toModel(),
        label = label,
        payload = payload.toPayloadMap(),
        createdAt = createdAt.toLong(),
        updatedAt = updatedAt.toLong(),
    )
