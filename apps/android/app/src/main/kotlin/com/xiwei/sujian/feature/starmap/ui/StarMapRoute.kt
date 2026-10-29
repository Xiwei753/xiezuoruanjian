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
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.center
import androidx.compose.ui.geometry.height
import androidx.compose.ui.geometry.size
import androidx.compose.ui.geometry.topLeft
import androidx.compose.ui.geometry.width
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.core.designsystem.component.SujianListItem
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.editor.presentation.AnnotationCoordinator
import com.xiwei.sujian.feature.editor.presentation.AnnotationObjectKind
import com.xiwei.sujian.feature.editor.presentation.AnnotationObjectRef
import com.xiwei.sujian.feature.editor.ui.AnnotationSheet
import com.xiwei.sujian.feature.starmap.data.StarMapRepository
import com.xiwei.sujian.feature.starmap.data.model.StarMapData
import com.xiwei.sujian.feature.starmap.data.model.StarMapGraphEdge
import com.xiwei.sujian.feature.starmap.data.model.StarMapGraphNode
import com.xiwei.sujian.feature.starmap.data.model.StarMapMeta
import com.xiwei.sujian.feature.starmap.data.model.StarMapNodeContentData
import com.xiwei.sujian.feature.starmap.data.model.StarMapPointData
import com.xiwei.sujian.feature.starmap.data.model.StarMapSelectionOwner
import com.xiwei.sujian.feature.starmap.data.model.StarMapSelectionTarget
import com.xiwei.sujian.feature.starmap.data.model.StarMapTargetPathData
import com.xiwei.sujian.feature.starmap.data.model.ownerCenter
import com.xiwei.sujian.feature.starmap.data.model.positionForPath
import com.xiwei.sujian.feature.starmap.data.model.resolveSelectionTarget
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
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
    projectId: String?,
    target: StarMapSelectionTarget?,
    onOpenStarmap: (starmapId: String, projectId: String?) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (starmapId == null) {
        StarMapRootScreen(onOpenStarmap = onOpenStarmap, modifier = modifier)
    } else {
        StarMapDetailScreen(starmapId = starmapId, projectId = projectId, target = target, modifier = modifier)
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
    projectId: String?,
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
                starmapId = starmapId,
                projectId = projectId,
                repository = repository,
                data = current.data,
                target = target,
                onNodeUpdated = { updated ->
                    // Issue #984：保存后立即用 Core 返回的节点刷新当前画布。
                    state = StarMapDetailState.Ready(current.data.withUpdatedNode(updated))
                },
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

// Issue #984：普通节点是文字卡片。卡片尺寸按内容测量，position 始终是左上角。
private val StarMapCardWidth = 200.dp
private val StarMapCardMinHeight = 56.dp
private val StarMapCardPadding = 12.dp
private val StarMapCardTitleBodyGap = 6.dp
private val StarMapCardCorner = 10.dp
private const val StarMapCardTitleMaxLines = 2
private const val StarMapCardBodyMaxLines = 5
private const val StarMapCardBodyPreviewMinScale = 0.55f

private data class StarMapNodeCard(
    val node: StarMapGraphNode,
    val rect: Rect,
    val title: TextLayoutResult,
    val titleTopLeft: Offset,
    val body: TextLayoutResult?,
    val bodyTopLeft: Offset,
)

private fun buildNodeCards(
    nodes: List<StarMapGraphNode>,
    textMeasurer: TextMeasurer,
    density: Density,
    titleStyle: TextStyle,
    bodyStyle: TextStyle,
): List<StarMapNodeCard> {
    val width = with(density) { StarMapCardWidth.toPx() }
    val padding = with(density) { StarMapCardPadding.toPx() }
    val gap = with(density) { StarMapCardTitleBodyGap.toPx() }
    val minHeight = with(density) { StarMapCardMinHeight.toPx() }
    val maxTextWidth = (width - padding * 2f).roundToInt().coerceAtLeast(1)
    val constraints = Constraints(maxWidth = maxTextWidth)

    return nodes.map { node ->
        val title =
            textMeasurer.measure(
                text = node.title,
                style = titleStyle,
                overflow = TextOverflow.Ellipsis,
                maxLines = StarMapCardTitleMaxLines,
                constraints = constraints,
            )
        // 正文只来自 empty / inline 内容；按原文本测量，保留 \n 与连续空行。
        val body =
            node.contentBody
                ?.takeIf {
                    it.isNotEmpty() &&
                        (node.contentKind == null ||
                            node.contentKind == StarMapNodeContentData.TYPE_EMPTY ||
                            node.contentKind == StarMapNodeContentData.TYPE_INLINE)
                }
                ?.let { bodyText ->
                    textMeasurer.measure(
                        text = bodyText,
                        style = bodyStyle,
                        overflow = TextOverflow.Ellipsis,
                        maxLines = StarMapCardBodyMaxLines,
                        constraints = constraints,
                    )
                }
        val contentHeight = title.size.height + (body?.let { gap + it.size.height } ?: 0f)
        val height = max(minHeight, contentHeight + padding * 2f)
        val contentTop = node.position.y + padding
        StarMapNodeCard(
            node = node,
            rect =
                Rect(
                    left = node.position.x,
                    top = node.position.y,
                    right = node.position.x + width,
                    bottom = node.position.y + height,
                ),
            title = title,
            titleTopLeft = Offset(node.position.x + padding, contentTop),
            body = body,
            bodyTopLeft = Offset(node.position.x + padding, contentTop + title.size.height + gap),
        )
    }
}

@Suppress("LongMethod", "CyclomaticComplexMethod", "CognitiveComplexMethod")
@Composable
private fun StarMapCanvas(
    starmapId: String,
    projectId: String?,
    repository: StarMapRepository,
    data: StarMapData,
    target: StarMapSelectionTarget?,
    onNodeUpdated: (StarMapGraphNode) -> Unit,
    modifier: Modifier = Modifier,
) {
    val appDependencies = LocalSujianAppDependencies.current
    val annotationScope = rememberCoroutineScope()
    val annotationCoordinator =
        remember(appDependencies.appServiceBridge, appDependencies.searchRepository, annotationScope) {
            AnnotationCoordinator(
                bridge = appDependencies.appServiceBridge.annotationBridge,
                searchRepository = appDependencies.searchRepository,
                scope = annotationScope,
            )
        }
    val annotationState by annotationCoordinator.state.collectAsStateWithLifecycle()
    val resolvedTarget = remember(data, target) { target?.let { data.resolveSelectionTarget(it) } }
    var selectedOwner by remember(data, target) { mutableStateOf(resolvedTarget?.owner) }
    var viewport by remember { mutableStateOf(IntSize.Zero) }
    var camera by remember { mutableStateOf(StarMapCamera(1f, Offset.Zero)) }
    var editingNode by remember(data) { mutableStateOf<StarMapGraphNode?>(null) }
    var annotationNode by remember(data) { mutableStateOf<StarMapGraphNode?>(null) }
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
    val nodeTitleStyle =
        TextStyle(
            color = MaterialTheme.colorScheme.onSurface,
            fontSize = 13.sp,
            lineHeight = 17.sp,
            fontWeight = FontWeight.SemiBold,
        )
    val nodeBodyStyle =
        TextStyle(
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            fontSize = 11.sp,
            lineHeight = 15.sp,
        )
    val edgeLabelStyle =
        TextStyle(
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            fontSize = 10.sp,
        )
    val nodeCards =
        remember(data.graph.nodes, textMeasurer, density, nodeTitleStyle, nodeBodyStyle) {
            buildNodeCards(
                nodes = data.graph.nodes,
                textMeasurer = textMeasurer,
                density = density,
                titleStyle = nodeTitleStyle,
                bodyStyle = nodeBodyStyle,
            )
        }
    val cardsById = remember(nodeCards) { nodeCards.associateBy { it.node.id } }
    val nodeFill = MaterialTheme.colorScheme.secondaryContainer
    val nodeBorder = MaterialTheme.colorScheme.secondary
    val edgeColor = MaterialTheme.colorScheme.outline
    val selectedColor = MaterialTheme.colorScheme.primary
    val embedFill = MaterialTheme.colorScheme.tertiaryContainer
    val embedBorder = MaterialTheme.colorScheme.tertiary
    val surfaceColor = MaterialTheme.colorScheme.surface

    LaunchedEffect(data.graph.starmapId, viewport, target) {
        if (viewport.width <= 0 || viewport.height <= 0) return@LaunchedEffect
        val fit = fitCamera(data, viewport, padding = 48f, cards = nodeCards)
        val targetCenter =
            resolvedTarget?.owner?.let { owner ->
                when (owner) {
                    is StarMapSelectionOwner.Node ->
                        cardsById[owner.id]?.rect?.center?.let { StarMapPointData(it.x, it.y) }
                    is StarMapSelectionOwner.Embed -> data.ownerCenter(owner)
                    is StarMapSelectionOwner.Edge -> data.ownerCenter(owner)
                }
            }
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
                    .pointerInput(data, nodeCards, nodeRadius) {
                        detectTapGestures(
                            onTap = { tap ->
                                val activeCamera = currentCamera
                                selectedOwner =
                                    hitTest(
                                        data = data,
                                        world = tap.toWorld(activeCamera),
                                        cardsById = cardsById,
                                        proximityTolerance = nodeRadius / activeCamera.scale,
                                    )
                            },
                            onDoubleTap = { tap ->
                                val activeCamera = currentCamera
                                val hit =
                                    hitTest(
                                        data = data,
                                        world = tap.toWorld(activeCamera),
                                        cardsById = cardsById,
                                        proximityTolerance = nodeRadius / activeCamera.scale,
                                    )
                                selectedOwner = hit
                                if (hit is StarMapSelectionOwner.Node) {
                                    editingNode = hit.node
                                }
                            },
                            onLongPress = { tap ->
                                val activeCamera = currentCamera
                                val hit =
                                    hitTest(
                                        data = data,
                                        world = tap.toWorld(activeCamera),
                                        cardsById = cardsById,
                                        proximityTolerance = nodeRadius / activeCamera.scale,
                                    )
                                val node = (hit as? StarMapSelectionOwner.Node)?.node
                                selectedOwner = hit
                                annotationNode = node
                                if (node == null) annotationCoordinator.dismissSheet()
                            },
                        )
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
                val (from, to) = data.edgeEndpoints(edge, cardsById) ?: return@forEach
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

            // 卡片和文字一起随相机缩放，保证卡片矩形就是视觉边界。
            withTransform({
                translate(camera.offset.x, camera.offset.y)
                scale(camera.scale, camera.scale, pivot = Offset.Zero)
            }) {
                val cornerRadius = CornerRadius(StarMapCardCorner.toPx())
                data.graph.nodes.forEach { node ->
                    val card = cardsById[node.id] ?: return@forEach
                    val isSelected = selectedOwner?.key == "node:${node.id}"
                    drawRoundRect(
                        color = nodeFill,
                        topLeft = card.rect.topLeft,
                        size = card.rect.size,
                        cornerRadius = cornerRadius,
                    )
                    if (isSelected) {
                        val halo = 4.dp.toPx()
                        drawRoundRect(
                            color = selectedColor.copy(alpha = 0.35f),
                            topLeft = Offset(card.rect.left - halo, card.rect.top - halo),
                            size = Size(card.rect.width + halo * 2f, card.rect.height + halo * 2f),
                            cornerRadius = CornerRadius(StarMapCardCorner.toPx() + halo),
                            style = Stroke(width = 3.dp.toPx()),
                        )
                    }
                    drawRoundRect(
                        color = if (isSelected) selectedColor else nodeBorder,
                        topLeft = card.rect.topLeft,
                        size = card.rect.size,
                        cornerRadius = cornerRadius,
                        style = Stroke(width = if (isSelected) 4.dp.toPx() else 2.dp.toPx()),
                    )
                    drawText(
                        textLayoutResult = card.title,
                        topLeft = card.titleTopLeft,
                    )
                    if (camera.scale >= StarMapCardBodyPreviewMinScale) {
                        card.body?.let { body ->
                            drawText(
                                textLayoutResult = body,
                                topLeft = card.bodyTopLeft,
                            )
                        }
                    }
                }
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

        editingNode?.let { node ->
            StarMapNodeEditorDialog(
                starmapId = starmapId,
                repository = repository,
                node = node,
                onDismiss = { editingNode = null },
                onSaved = { updated ->
                    editingNode = null
                    onNodeUpdated(updated)
                },
            )
        }
        val selectedAnnotationNode = annotationNode
        val annotationProjectId = projectId
        if (selectedAnnotationNode != null && annotationProjectId != null) {
            LaunchedEffect(selectedAnnotationNode.id, annotationProjectId) {
                annotationCoordinator.openObject(
                    ref =
                        AnnotationObjectRef(
                            AnnotationObjectKind.StarMapNode,
                            annotationProjectId,
                            selectedAnnotationNode.id,
                        ),
                    label = selectedAnnotationNode.title,
                )
            }
        }
        annotationNode?.takeIf { projectId == null }?.let { node ->
            Surface(
                modifier = Modifier.align(Alignment.TopEnd).padding(8.dp),
                color = MaterialTheme.colorScheme.surfaceContainerHigh,
                shape = MaterialTheme.shapes.small,
            ) {
                Text(node.title, modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp))
            }
        }
        if (annotationState.isSheetOpen) {
            AnnotationSheet(
                state = annotationState,
                onDismiss = { annotationCoordinator.dismissSheet(); annotationNode = null },
                onTagToggled = annotationCoordinator::toggleTag,
                onCreateTag = annotationCoordinator::createTag,
                onRegisterName = annotationCoordinator::registerName,
                onNameChanged = annotationCoordinator::onNameDraftChanged,
                onSearchChanged = annotationCoordinator::onLinkSearchChanged,
                onObjectLink = annotationCoordinator::addObjectLink,
                onDeleteLink = annotationCoordinator::deleteLink,
            )
        }
    }
}

@Suppress("LongMethod")
@Composable
private fun StarMapNodeEditorDialog(
    starmapId: String,
    repository: StarMapRepository,
    node: StarMapGraphNode,
    onDismiss: () -> Unit,
    onSaved: (StarMapGraphNode) -> Unit,
) {
    val scope = rememberCoroutineScope()
    var title by remember(node.id, node.title) { mutableStateOf(node.title) }
    var body by remember(node.id, node.contentBody) { mutableStateOf(node.contentBody.orEmpty()) }
    var saving by remember(node.id) { mutableStateOf(false) }
    var errorDetail by remember(node.id) { mutableStateOf<String?>(null) }
    val trimmedTitle = title.trim()
    // 引用类型只允许改名称；保存时不传 content，Core 会保留原引用。
    val editableBody =
        node.contentKind == null ||
            node.contentKind == StarMapNodeContentData.TYPE_EMPTY ||
            node.contentKind == StarMapNodeContentData.TYPE_INLINE
    val originalBody = node.contentBody.orEmpty()

    fun save() {
        if (saving || trimmedTitle.isEmpty()) return
        val titleChanged = trimmedTitle != node.title
        // 名称与正文分别更新：只有正文确实变化才发送 content。
        // 原样比较（含换行），不做 trim；empty 节点新增正文时才转 inline。
        val bodyChanged = editableBody && body != originalBody
        if (!titleChanged && !bodyChanged) {
            onDismiss()
            return
        }
        saving = true
        errorDetail = null
        val content =
            if (bodyChanged) {
                StarMapNodeContentData(
                    type = StarMapNodeContentData.TYPE_INLINE,
                    summary = node.contentSummary,
                    // Issue #984：正文不 trim，段落空白与连续空行原样交给 Core。
                    body = body,
                )
            } else {
                null
            }
        scope.launch {
            val result =
                withContext(Dispatchers.IO) {
                    repository.updateStarmapNode(
                        starmapId = starmapId,
                        nodeId = node.id,
                        title = if (titleChanged) trimmedTitle else null,
                        content = content,
                    )
                }
            when (result) {
                is BridgeResult.Success -> {
                    saving = false
                    onSaved(result.data)
                }
                is BridgeResult.Error -> {
                    saving = false
                    errorDetail = result.envelope.rawError ?: result.message
                }
                is BridgeResult.NotLoaded -> {
                    saving = false
                    errorDetail = result.envelope.rawError ?: result.envelope.errorCode
                }
            }
        }
    }

    AlertDialog(
        onDismissRequest = { if (!saving) onDismiss() },
        title = { Text(stringResource(R.string.starmap_node_edit_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(
                    value = title,
                    onValueChange = { title = it },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text(stringResource(R.string.starmap_node_name_label)) },
                    singleLine = true,
                    isError = trimmedTitle.isEmpty(),
                    supportingText =
                        if (trimmedTitle.isEmpty()) {
                            { Text(stringResource(R.string.starmap_node_title_required)) }
                        } else {
                            null
                        },
                )
                if (editableBody) {
                    OutlinedTextField(
                        value = body,
                        onValueChange = { body = it },
                        modifier =
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = 120.dp),
                        label = { Text(stringResource(R.string.starmap_node_body_label)) },
                        singleLine = false,
                        minLines = 4,
                        maxLines = 8,
                    )
                }
                errorDetail?.let { detail ->
                    Text(
                        text = stringResource(R.string.error_save_failed, detail),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = ::save,
                enabled = trimmedTitle.isNotEmpty() && !saving,
            ) {
                Text(
                    if (saving) {
                        stringResource(R.string.status_saving)
                    } else {
                        stringResource(R.string.action_save)
                    },
                )
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, enabled = !saving) {
                Text(stringResource(R.string.action_cancel))
            }
        },
    )
}

private fun StarMapPointData.toScreen(camera: StarMapCamera): Offset =
    Offset(
        x = x * camera.scale + camera.offset.x,
        y = y * camera.scale + camera.offset.y,
    )

private fun Offset.toScreen(camera: StarMapCamera): Offset =
    Offset(
        x = x * camera.scale + camera.offset.x,
        y = y * camera.scale + camera.offset.y,
    )

private fun Offset.toWorld(camera: StarMapCamera): Offset =
    Offset(
        x = (x - camera.offset.x) / camera.scale,
        y = (y - camera.offset.y) / camera.scale,
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

/** Issue #984：节点包围盒使用卡片矩形（position 是左上角），嵌入仍使用自身落点。 */
private fun fitCamera(
    data: StarMapData,
    viewport: IntSize,
    padding: Float,
    cards: List<StarMapNodeCard>,
): StarMapCamera {
    val bounds =
        buildList {
            addAll(cards.map { it.rect })
            addAll(
                data.embeds.map { embed ->
                    Rect(
                        left = embed.position.x,
                        top = embed.position.y,
                        right = embed.position.x,
                        bottom = embed.position.y,
                    )
                },
            )
        }
    if (bounds.isEmpty() || viewport.width <= 0 || viewport.height <= 0) {
        return StarMapCamera(
            scale = 1f,
            offset = Offset(viewport.width / 2f, viewport.height / 2f),
        )
    }
    val minX = bounds.minOf { it.left }
    val maxX = bounds.maxOf { it.right }
    val minY = bounds.minOf { it.top }
    val maxY = bounds.maxOf { it.bottom }
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

private fun StarMapData.withUpdatedNode(updated: StarMapGraphNode): StarMapData =
    copy(
        graph =
            graph.copy(
                nodes = graph.nodes.map { node -> if (node.id == updated.id) updated else node },
            ),
    )

private fun hitTest(
    data: StarMapData,
    world: Offset,
    cardsById: Map<String, StarMapNodeCard>,
    proximityTolerance: Float,
): StarMapSelectionOwner? {
    // Issue #984：普通节点按卡片矩形命中，和绘制边界保持同一份几何。
    data.graph.nodes
        .mapNotNull { node -> cardsById[node.id]?.let { card -> node to card } }
        .filter { (_, card) -> card.rect.contains(world) }
        .minByOrNull { (_, card) -> distance(world, card.rect.center) }
        ?.let { (node, _) -> return StarMapSelectionOwner.Node(node) }

    data.embeds
        .map { embed -> embed to distance(world, embed.position) }
        .filter { (_, distance) -> distance <= proximityTolerance }
        .minByOrNull { (_, distance) -> distance }
        ?.let { (embed, _) -> return StarMapSelectionOwner.Embed(embed) }

    return data.graph.edges
        .map { edge -> edge to edge.distanceTo(data, world, cardsById) }
        .filter { (_, distance) -> distance <= proximityTolerance }
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

private fun distance(
    point: Offset,
    target: Offset,
): Float {
    val dx = point.x - target.x
    val dy = point.y - target.y
    return sqrt(dx * dx + dy * dy)
}

private data class StarMapEdgeAnchor(
    val center: Offset,
    val card: StarMapNodeCard?,
)

private fun StarMapData.edgeEndpoints(
    edge: StarMapGraphEdge,
    cardsById: Map<String, StarMapNodeCard>,
): Pair<Offset, Offset>? {
    val from = edgeAnchor(edge.from, cardsById) ?: return null
    val to = edgeAnchor(edge.to, cardsById) ?: return null
    // 连线落在卡片边界而不是卡片中心。
    val start = from.card?.rect?.edgePointToward(to.center) ?: from.center
    val end = to.card?.rect?.edgePointToward(from.center) ?: to.center
    return start to end
}

private fun StarMapData.edgeAnchor(
    path: StarMapTargetPathData,
    cardsById: Map<String, StarMapNodeCard>,
): StarMapEdgeAnchor? {
    path.segments.firstOrNull()?.instanceId?.let { instanceId ->
        embeds.firstOrNull { it.instanceId == instanceId }?.let { embed ->
            return StarMapEdgeAnchor(
                center = Offset(embed.position.x, embed.position.y),
                card = null,
            )
        }
    }
    path.target.nodeId?.let { nodeId ->
        cardsById[nodeId]?.let { card ->
            return StarMapEdgeAnchor(center = card.rect.center, card = card)
        }
    }
    path.target.entityId?.let { entityId ->
        cardsById[entityId]?.let { card ->
            return StarMapEdgeAnchor(center = card.rect.center, card = card)
        }
    }
    return positionForPath(path)?.let { point ->
        StarMapEdgeAnchor(center = Offset(point.x, point.y), card = null)
    }
}

private fun Rect.edgePointToward(target: Offset): Offset {
    val center = center
    val dx = target.x - center.x
    val dy = target.y - center.y
    if (abs(dx) < 0.001f && abs(dy) < 0.001f) return center
    val tx =
        when {
            dx > 0f -> (right - center.x) / dx
            dx < 0f -> (left - center.x) / dx
            else -> Float.POSITIVE_INFINITY
        }
    val ty =
        when {
            dy > 0f -> (bottom - center.y) / dy
            dy < 0f -> (top - center.y) / dy
            else -> Float.POSITIVE_INFINITY
        }
    val t = min(tx, ty).coerceIn(0f, 1f)
    return Offset(center.x + dx * t, center.y + dy * t)
}

private fun StarMapGraphEdge.distanceTo(
    data: StarMapData,
    point: Offset,
    cardsById: Map<String, StarMapNodeCard>,
): Float {
    val (from, to) = data.edgeEndpoints(this, cardsById) ?: return Float.MAX_VALUE
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

private fun StarMapSelectionOwner.ownerTitle(): String =
    when (this) {
        is StarMapSelectionOwner.Node -> node.title
        is StarMapSelectionOwner.Edge -> edge.label ?: edge.id
        is StarMapSelectionOwner.Embed -> embed.label ?: embed.instanceId
    }
