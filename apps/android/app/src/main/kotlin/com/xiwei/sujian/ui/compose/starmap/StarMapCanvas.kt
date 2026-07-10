package com.xiwei.sujian.ui.compose.starmap

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gesture.detectDragGestures
import androidx.compose.foundation.gesture.detectTapGestures
import androidx.compose.foundation.gesture.detectTransformGestures
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.input.pointer.pointerInput
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapLayoutNodeData
import com.xiwei.sujian.model.StarMapEdgeRenderData
import com.xiwei.sujian.model.StarMapViewportData
import com.xiwei.sujian.model.StarMapNodeKind

private data class NodeDragState(
    val nodeId: String,
    val startLayoutX: Float,
    val startLayoutY: Float,
    val startOffsetX: Float,
    val startOffsetY: Float,
    var currentLayoutX: Float,
    var currentLayoutY: Float
)

@Composable
fun StarMapCanvas(
    data: StarMapData,
    onNodeDrag: ((nodeId: String, x: Float, y: Float) -> Unit)? = null,
    onViewportChange: ((viewport: StarMapViewportData) -> Unit)? = null,
    onNodeTap: ((nodeId: String) -> Unit)? = null,
    modifier: Modifier = Modifier
) {
    var scale by remember { mutableFloatStateOf(data.viewport.scale.coerceIn(0.2f, 5f)) }
    var offsetX by remember { mutableFloatStateOf(data.viewport.offsetX) }
    var offsetY by remember { mutableFloatStateOf(data.viewport.offsetY) }
    var dragState by remember { mutableStateOf<NodeDragState?>(null) }

    val nodeLayoutMap = remember(data.layout.nodes) {
        data.layout.nodes.associateBy { it.nodeId }
    }

    val nodeGraphMap = remember(data.graph.nodes) {
        data.graph.nodes.associateBy { it.id }
    }

    val effectiveLayoutNodes = remember(data.layout.nodes, dragState) {
        if (dragState == null) data.layout.nodes
        else data.layout.nodes.map {
            if (it.nodeId == dragState!!.nodeId) it.copy(x = dragState!!.currentLayoutX, y = dragState!!.currentLayoutY)
            else it
        }
    }

    Canvas(
        modifier = modifier
            .fillMaxSize()
            .pointerInput(data.graph.starmapId) {
                detectTransformGestures { _, pan, zoom, _ ->
                    scale = (scale * zoom).coerceIn(0.2f, 5f)
                    offsetX += pan.x / scale
                    offsetY += pan.y / scale
                }
            }
            .pointerInput(data.graph.starmapId) {
                detectDragGestures(
                    onDragStart = { offset ->
                        val hitNodeId = hitTestNode(
                            nodeLayoutMap = nodeLayoutMap,
                            offset = offset,
                            scale = scale,
                            offsetX = offsetX,
                            offsetY = offsetY
                        )
                        if (hitNodeId != null) {
                            val layoutNode = nodeLayoutMap[hitNodeId]
                            if (layoutNode != null) {
                                dragState = NodeDragState(
                                    nodeId = hitNodeId,
                                    startLayoutX = layoutNode.x,
                                    startLayoutY = layoutNode.y,
                                    startOffsetX = offset.x,
                                    startOffsetY = offset.y,
                                    currentLayoutX = layoutNode.x,
                                    currentLayoutY = layoutNode.y
                                )
                            }
                        }
                    },
                    onDrag = { change, dragAmount ->
                        if (dragState != null) {
                            change.consume()
                            val dx = dragAmount.x / scale
                            val dy = dragAmount.y / scale
                            dragState = dragState!!.copy(
                                currentLayoutX = dragState!!.currentLayoutX + dx,
                                currentLayoutY = dragState!!.currentLayoutY + dy
                            )
                        }
                    },
                    onDragEnd = {
                        dragState?.let { state ->
                            onNodeDrag?.invoke(state.nodeId, state.currentLayoutX, state.currentLayoutY)
                            dragState = null
                        }
                    },
                    onDragCancel = {
                        dragState = null
                    }
                )
            }
            .pointerInput(data.graph.starmapId) {
                detectTapGestures { offset ->
                    val hitNodeId = hitTestNode(
                        nodeLayoutMap = nodeLayoutMap,
                        offset = offset,
                        scale = scale,
                        offsetX = offsetX,
                        offsetY = offsetY
                    )
                    if (hitNodeId != null) {
                        onNodeTap?.invoke(hitNodeId)
                    }
                }
            }
    ) {
        val canvasWidth = size.width
        val canvasHeight = size.height

        for (edge in data.edgeRenders) {
            drawEdgeRender(
                edge = edge,
                scale = scale,
                offsetX = offsetX,
                offsetY = offsetY,
                canvasWidth = canvasWidth,
                canvasHeight = canvasHeight
            )
        }

        for (layoutNode in effectiveLayoutNodes) {
            val graphNode = nodeGraphMap[layoutNode.nodeId]
            drawNode(
                layoutNode = layoutNode,
                graphNode = graphNode,
                scale = scale,
                offsetX = offsetX,
                offsetY = offsetY,
                canvasWidth = canvasWidth,
                canvasHeight = canvasHeight
            )
        }
    }
}

private fun hitTestNode(
    nodeLayoutMap: Map<String, StarMapLayoutNodeData>,
    offset: Offset,
    scale: Float,
    offsetX: Float,
    offsetY: Float
): String? {
    for ((nodeId, layoutNode) in nodeLayoutMap) {
        val screenX = (layoutNode.x + offsetX) * scale
        val screenY = (layoutNode.y + offsetY) * scale
        val halfW = layoutNode.width * scale / 2f
        val halfH = layoutNode.height * scale / 2f
        if (offset.x in (screenX - halfW)..(screenX + halfW) &&
            offset.y in (screenY - halfH)..(screenY + halfH)
        ) {
            return nodeId
        }
    }
    return null
}

private fun DrawScope.drawEdgeRender(
    edge: StarMapEdgeRenderData,
    scale: Float,
    offsetX: Float,
    offsetY: Float,
    canvasWidth: Float,
    canvasHeight: Float
) {
    val startX = (edge.startX + offsetX) * scale
    val startY = (edge.startY + offsetY) * scale
    val endX = (edge.endX + offsetX) * scale
    val endY = (edge.endY + offsetY) * scale

    if (startX < -100 && endX < -100) return
    if (startX > canvasWidth + 100 && endX > canvasWidth + 100) return
    if (startY < -100 && endY < -100) return
    if (startY > canvasHeight + 100 && endY > canvasHeight + 100) return

    drawLine(
        color = Color(0xFF888888),
        start = Offset(startX, startY),
        end = Offset(endX, endY),
        strokeWidth = 1.5f * scale,
        pathEffect = PathEffect.dashPathEffect(floatArrayOf(10f, 5f), 0f)
    )

    if (edge.arrowTipX != 0f || edge.arrowTipY != 0f) {
        val tipX = (edge.arrowTipX + offsetX) * scale
        val tipY = (edge.arrowTipY + offsetY) * scale
        val leftX = (edge.arrowLeftX + offsetX) * scale
        val leftY = (edge.arrowLeftY + offsetY) * scale
        val rightX = (edge.arrowRightX + offsetX) * scale
        val rightY = (edge.arrowRightY + offsetY) * scale
        drawPath(
            path = androidx.compose.ui.graphics.Path().apply {
                moveTo(tipX, tipY)
                lineTo(leftX, leftY)
                lineTo(rightX, rightY)
                close()
            },
            color = Color(0xFF888888)
        )
    }
}

private fun DrawScope.drawNode(
    layoutNode: StarMapLayoutNodeData,
    graphNode: StarMapGraphNode?,
    scale: Float,
    offsetX: Float,
    offsetY: Float,
    canvasWidth: Float,
    canvasHeight: Float
) {
    val cx = (layoutNode.x + offsetX) * scale
    val cy = (layoutNode.y + offsetY) * scale
    val w = layoutNode.width * scale
    val h = layoutNode.height * scale

    if (cx + w / 2 < 0 || cx - w / 2 > canvasWidth) return
    if (cy + h / 2 < 0 || cy - h / 2 > canvasHeight) return

    val nodeColor = when (graphNode?.kind) {
        StarMapNodeKind.Character -> Color(0xFF6750A4)
        StarMapNodeKind.Event -> Color(0xFFB3261E)
        StarMapNodeKind.Location -> Color(0xFF2E7D32)
        StarMapNodeKind.Item -> Color(0xFFFF8F00)
        StarMapNodeKind.Concept -> Color(0xFF0288D1)
        StarMapNodeKind.Theme -> Color(0xFF7B1FA2)
        StarMapNodeKind.Note -> Color(0xFF546E7A)
        else -> Color(0xFF625B71)
    }

    val radius = layoutNode.radius * scale
    drawRoundRect(
        color = nodeColor.copy(alpha = 0.15f),
        topLeft = Offset(cx - w / 2, cy - h / 2),
        size = Size(w, h),
        cornerRadius = androidx.compose.ui.geometry.CornerRadius(radius)
    )
    drawRoundRect(
        color = nodeColor.copy(alpha = 0.6f),
        topLeft = Offset(cx - w / 2, cy - h / 2),
        size = Size(w, h),
        cornerRadius = androidx.compose.ui.geometry.CornerRadius(radius),
        style = Stroke(width = 1.5f * scale)
    )

    if (graphNode != null) {
        drawContext.canvas.nativeCanvas.drawText(
            graphNode.title,
            cx,
            cy + 4f * scale,
            android.graphics.Paint().apply {
                color = nodeColor.toArgb()
                textSize = 12f * scale
                textAlign = android.graphics.Paint.Align.CENTER
                isAntiAlias = true
            }
        )
    }
}

private fun Color.toArgb(): Int = ((alpha * 255).toInt() shl 24) or
        ((red * 255).toInt() shl 16) or
        ((green * 255).toInt() shl 8) or
        (blue * 255).toInt())
