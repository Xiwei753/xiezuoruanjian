package com.xiwei.sujian.ui

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.util.AttributeSet
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapNodeKind

/**
 * StarMapCanvasView — 星图画布自定义 View
 *
 * 使用 Canvas 绘制星图节点和连线，支持平移和缩放交互。
 *
 * ## 架构定位
 * - StarMapController → StarMapCanvasView → Canvas 绘制
 *
 * ## 职责边界
 * - **做**：星图可视化渲染、节点拖拽、平移/缩放交互
 * - **不做**：数据管理（由 StarMapController 负责）
 *
 * ## 使用场景
 * - MainActivity 星图标签页的可视化展示
 * - 用户拖拽节点调整布局
 */
class StarMapCanvasView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {

    private var data: StarMapData? = null

    private val paintNodeBg = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.parseColor("#1A1D23") }
    private val paintNodeBorder = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#2A2E36")
        style = Paint.Style.STROKE
        strokeWidth = 2f
    }
    private val paintHeader = Paint(Paint.ANTI_ALIAS_FLAG)

    private val paintTitleText = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#E2E4E9")
        textSize = 36f
        textAlign = Paint.Align.CENTER
    }

    private val paintKindText = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE
        textSize = 28f
        typeface = Typeface.DEFAULT_BOLD
        textAlign = Paint.Align.CENTER
    }

    private val paintEdge = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#4C566A")
        strokeWidth = 4f
        style = Paint.Style.STROKE
    }

    var panX = 0f
    var panY = 0f
    var zoom = 1f
    var lastTouchX = 0f
    var lastTouchY = 0f
    var draggingNodeId: String? = null
    var onNodeDragListener: ((String, Float, Float) -> Unit)? = null
    var onNodeHitTestListener: ((Float, Float) -> String?)? = null
    var onLayoutSavedListener: (() -> Unit)? = null

    private val scaleGestureDetector = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(detector: ScaleGestureDetector): Boolean {
            val oldZoom = zoom
            val newZoom = (zoom * detector.scaleFactor).coerceIn(0.35f, 3.0f)
            if (newZoom == oldZoom) return false

            val focusX = detector.focusX
            val focusY = detector.focusY
            val graphFocusX = (focusX - panX) / oldZoom
            val graphFocusY = (focusY - panY) / oldZoom
            zoom = newZoom
            panX = focusX - graphFocusX * zoom
            panY = focusY - graphFocusY * zoom
            invalidate()
            return true
        }
    })

    fun getData(): StarMapData? = data

    fun setData(newData: StarMapData) {
        this.data = newData
        invalidate()
    }

    private fun getKindColor(kind: StarMapNodeKind): Int {
        return when (kind) {
            StarMapNodeKind.Chapter -> Color.parseColor("#4CAF50")
            StarMapNodeKind.Character -> Color.parseColor("#2196F3")
            StarMapNodeKind.Location -> Color.parseColor("#FF9800")
            StarMapNodeKind.Event -> Color.parseColor("#F44336")
            StarMapNodeKind.Concept -> Color.parseColor("#9C27B0")
            else -> Color.parseColor("#606470")
        }
    }

    private fun getKindString(kind: StarMapNodeKind): String {
        return when (kind) {
            StarMapNodeKind.Note -> "Note"
            StarMapNodeKind.Chapter -> "Chapter"
            StarMapNodeKind.Character -> "Character"
            StarMapNodeKind.Location -> "Location"
            StarMapNodeKind.Event -> "Event"
            StarMapNodeKind.Concept -> "Concept"
            StarMapNodeKind.Theme -> "Theme"
            StarMapNodeKind.Item -> "Item"
            StarMapNodeKind.Organization -> "Organization"
            StarMapNodeKind.Timeline -> "Timeline"
            StarMapNodeKind.Plot -> "Plot"
            StarMapNodeKind.Foreshadowing -> "Foreshadowing"
            StarMapNodeKind.Custom -> "Custom"
        }
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.drawColor(Color.parseColor("#111318"))

        val currentData = data ?: return

        canvas.save()
        canvas.translate(panX, panY)
        canvas.scale(zoom, zoom)

        for (edge in currentData.edgeRenders) {
            canvas.drawLine(edge.startX, edge.startY, edge.endX, edge.endY, paintEdge)
            canvas.drawLine(edge.arrowTipX, edge.arrowTipY, edge.arrowLeftX, edge.arrowLeftY, paintEdge)
            canvas.drawLine(edge.arrowTipX, edge.arrowTipY, edge.arrowRightX, edge.arrowRightY, paintEdge)
        }

        for (node in currentData.graph.nodes) {
            val layout = currentData.layout.nodes.find { it.nodeId == node.id }
            if (layout != null) {
                val rect = RectF(layout.x, layout.y, layout.x + layout.width, layout.y + layout.height)

                // Draw node background
                canvas.drawRoundRect(rect, 16f, 16f, paintNodeBg)

                // Draw border
                canvas.drawRoundRect(rect, 16f, 16f, paintNodeBorder)

                // Draw header strip
                paintHeader.color = getKindColor(node.kind)
                val headerRect = RectF(layout.x + 16f, layout.y + 16f, layout.x + layout.width - 16f, layout.y + 16f + 48f)
                canvas.drawRoundRect(headerRect, 8f, 8f, paintHeader)

                // Draw kind text
                canvas.drawText(
                    getKindString(node.kind),
                    layout.x + layout.width / 2,
                    layout.y + 16f + 34f,
                    paintKindText
                )

                // Draw title text
                canvas.drawText(
                    node.title,
                    layout.x + layout.width / 2,
                    layout.y + layout.height / 2 + 32f,
                    paintTitleText
                )
            }
        }

        canvas.restore()
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val x = event.x
        val y = event.y
        scaleGestureDetector.onTouchEvent(event)

        if (event.pointerCount > 1 || scaleGestureDetector.isInProgress) {
            draggingNodeId = null
            lastTouchX = x
            lastTouchY = y
            return true
        }

        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                lastTouchX = x
                lastTouchY = y

                val graphX = (x - panX) / zoom
                val graphY = (y - panY) / zoom

                draggingNodeId = onNodeHitTestListener?.invoke(graphX, graphY)
            }
            MotionEvent.ACTION_MOVE -> {
                val dx = x - lastTouchX
                val dy = y - lastTouchY

                if (draggingNodeId != null) {
                    onNodeDragListener?.invoke(draggingNodeId!!, dx / zoom, dy / zoom)
                } else {
                    panX += dx
                    panY += dy
                }

                lastTouchX = x
                lastTouchY = y
                invalidate()
            }
            MotionEvent.ACTION_UP -> {
                if (draggingNodeId != null) {
                    onLayoutSavedListener?.invoke()
                }
                draggingNodeId = null
            }
        }
        return true
    }
}
