package com.xiwei.sujian.editor.selfrender

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.RenderNode
import android.os.Build
import android.text.Layout

interface AndroidLineVisualResource {
    fun record(layout: Layout, lineIdx: Int, textPaint: Paint, textColor: Int, scrollX: Int, scrollY: Int)
    fun drawSlice(canvas: Canvas, sourceRect: RectF, destinationRect: RectF, alpha: Int, scale: Float)
    fun release()
}

class RenderNodeVisualResource(
    private val name: String
) : AndroidLineVisualResource {

    private var renderNode: RenderNode? = null
    private var width = 0
    private var height = 0

    override fun record(layout: Layout, lineIdx: Int, textPaint: Paint, textColor: Int, scrollX: Int, scrollY: Int) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return

        val lineTop = layout.getLineTop(lineIdx)
        val lineBottom = layout.getLineBottom(lineIdx)
        val lineLeft = layout.getLineLeft(lineIdx).toInt()
        val lineRight = layout.getLineRight(lineIdx).toInt()
        width = (lineRight - lineLeft).coerceAtLeast(1)
        height = (lineBottom - lineTop).coerceAtLeast(1)

        val node = renderNode ?: RenderNode(name).also { renderNode = it }
        node.setPosition(0, 0, width, height)

        val recordingCanvas = node.beginRecording(width, height)
        recordingCanvas.translate(-lineLeft.toFloat(), -lineTop.toFloat())
        val savedPaint = textPaint.color
        textPaint.color = textColor
        recordingCanvas.save()
        val clipPath = android.graphics.Path()
        layout.getSelectionPath(
            layout.getLineStart(lineIdx),
            layout.getLineEnd(lineIdx),
            clipPath
        )
        if (!clipPath.isEmpty) {
            recordingCanvas.clipPath(clipPath)
        }
        layout.draw(recordingCanvas)
        recordingCanvas.restore()
        textPaint.color = savedPaint
        node.endRecording()
    }

    override fun drawSlice(canvas: Canvas, sourceRect: RectF, destinationRect: RectF, alpha: Int, scale: Float) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        val node = renderNode ?: return
        if (width <= 0 || height <= 0) return

        node.alpha = alpha / 255f
        canvas.save()
        canvas.scale(scale, scale, destinationRect.centerX(), destinationRect.centerY())
        canvas.translate(destinationRect.left - sourceRect.left, destinationRect.top - sourceRect.top)
        canvas.clipRect(sourceRect.left.toInt(), sourceRect.top.toInt(),
            sourceRect.right.toInt().coerceAtMost(width),
            sourceRect.bottom.toInt().coerceAtMost(height))
        canvas.drawRenderNode(node)
        canvas.restore()
    }

    override fun release() {
        renderNode = null
    }
}

class BitmapVisualResource : AndroidLineVisualResource {

    private var bitmap: Bitmap? = null
    private var bitmapWidth = 0
    private var bitmapHeight = 0
    private val bitmapPaint = Paint(Paint.FILTER_BITMAP_FLAG)

    override fun record(layout: Layout, lineIdx: Int, textPaint: Paint, textColor: Int, scrollX: Int, scrollY: Int) {
        val lineTop = layout.getLineTop(lineIdx)
        val lineBottom = layout.getLineBottom(lineIdx)
        val lineLeft = layout.getLineLeft(lineIdx).toInt()
        val lineRight = layout.getLineRight(lineIdx).toInt()
        val w = (lineRight - lineLeft).coerceAtLeast(1)
        val h = (lineBottom - lineTop).coerceAtLeast(1)

        if (w != bitmapWidth || h != bitmapHeight || bitmap == null) {
            bitmap?.recycle()
            bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
            bitmapWidth = w
            bitmapHeight = h
        }

        val bmp = bitmap ?: return
        bmp.eraseColor(0)
        val bmpCanvas = Canvas(bmp)
        bmpCanvas.translate(-lineLeft.toFloat(), -lineTop.toFloat())
        val savedPaint = textPaint.color
        textPaint.color = textColor
        bmpCanvas.save()
        val clipPath = android.graphics.Path()
        layout.getSelectionPath(
            layout.getLineStart(lineIdx),
            layout.getLineEnd(lineIdx),
            clipPath
        )
        if (!clipPath.isEmpty) {
            bmpCanvas.clipPath(clipPath)
        }
        layout.draw(bmpCanvas)
        bmpCanvas.restore()
        textPaint.color = savedPaint
    }

    override fun drawSlice(canvas: Canvas, sourceRect: RectF, destinationRect: RectF, alpha: Int, scale: Float) {
        val bmp = bitmap ?: return
        if (bmp.isRecycled) return

        bitmapPaint.alpha = alpha
        canvas.save()
        if (scale != 1f) {
            canvas.scale(scale, scale, destinationRect.centerX(), destinationRect.centerY())
        }
        val src = Rect(
            sourceRect.left.toInt(), sourceRect.top.toInt(),
            sourceRect.right.toInt().coerceAtMost(bitmapWidth),
            sourceRect.bottom.toInt().coerceAtMost(bitmapHeight)
        )
        canvas.drawBitmap(bmp, src, destinationRect, bitmapPaint)
        canvas.restore()
    }

    override fun release() {
        bitmap?.recycle()
        bitmap = null
    }
}

object AndroidLineVisualResourceFactory {
    fun create(lineIdx: Int): AndroidLineVisualResource {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            RenderNodeVisualResource("line_snapshot_$lineIdx")
        } else {
            BitmapVisualResource()
        }
    }
}
