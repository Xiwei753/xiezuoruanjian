package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.model.StarMapLayoutData
import com.xiwei.sujian.model.StarMapLayoutKind
import com.xiwei.sujian.model.StarMapLayoutNodeData
import uniffi.writer_core.StarMapLayoutDto
import uniffi.writer_core.StarMapLayoutKindDto
import uniffi.writer_core.StarMapLayoutNodeDto

internal fun StarMapLayoutData.toDto(cache: StarMapRawCache?): StarMapLayoutDto =
    StarMapLayoutDto(
        kind = kind.toDto(),
        nodes = nodes.map { it.toDto(cache?.layoutNodes?.get(it.nodeId)) },
    )

internal fun StarMapLayoutNodeData.toDto(base: StarMapLayoutNodeDto?): StarMapLayoutNodeDto =
    base?.copy(
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
        orbitGroup = orbitGroup,
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
        orbitGroup = orbitGroup,
    )

internal fun StarMapLayoutDto.toModel(): StarMapLayoutData =
    StarMapLayoutData(
        kind =
            when (kind) {
                StarMapLayoutKindDto.FREEFORM -> StarMapLayoutKind.Freeform
                StarMapLayoutKindDto.AUTO_RADIAL -> StarMapLayoutKind.AutoRadial
                StarMapLayoutKindDto.CUSTOM -> StarMapLayoutKind.Custom
            },
        nodes = nodes.map { it.toModel() },
    )

internal fun StarMapLayoutNodeDto.toModel(): StarMapLayoutNodeData =
    StarMapLayoutNodeData(
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
        orbitGroup = orbitGroup,
    )
