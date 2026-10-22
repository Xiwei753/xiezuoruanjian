package com.xiwei.sujian.feature.starmap.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.core.designsystem.component.SujianListItem
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.starmap.data.StarMapRepository
import com.xiwei.sujian.feature.starmap.data.model.StarMapData
import com.xiwei.sujian.feature.starmap.data.model.StarMapGraphEdge
import com.xiwei.sujian.feature.starmap.data.model.StarMapMeta
import com.xiwei.sujian.feature.starmap.data.model.StarMapPointData
import com.xiwei.sujian.feature.starmap.data.model.StarMapSelectionOwner
import com.xiwei.sujian.feature.starmap.data.model.StarMapSelectionTarget
import com.xiwei.sujian.feature.starmap.data.model.StarMapTargetPathData
import com.xiwei.sujian.feature.starmap.data.model.positionForPath
import com.xiwei.sujian.feature.starmap.data.model.resolveSelectionTarget
import com.xiwei.sujian.feature.starmap.data.model.selectionCenter
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sqrt

/**
 * 星图入口。
 *
 * - [starmapId] 为空时画星图列表，保留一级「星图」根页；
 * - 搜索结果带 starmapId 时画最小可用详情：节点按 Core position 绘制，
 *   连线端点由 target path 解析，嵌入按自身 position 绘制。
 *
 * 数据全部来自 [StarMapRepository]，不在 Android 端另建星图数据层。
 */
@Composable
fun StarMapScreen(
    starmapId: String?,
    target: StarMapSelectionTarget?,
    onOpenStarmap: (starmapId: String, projectId: String?) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (starmapId == null) {
        StarMapRootScreen(onOpenStarmap = onOpenStarmap, modifier = modifier)
    } else {
        StarMapDetailScreen(starmapId = starmapId, target = target, modifier = modifier)
    }
}

/** 一级星图根页 — 读取现有星图列表，点击进入详情。 */
@Composable
private fun StarMapRootScreen(
    onOpenStarmap: (String, String?) -> Unit,
    modifier: Modifier = Modifier,
) {
    val repository = LocalSujianAppDependencies.current.starmapRepository
    var reloadToken by remember { mutableIntStateOf(0) }
    var state by remember { mutableStateOf<StarMapListState>(StarMapListState.Loading) }

    LaunchedEffect(repository, reloadToken) {
        state = StarMapListState.Loading
        state = loadStarMaps(repository)
    }

    when (val current = state) {
        StarMapListState.Loading ->
            Box(modifier = modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        is StarMapListState.Failed ->
            StarMapMessage(
                title = stringResource(id = R.string.starmap_load_failed),
                actionLabel = stringResource(id = R.string.search_retry),
                onAction = { reloadToken++ },
                modifier = modifier.fillMaxSize(),
            )
        is StarMapListState.Ready -> {
            if (current.maps.isEmpty()) {
                StarMapMessage(
                    title = stringResource(id = R.string.starmap_empty),
                    modifier = modifier.fillMaxSize(),
                )
            } else {
                LazyColumn(
                    modifier = modifier.fillMaxSize(),
                    contentPadding = PaddingValues(vertical = 8.dp),
                ) {
                    items(current.maps, key = { it.starmapId }) { map ->
                        SujianListItem(
                            headline = map.title,
                            supportingText = map.description.takeIf { it.isNotBlank() },
                            onClick = { onOpenStarmap(map.starmapId, map.projectId) },
                        )
                    }
                }
            }
        }
    }
}

/** 星图详情 — 加载 phased snapshot 后由 Canvas 绘制。 */
@Composable
private fun StarMapDetailScreen(
    starmapId: String,
    target: StarMapSelectionTarget?,
    modifier: Modifier = Modifier,
) {
    val repository = LocalSujianAppDependencies.current.starmapRepository
    var reloadToken by remember { mutableIntStateOf(0) }
    var state by remember(starmapId) { mutableStateOf<StarMapDetailState>(StarMapDetailState.Loading) }

    LaunchedEffect(repository, starmapId, reloadToken) {
        state = StarMapDetailState.Loading
        state = loadStarMapDetail(repository, starmapId)
    }

    when (val current = state) {
        StarMapDetailState.Loading ->
            Box(modifier = modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        StarMapDetailState.Failed ->
            StarMapMessage(
                title = stringResource(id = R.string.starmap_load_failed),
                actionLabel = stringResource(id = R.string.search_retry),
                onAction = { reloadToken++ },
                modifier = modifier.fillMaxSize(),
            )
        is StarMapDetailState.Ready ->
            StarMapCanvas(
                data = current.data,
                target = target,
                modifier = modifier.fillMaxSize(),
            )
    }
}

private sealed interface StarMapListState {
    data object Loading : StarMapListState

    data class Ready(val maps: List<StarMapMeta>) : StarMapListState

    data object Failed : StarMapListState
}

private sealed interface StarMapDetailState {
    data object Loading : StarMapDetailState

    data class Ready(val data: StarMapData) : StarMapDetailState

    data object Failed : StarMapDetailState
}

private suspend fun loadStarMaps(repository: StarMapRepository): StarMapListState =
    withContext(Dispatchers.IO) {
        when (val result = repository.listStarmaps()) {
            is BridgeResult.Success -> StarMapListState.Ready(result.data)
            is BridgeResult.Error, BridgeResult.NotLoaded -> StarMapListState.Failed
        }
    }

private suspend fun loadStarMapDetail(
    repository: StarMapRepository,
    starmapId: String,
): StarMapDetailState =
    withContext(Dispatchers.IO) {
        when (val result = repository.getStarmapPhasedSnapshot(starmapId)) {
            is BridgeResult.Success -> StarMapDetailState.Ready(result.data.data)
            is BridgeResult.Error, BridgeResult.NotLoaded -> StarMapDetailState.Failed
        }
    }

@Composable
private fun StarMapMessage(
    title: String,
    modifier: Modifier = Modifier,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
) {
    Column(
        modifier = modifier.padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (actionLabel != null && onAction != null) {
            Spacer(Modifier.height(8.dp))
            TextButton(onClick = onAction) {
                Text(actionLabel)
            }
        }
    }
}

private data class StarMapCamera(
    val scale: Float,
    val offset: Offset,
)

@Suppress("LongMethod", "CyclomaticComplexMethod", "CognitiveComplexMethod")
@Composable
private fun StarMapCanvas(
    data: StarMapData,
    target: StarMapSelectionTarget?,
    modifier: Modifier = Modifier,
) {
    val resolvedTarget = remember(data, target) { target?.let { data.resolveSelectionTarget(it) } }
    var selectedOwner by remember(data, target) { mutableStateOf(resolvedTarget?.owner) }
    var viewport by remember { mutableStateOf(IntSize.Zero) }
    var camera by remember { mutableStateOf(StarMapCamera(1f, Offset.Zero)) }
    val currentCamera by rememberUpdatedState(camera)

    val textMeasurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val nodeRadius = with(density) { 20.dp.toPx() }
    val nodeLabelStyle =
        TextStyle(
            color = MaterialTheme.colorScheme.onSurface,
            fontSize = 11.sp,
            fontWeight = FontWeight.Medium,
        )
    val edgeLabelStyle =
        TextStyle(
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            fontSize = 10.sp,
        )
    val nodeFill = MaterialTheme.colorScheme.secondaryContainer
    val nodeBorder = MaterialTheme.colorScheme.secondary
    val edgeColor = MaterialTheme.colorScheme.outline
    val selectedColor = MaterialTheme.colorScheme.primary
    val embedFill = MaterialTheme.colorScheme.tertiaryContainer
    val embedBorder = MaterialTheme.colorScheme.tertiary
    val surfaceColor = MaterialTheme.colorScheme.surface

    LaunchedEffect(data, viewport, target) {
        if (viewport.width <= 0 || viewport.height <= 0) return@LaunchedEffect
        val fit = fitCamera(data, viewport, padding = 48f)
        val targetCenter = target?.let { data.selectionCenter(it) }
        camera =
            if (targetCenter == null) {
                fit
            } else {
                fit.copy(scale = max(fit.scale, 0.85f)).centerOn(targetCenter, viewport)
            }
    }

    Box(modifier = modifier) {
        Canvas(
            modifier =
                Modifier
                    .fillMaxSize()
                    .onSizeChanged { viewport = it }
                    .pointerInput(data, nodeRadius) {
                        detectTapGestures { tap ->
                            val activeCamera = currentCamera
                            val world =
                                Offset(
                                    x = (tap.x - activeCamera.offset.x) / activeCamera.scale,
                                    y = (tap.y - activeCamera.offset.y) / activeCamera.scale,
                                )
                            selectedOwner =
                                hitTest(
                                    data = data,
                                    world = world,
                                    nodeTolerance = nodeRadius / activeCamera.scale,
                                )
                        }
                    }
                    .pointerInput(Unit) {
                        detectTransformGestures { centroid, pan, zoom, _ ->
                            val activeCamera = currentCamera
                            val oldScale = activeCamera.scale
                            val newScale = (oldScale * zoom).coerceIn(0.05f, 8f)
                            val newOffset =
                                centroid - (centroid - activeCamera.offset) * (newScale / oldScale) + pan
                            camera = StarMapCamera(newScale, newOffset)
                        }
                    },
        ) {
            drawRect(color = surfaceColor)

            data.graph.edges.forEach { edge ->
                val from = data.selectionCenterOf(edge.from) ?: return@forEach
                val to = data.selectionCenterOf(edge.to) ?: return@forEach
                val start = from.toScreen(camera)
                val end = to.toScreen(camera)
                val isSelected = selectedOwner?.key == "edge:${edge.id}"
                drawLine(
                    color = if (isSelected) selectedColor else edgeColor,
                    start = start,
                    end = end,
                    strokeWidth = if (isSelected) 4.dp.toPx() else 1.5.dp.toPx(),
                )
                edge.label?.takeIf { camera.scale >= 0.65f }?.let { label ->
                    val text = textMeasurer.measure(label, edgeLabelStyle)
                    drawText(
                        textLayoutResult = text,
                        topLeft =
                            Offset(
                                x = (start.x + end.x) / 2f - text.size.width / 2f,
                                y = (start.y + end.y) / 2f - text.size.height / 2f,
                            ),
                    )
                }
            }

            data.embeds.forEach { embed ->
                val center = embed.position.toScreen(camera)
                val half = 18.dp.toPx()
                val isSelected = selectedOwner?.key == "embed:${embed.instanceId}"
                drawRoundRect(
                    color = if (isSelected) selectedColor.copy(alpha = 0.22f) else embedFill,
                    topLeft = Offset(center.x - half, center.y - half),
                    size = Size(half * 2f, half * 2f),
                    cornerRadius = CornerRadius(8.dp.toPx()),
                )
                drawRoundRect(
                    color = if (isSelected) selectedColor else embedBorder,
                    topLeft = Offset(center.x - half, center.y - half),
                    size = Size(half * 2f, half * 2f),
                    cornerRadius = CornerRadius(8.dp.toPx()),
                    style = Stroke(width = if (isSelected) 4.dp.toPx() else 2.dp.toPx()),
                )
                if (isSelected) {
                    drawCircle(
                        color = selectedColor.copy(alpha = 0.35f),
                        radius = half + 10.dp.toPx(),
                        center = center,
                        style = Stroke(width = 3.dp.toPx()),
                    )
                }
                val label = embed.label ?: embed.instanceId
                val text = textMeasurer.measure(label, nodeLabelStyle)
                drawText(
                    textLayoutResult = text,
                    topLeft = Offset(center.x - text.size.width / 2f, center.y + half + 4.dp.toPx()),
                )
            }

            data.graph.nodes.forEach { node ->
                val center = node.position.toScreen(camera)
                val isSelected = selectedOwner?.key == "node:${node.id}"
                drawCircle(
                    color = nodeFill,
                    radius = nodeRadius,
                    center = center,
                )
                drawCircle(
                    color = if (isSelected) selectedColor else nodeBorder,
                    radius = nodeRadius,
                    center = center,
                    style = Stroke(width = if (isSelected) 4.dp.toPx() else 2.dp.toPx()),
                )
                if (isSelected) {
                    drawCircle(
                        color = selectedColor.copy(alpha = 0.35f),
                        radius = nodeRadius + 8.dp.toPx(),
                        center = center,
                        style = Stroke(width = 3.dp.toPx()),
                    )
                }
                val text = textMeasurer.measure(node.title, nodeLabelStyle)
                drawText(
                    textLayoutResult = text,
                    topLeft = Offset(center.x - text.size.width / 2f, center.y + nodeRadius + 4.dp.toPx()),
                )
            }
        }

        val focusDescription =
            remember(data, target, selectedOwner) {
                if (resolvedTarget == null || selectedOwner?.key != resolvedTarget.owner.key) {
                    null
                } else {
                    when (val requested = resolvedTarget.requested) {
                        is StarMapSelectionTarget.Link ->
                            data.linkLabel(requested.id) to resolvedTarget.owner.ownerTitle()
                        is StarMapSelectionTarget.Hyperlink ->
                            data.hyperlinkLabel(requested.id) to resolvedTarget.owner.ownerTitle()
                        else -> null
                    }
                }
            }
        if (focusDescription != null) {
            val (label, ownerTitle) = focusDescription
            Surface(
                modifier =
                    Modifier
                        .align(Alignment.BottomCenter)
                        .padding(16.dp),
                color = MaterialTheme.colorScheme.surfaceContainerHigh,
                shape = MaterialTheme.shapes.medium,
            ) {
                Text(
                    text =
                        stringResource(
                            id = R.string.starmap_link_focus_owner,
                            label,
                            ownerTitle,
                        ),
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                )
            }
        }
    }
}

private fun StarMapPointData.toScreen(camera: StarMapCamera): Offset =
    Offset(
        x = x * camera.scale + camera.offset.x,
        y = y * camera.scale + camera.offset.y,
    )

private fun StarMapCamera.centerOn(
    point: StarMapPointData,
    viewport: IntSize,
): StarMapCamera =
    copy(
        offset =
            Offset(
                x = viewport.width / 2f - point.x * scale,
                y = viewport.height / 2f - point.y * scale,
            ),
    )

private fun fitCamera(
    data: StarMapData,
    viewport: IntSize,
    padding: Float,
): StarMapCamera {
    val points =
        buildList {
            addAll(data.graph.nodes.map { it.position })
            addAll(data.embeds.map { it.position })
        }
    if (points.isEmpty() || viewport.width <= 0 || viewport.height <= 0) {
        return StarMapCamera(
            scale = 1f,
            offset = Offset(viewport.width / 2f, viewport.height / 2f),
        )
    }
    val minX = points.minOf { it.x }
    val maxX = points.maxOf { it.x }
    val minY = points.minOf { it.y }
    val maxY = points.maxOf { it.y }
    val width = max(maxX - minX, 1f)
    val height = max(maxY - minY, 1f)
    val availableWidth = max(viewport.width - padding * 2f, 1f)
    val availableHeight = max(viewport.height - padding * 2f, 1f)
    val scale = min(availableWidth / width, availableHeight / height).coerceIn(0.08f, 1.8f)
    return StarMapCamera(
        scale = scale,
        offset = Offset(viewport.width / 2f, viewport.height / 2f),
    ).centerOn(
        point = StarMapPointData(x = (minX + maxX) / 2f, y = (minY + maxY) / 2f),
        viewport = viewport,
    )
}

private fun StarMapData.selectionCenterOf(path: StarMapTargetPathData): StarMapPointData? = positionForPath(path)

private fun hitTest(
    data: StarMapData,
    world: Offset,
    nodeTolerance: Float,
): StarMapSelectionOwner? {
    data.graph.nodes
        .map { node -> node to distance(world, node.position) }
        .filter { (_, distance) -> distance <= nodeTolerance }
        .minByOrNull { (_, distance) -> distance }
        ?.let { (node, _) -> return StarMapSelectionOwner.Node(node) }

    val embedTolerance = nodeTolerance
    data.embeds
        .map { embed -> embed to distance(world, embed.position) }
        .filter { (_, distance) -> distance <= embedTolerance }
        .minByOrNull { (_, distance) -> distance }
        ?.let { (embed, _) -> return StarMapSelectionOwner.Embed(embed) }

    return data.graph.edges
        .map { edge -> edge to edge.distanceTo(data, world) }
        .filter { (_, distance) -> distance <= nodeTolerance }
        .minByOrNull { (_, distance) -> distance }
        ?.let { (edge, _) -> StarMapSelectionOwner.Edge(edge) }
}

private fun distance(
    point: Offset,
    target: StarMapPointData,
): Float {
    val dx = point.x - target.x
    val dy = point.y - target.y
    return sqrt(dx * dx + dy * dy)
}

private fun StarMapGraphEdge.distanceTo(
    data: StarMapData,
    point: Offset,
): Float {
    val from = data.selectionCenterOf(from) ?: return Float.MAX_VALUE
    val to = data.selectionCenterOf(to) ?: return Float.MAX_VALUE
    return distanceToSegment(
        px = point.x,
        py = point.y,
        x1 = from.x,
        y1 = from.y,
        x2 = to.x,
        y2 = to.y,
    )
}

private fun distanceToSegment(
    px: Float,
    py: Float,
    x1: Float,
    y1: Float,
    x2: Float,
    y2: Float,
): Float {
    val dx = x2 - x1
    val dy = y2 - y1
    val lengthSquared = dx * dx + dy * dy
    if (lengthSquared <= 0f) {
        val ddx = px - x1
        val ddy = py - y1
        return sqrt(ddx * ddx + ddy * ddy)
    }
    val t = (((px - x1) * dx + (py - y1) * dy) / lengthSquared).coerceIn(0f, 1f)
    val nearestX = x1 + t * dx
    val nearestY = y1 + t * dy
    val ddx = px - nearestX
    val ddy = py - nearestY
    return sqrt(ddx * ddx + ddy * ddy)
}

private fun StarMapData.linkLabel(linkId: String): String = links.firstOrNull { it.linkId == linkId }?.label ?: linkId

private fun StarMapData.hyperlinkLabel(hyperlinkId: String): String =
    hyperlinks.firstOrNull { it.hyperlinkId == hyperlinkId }?.label ?: hyperlinkId

private fun StarMapSelectionOwner.ownerTitle(): String =
    when (this) {
        is StarMapSelectionOwner.Node -> node.title
        is StarMapSelectionOwner.Edge -> edge.label ?: edge.id
        is StarMapSelectionOwner.Embed -> embed.label ?: embed.instanceId
    }
