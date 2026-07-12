package com.xiwei.sujian.editor.selfrender

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.RenderNode
import android.os.Build
import android.text.Layout

/**
 * 行视觉资源的平台抽象。
 *
 * API 29+ 使用 [RenderNode]（[RenderNodeVisualResource]），
 * API 24–28 使用 [Bitmap]（[BitmapVisualResource]），是同一抽象的不同后端。
 *
 * [record] 负责一次性录制完整视觉行；动画帧只修改 destination/alpha/scale 并裁剪，
 * 不再调用 `drawText()` 重新 shaping。
 *
 * 资源释放和重复录制的生命周期约束：[release] 必须可重复调用；
 * 事务结束前不能释放仍被 slice 引用的资源。
 */
interface AndroidLineVisualResource {
    fun record(layout: Layout, lineIdx: Int, textPaint: Paint, textColor: Int, scrollX: Int, scrollY: Int)
    fun drawSlice(canvas: Canvas, sourceRect: RectF, destinationRect: RectF, alpha: Int, scale: Float)
    fun release()
}

/**
 * API 29+ 后端：使用 [RenderNode] 录制行视觉资源。
 * 动画帧通过 alpha/translate/clip 变换 RenderNode，不重新 shaping。
 */
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
        recordingCanvas.clipRect(0f, 0f, width.toFloat(), height.toFloat())
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

/**
 * API 24–28 后端：使用 [Bitmap] 录制行视觉资源。
 * 动画帧通过 drawBitmap + src/dst rect 变换，不重新 shaping。
 */
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
        bmpCanvas.clipRect(0f, 0f, w.toFloat(), h.toFloat())
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
