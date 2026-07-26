package com.xiwei.sujian.support

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Rect
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.PixelCopy
import android.view.View
import androidx.test.espresso.Espresso
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import org.junit.Assert
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

object EditorBitmapCapture {

    data class CapturedFrame(
        val bitmap: Bitmap,
        val width: Int,
        val height: Int
    ) {
        fun pixel(x: Int, y: Int): Int = bitmap.getPixel(x, y)

        fun alpha(x: Int, y: Int): Int = Color.alpha(bitmap.getPixel(x, y))

        fun red(x: Int, y: Int): Int = Color.red(bitmap.getPixel(x, y))

        fun green(x: Int, y: Int): Int = Color.green(bitmap.getPixel(x, y))

        fun blue(x: Int, y: Int): Int = Color.blue(bitmap.getPixel(x, y))

        fun isPixelNonBackground(x: Int, y: Int, backgroundColor: Int = Color.WHITE): Boolean {
            return bitmap.getPixel(x, y) != backgroundColor
        }

        fun regionHasNonBackgroundPixels(
            left: Int, top: Int, right: Int, bottom: Int,
            backgroundColor: Int = Color.WHITE
        ): Boolean {
            for (y in top until bottom) {
                for (x in left until right) {
                    if (x < width && y < height && bitmap.getPixel(x, y) != backgroundColor) {
                        return true
                    }
                }
            }
            return false
        }

        fun countNonBackgroundPixels(
            left: Int, top: Int, right: Int, bottom: Int,
            backgroundColor: Int = Color.WHITE
        ): Int {
            var count = 0
            for (y in top until bottom) {
                for (x in left until right) {
                    if (x < width && y < height && bitmap.getPixel(x, y) != backgroundColor) {
                        count++
                    }
                }
            }
            return count
        }

        fun findFirstNonBackgroundPixel(
            backgroundColor: Int = Color.WHITE
        ): Pair<Int, Int>? {
            for (y in 0 until height) {
                for (x in 0 until width) {
                    if (bitmap.getPixel(x, y) != backgroundColor) {
                        return Pair(x, y)
                    }
                }
            }
            return null
        }

        fun findLastNonBackgroundPixel(
            backgroundColor: Int = Color.WHITE
        ): Pair<Int, Int>? {
            var last: Pair<Int, Int>? = null
            for (y in 0 until height) {
                for (x in 0 until width) {
                    if (bitmap.getPixel(x, y) != backgroundColor) {
                        last = Pair(x, y)
                    }
                }
            }
            return last
        }

        fun contentBounds(
            backgroundColor: Int = Color.WHITE
        ): Rect {
            var minX = width
            var minY = height
            var maxX = 0
            var maxY = 0
            for (y in 0 until height) {
                for (x in 0 until width) {
                    if (bitmap.getPixel(x, y) != backgroundColor) {
                        if (x < minX) minX = x
                        if (y < minY) minY = y
                        if (x > maxX) maxX = x
                        if (y > maxY) maxY = y
                    }
                }
            }
            return Rect(minX, minY, maxX, maxY)
        }
    }

    fun captureEditorBitmap(): CapturedFrame {
        var captured: CapturedFrame? = null
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check { view, _ ->
                val editorView = view as? SujianEditorView
                    ?: throw AssertionError("View is not a SujianEditorView")
                captured = captureViewBitmap(editorView)
            }
        return captured ?: throw AssertionError("Failed to capture editor bitmap")
    }

    fun captureViewBitmap(view: SujianEditorView): CapturedFrame {
        Assert.assertTrue("View must be laid out to capture bitmap", view.width > 0 && view.height > 0)

        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val ctx = view.context
            if (ctx is android.app.Activity && view.windowToken != null) {
                val latch = CountDownLatch(1)
                var captureSucceeded = false
                try {
                    PixelCopy.request(ctx.window, bitmap, { result ->
                        captureSucceeded = result == PixelCopy.SUCCESS
                        latch.countDown()
                    }, Handler(Looper.getMainLooper()))
                    val awaited = latch.await(3, TimeUnit.SECONDS)
                    if (awaited && captureSucceeded) {
                        return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height)
                    }
                } catch (_: Exception) { }
            }
        }

        drawViewToBitmap(view, bitmap)
        return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height)
    }

    private fun drawViewToBitmap(view: View, bitmap: Bitmap) {
        val canvas = Canvas(bitmap)
        view.draw(canvas)
    }

    fun assertBitmapHasContent(
        frame: CapturedFrame,
        message: String = "Captured frame should have non-background content"
    ) {
        val bounds = frame.contentBounds()
        Assert.assertTrue(
            message,
            bounds.width() > 0 && bounds.height() > 0
        )
    }

    fun assertBitmapRegionHasContent(
        frame: CapturedFrame,
        left: Int, top: Int, right: Int, bottom: Int,
        message: String = "Region should have non-background content"
    ) {
        Assert.assertTrue(
            message,
            frame.regionHasNonBackgroundPixels(left, top, right, bottom)
        )
    }

    fun assertBitmapRegionIsEmpty(
        frame: CapturedFrame,
        left: Int, top: Int, right: Int, bottom: Int,
        backgroundColor: Int = Color.WHITE,
        message: String = "Region should be background only"
    ) {
        Assert.assertFalse(
            message,
            frame.regionHasNonBackgroundPixels(left, top, right, bottom, backgroundColor)
        )
    }
}
