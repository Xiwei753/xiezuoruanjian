package com.xiwei.writerapp.ui.mindmap

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.util.AttributeSet
import android.view.GestureDetector
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import com.xiwei.writerapp.model.MindMapEdge
import com.xiwei.writerapp.model.MindMapNode
import com.xiwei.writerapp.model.MindMapNodeKind
import com.xiwei.writerapp.model.MindMapSnapshot
import com.xiwei.writerapp.model.MindMapViewport
import kotlin.math.max

class MindMapRenderView @JvmOverloads constructor(
    context: Context, attrs: AttributeSet? = null, defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {

    private var snapshot: MindMapSnapshot? = null
    private val viewport = MindMapViewport()
    private val transformMatrix = Matrix()
    private val inverseMatrix = Matrix()

    private val nodePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
    }
    private val nodeBorderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 2f
        color = Color.DKGRAY
    }
    private val selectedNodeBorderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 6f
        color = Color.parseColor("#4CAF50")
    }
    private val edgePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 3f
        color = Color.GRAY
    }
    private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.BLACK
        textSize = 24f
        textAlign = Paint.Align.CENTER
    }
    private val hudPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.BLACK
        textSize = 30f
    }

    private val nodeRect = RectF()
    private val edgePath = Path()

    private var showHud = true
    private var lastDrawTime = 0L
    private var fps = 0f
    private var visibleNodesCount = 0
    private var selectedNodeId: String? = null

    // For touch handling
    private val scaleGestureDetector: ScaleGestureDetector
    private val gestureDetector: GestureDetector

    // Request high refresh rate for smooth panning (Android 11+)
    init {
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
            val display = context.display
            if (display != null) {
                var maxRefreshRate = 60f
                for (mode in display.supportedModes) {
                    maxRefreshRate = max(maxRefreshRate, mode.refreshRate)
                }
                (context as? android.app.Activity)?.window?.let { window ->
                    window.attributes = window.attributes.apply {
                        preferredRefreshRate = maxRefreshRate
                    }
                }
            }
        }
    }

    init {
        scaleGestureDetector = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
            override fun onScale(detector: ScaleGestureDetector): Boolean {
                val scaleFactor = detector.scaleFactor
                val focusX = detector.focusX
                val focusY = detector.focusY

                viewport.scale *= scaleFactor
                viewport.scale = viewport.scale.coerceIn(0.1f, 5.0f)

                // Adjust translation to zoom around focus point
                viewport.translateX = focusX - (focusX - viewport.translateX) * scaleFactor
                viewport.translateY = focusY - (focusY - viewport.translateY) * scaleFactor

                updateTransform()
                return true
            }
        })

        gestureDetector = GestureDetector(context, object : GestureDetector.SimpleOnGestureListener() {
            override fun onScroll(e1: MotionEvent?, e2: MotionEvent, distanceX: Float, distanceY: Float): Boolean {
                viewport.translateX -= distanceX
                viewport.translateY -= distanceY
                updateTransform()
                return true
            }

            override fun onSingleTapUp(e: MotionEvent): Boolean {
                val point = floatArrayOf(e.x, e.y)
                inverseMatrix.mapPoints(point)
                val tapX = point[0]
                val tapY = point[1]

                selectedNodeId = null
                snapshot?.nodes?.forEach { node ->
                    val halfW = node.width / 2f
                    val halfH = node.height / 2f
                    if (tapX in (node.x - halfW)..(node.x + halfW) &&
                        tapY in (node.y - halfH)..(node.y + halfH)
                    ) {
                        selectedNodeId = node.id
                    }
                }
                invalidate()
                return true
            }
        })
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        var handled = scaleGestureDetector.onTouchEvent(event)
        handled = gestureDetector.onTouchEvent(event) || handled
        return handled || super.onTouchEvent(event)
    }

    fun setSnapshot(newSnapshot: MindMapSnapshot?) {
        this.snapshot = newSnapshot
        fitToScreen()
        invalidate()
    }

    fun fitToScreen() {
        val snap = snapshot ?: return
        val bounds = snap.bounds
        if (bounds.minX >= bounds.maxX || bounds.minY >= bounds.maxY) return

        val contentWidth = bounds.maxX - bounds.minX + 200f // Padding
        val contentHeight = bounds.maxY - bounds.minY + 200f
        val viewWidth = width.toFloat()
        val viewHeight = height.toFloat()

        if (viewWidth == 0f || viewHeight == 0f) return

        val scaleX = viewWidth / contentWidth
        val scaleY = viewHeight / contentHeight
        viewport.scale = minOf(scaleX, scaleY).coerceIn(0.1f, 2.0f)

        val contentCenterX = (bounds.minX + bounds.maxX) / 2f
        val contentCenterY = (bounds.minY + bounds.maxY) / 2f

        viewport.translateX = viewWidth / 2f - contentCenterX * viewport.scale
        viewport.translateY = viewHeight / 2f - contentCenterY * viewport.scale

        updateTransform()
    }

    fun toggleHud() {
        showHud = !showHud
        invalidate()
    }

    private fun updateTransform() {
        transformMatrix.reset()
        transformMatrix.postScale(viewport.scale, viewport.scale)
        transformMatrix.postTranslate(viewport.translateX, viewport.translateY)
        transformMatrix.invert(inverseMatrix)
        invalidate()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        if (oldw == 0 && oldh == 0 && snapshot != null) {
            fitToScreen()
        }
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val startTime = System.nanoTime()

        canvas.drawColor(Color.parseColor("#F5F5F5")) // Background

        val snap = snapshot ?: return

        canvas.save()
        canvas.concat(transformMatrix)

        // Draw edges
        for (edge in snap.edges) {
            val fromNode = snap.nodes.find { it.id == edge.from } ?: continue
            val toNode = snap.nodes.find { it.id == edge.to } ?: continue

            edgePath.reset()
            edgePath.moveTo(fromNode.x, fromNode.y)
            // Draw a simple straight line for now, can be curved later
            edgePath.lineTo(toNode.x, toNode.y)
            canvas.drawPath(edgePath, edgePaint)
        }

        // Viewport culling bounds (in local coordinates)
        val viewRect = floatArrayOf(0f, 0f, width.toFloat(), height.toFloat())
        inverseMatrix.mapPoints(viewRect)
        // This is a rough estimation of the visible area
        val vMinX = minOf(viewRect[0], viewRect[2])
        val vMinY = minOf(viewRect[1], viewRect[3])
        val vMaxX = maxOf(viewRect[0], viewRect[2])
        val vMaxY = maxOf(viewRect[1], viewRect[3])

        visibleNodesCount = 0

        // Draw nodes
        for (node in snap.nodes) {
            val halfW = node.width / 2f
            val halfH = node.height / 2f
            val nMinX = node.x - halfW
            val nMinY = node.y - halfH
            val nMaxX = node.x + halfW
            val nMaxY = node.y + halfH

            // Simple frustum culling
            if (nMaxX < vMinX || nMinX > vMaxX || nMaxY < vMinY || nMinY > vMaxY) {
                continue
            }

            visibleNodesCount++

            nodePaint.color = when (node.kind) {
                MindMapNodeKind.Project -> Color.parseColor("#BBDEFB")
                MindMapNodeKind.Volume -> Color.parseColor("#C8E6C9")
                MindMapNodeKind.Chapter -> Color.parseColor("#FFF9C4")
            }

            nodeRect.set(nMinX, nMinY, nMaxX, nMaxY)
            canvas.drawRoundRect(nodeRect, node.radius, node.radius, nodePaint)

            if (selectedNodeId == node.id) {
                canvas.drawRoundRect(nodeRect, node.radius, node.radius, selectedNodeBorderPaint)
            } else {
                canvas.drawRoundRect(nodeRect, node.radius, node.radius, nodeBorderPaint)
            }

            // Draw text (ellipsize manually or just draw)
            val textY = node.y - (textPaint.descent() + textPaint.ascent()) / 2
            canvas.drawText(node.title, node.x, textY, textPaint)
        }

        canvas.restore()

        val drawDurationMs = (System.nanoTime() - startTime) / 1000000f
        val now = System.currentTimeMillis()
        if (lastDrawTime > 0) {
            val dt = now - lastDrawTime
            if (dt > 0) {
                // simple moving average
                fps = fps * 0.9f + (1000f / dt) * 0.1f
            }
        }
        lastDrawTime = now

        if (showHud) {
            var hudY = 40f
            canvas.drawText(String.format("FPS: %.1f", fps), 20f, hudY, hudPaint)
            hudY += 40f
            canvas.drawText(String.format("Frame Time: %.1f ms", drawDurationMs), 20f, hudY, hudPaint)
            hudY += 40f
            canvas.drawText("Nodes: $visibleNodesCount / ${snap.nodes.size}", 20f, hudY, hudPaint)
            hudY += 40f
            canvas.drawText("Parse Time: ${snap.parseTimeMs} ms", 20f, hudY, hudPaint)
            hudY += 40f
            canvas.drawText("JSON Bytes: ${snap.jsonBytes}", 20f, hudY, hudPaint)
        }
    }
}
