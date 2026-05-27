package com.xiwei.writerapp.ui

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.view.MotionEvent
import android.view.View
import com.xiwei.writerapp.model.StarMapData

class StarMapCanvasView(context: Context) : View(context) {

    private var data: StarMapData? = null

    private val paintNode = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.parseColor("#2A2E36") }
    private val paintText = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE
        textSize = 36f
        textAlign = Paint.Align.CENTER
    }
    private val paintEdge = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.GRAY
        strokeWidth = 3f
        style = Paint.Style.STROKE
    }

    var panX = 0f
    var panY = 0f
    var lastTouchX = 0f
    var lastTouchY = 0f
    var draggingNodeId: String? = null
    var onLayoutChangedListener: (() -> Unit)? = null

    fun getData(): StarMapData? = data

    fun setData(newData: StarMapData) {
        this.data = newData
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.drawColor(Color.parseColor("#111318"))

        val currentData = data ?: return

        canvas.save()
        canvas.translate(panX, panY)

        for (edge in currentData.graph.edges) {
            val fromLayout = currentData.layout.nodes.find { it.nodeId == edge.from }
            val toLayout = currentData.layout.nodes.find { it.nodeId == edge.to }

            if (fromLayout != null && toLayout != null) {
                canvas.drawLine(
                    fromLayout.x + fromLayout.width / 2,
                    fromLayout.y + fromLayout.height / 2,
                    toLayout.x + toLayout.width / 2,
                    toLayout.y + toLayout.height / 2,
                    paintEdge
                )
            }
        }

        for (node in currentData.graph.nodes) {
            val layout = currentData.layout.nodes.find { it.nodeId == node.id }
            if (layout != null) {
                val rect = RectF(layout.x, layout.y, layout.x + layout.width, layout.y + layout.height)
                canvas.drawRoundRect(rect, 16f, 16f, paintNode)

                canvas.drawText(
                    node.title,
                    layout.x + layout.width / 2,
                    layout.y + layout.height / 2 + 12f,
                    paintText
                )
            }
        }

        canvas.restore()
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val x = event.x
        val y = event.y

        when (event.action) {
            MotionEvent.ACTION_DOWN -> {
                lastTouchX = x
                lastTouchY = y

                val graphX = x - panX
                val graphY = y - panY

                draggingNodeId = null
                data?.layout?.nodes?.forEach { layout ->
                    if (graphX >= layout.x && graphX <= layout.x + layout.width &&
                        graphY >= layout.y && graphY <= layout.y + layout.height) {
                        draggingNodeId = layout.nodeId
                    }
                }
            }
            MotionEvent.ACTION_MOVE -> {
                val dx = x - lastTouchX
                val dy = y - lastTouchY

                if (draggingNodeId != null) {
                    data?.layout?.nodes?.find { it.nodeId == draggingNodeId }?.let { layout ->
                        val newLayout = layout.copy(x = layout.x + dx, y = layout.y + dy)
                        val mutableNodes = data!!.layout.nodes.toMutableList()
                        val idx = mutableNodes.indexOf(layout)
                        if (idx != -1) {
                            mutableNodes[idx] = newLayout
                            data = data!!.copy(layout = data!!.layout.copy(nodes = mutableNodes))
                        }
                    }
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
                    onLayoutChangedListener?.invoke()
                }
                draggingNodeId = null
            }
        }
        return true
    }
}
