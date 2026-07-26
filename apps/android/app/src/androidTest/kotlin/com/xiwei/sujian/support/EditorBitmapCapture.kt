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
import com.xiwei.sujian.editor.v2.visual.ColorDistance
import org.junit.Assert
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

object EditorBitmapCapture {

    data class CapturedFrame(
        val bitmap: Bitmap,
        val width: Int,
        val height: Int,
        val backgroundColor: Int
    ) {
        fun pixel(x: Int, y: Int): Int = bitmap.getPixel(x, y)

        fun alpha(x: Int, y: Int): Int = Color.alpha(bitmap.getPixel(x, y))

        fun red(x: Int, y: Int): Int = Color.red(bitmap.getPixel(x, y))

        fun green(x: Int, y: Int): Int = Color.green(bitmap.getPixel(x, y))

        fun blue(x: Int, y: Int): Int = Color.blue(bitmap.getPixel(x, y))

        fun isPixelNonBackground(x: Int, y: Int): Boolean {
            return !ColorDistance.isClose(bitmap.getPixel(x, y), backgroundColor)
        }

        fun regionHasNonBackgroundPixels(
            left: Int, top: Int, right: Int, bottom: Int
        ): Boolean {
            for (y in top until bottom) {
                for (x in left until right) {
                    if (x < width && y < height && !ColorDistance.isClose(bitmap.getPixel(x, y), backgroundColor)) {
                        return true
                    }
                }
            }
            return false
        }

        fun countNonBackgroundPixels(
            left: Int, top: Int, right: Int, bottom: Int
        ): Int {
            var count = 0
            for (y in top until bottom) {
                for (x in left until right) {
                    if (x < width && y < height && !ColorDistance.isClose(bitmap.getPixel(x, y), backgroundColor)) {
                        count++
                    }
                }
            }
            return count
        }

        fun findFirstNonBackgroundPixel(): Pair<Int, Int>? {
            for (y in 0 until height) {
                for (x in 0 until width) {
                    if (!ColorDistance.isClose(bitmap.getPixel(x, y), backgroundColor)) {
                        return Pair(x, y)
                    }
                }
            }
            return null
        }

        fun findLastNonBackgroundPixel(): Pair<Int, Int>? {
            var last: Pair<Int, Int>? = null
            for (y in 0 until height) {
                for (x in 0 until width) {
                    if (!ColorDistance.isClose(bitmap.getPixel(x, y), backgroundColor)) {
                        last = Pair(x, y)
                    }
                }
            }
            return last
        }

        fun contentBounds(): Rect {
            var minX = width
            var minY = height
            var maxX = 0
            var maxY = 0
            for (y in 0 until height) {
                for (x in 0 until width) {
                    if (!ColorDistance.isClose(bitmap.getPixel(x, y), backgroundColor)) {
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
        val view = resolveEditorView()
        return captureViewBitmap(view)
    }

    fun capturePixelCopyBitmap(): CapturedFrame {
        val view = resolveEditorView()
        return capturePixelCopyOnly(view)
    }

    fun captureSoftwareBitmap(): CapturedFrame {
        val view = resolveEditorView()
        return captureSoftwareBitmap(view)
    }

    private fun resolveEditorView(): SujianEditorView {
        var viewRef: SujianEditorView? = null
        Espresso.onView(ViewMatchers.withId(R.id.editor_content))
            .check { view, _ ->
                viewRef = view as? SujianEditorView
                    ?: throw AssertionError("View is not a SujianEditorView")
            }
        return viewRef ?: throw AssertionError("Failed to resolve editor view")
    }

    fun captureViewBitmap(view: SujianEditorView): CapturedFrame {
        Assert.assertTrue("View must be laid out to capture bitmap", view.width > 0 && view.height > 0)

        val backgroundColor = view.getThemeBackgroundColor()
        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val ctx = view.context
            if (ctx is android.app.Activity && view.windowToken != null) {
                val srcRect = computeSrcRect(view)
                val latch = CountDownLatch(1)
                var captureSucceeded = false
                try {
                    PixelCopy.request(ctx.window, srcRect, bitmap, { result ->
                        captureSucceeded = result == PixelCopy.SUCCESS
                        latch.countDown()
                    }, Handler(Looper.getMainLooper()))
                    val awaited = latch.await(3, TimeUnit.SECONDS)
                    if (awaited && captureSucceeded) {
                        return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height, backgroundColor = backgroundColor)
                    }
                } catch (_: Exception) {
                }
            }
        }

        drawViewToBitmap(view, bitmap)
        return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height, backgroundColor = backgroundColor)
    }

    fun capturePixelCopyOnly(view: SujianEditorView): CapturedFrame {
        Assert.assertTrue("View must be laid out to capture bitmap", view.width > 0 && view.height > 0)
        Assert.assertTrue("PixelCopy requires API 26+", Build.VERSION.SDK_INT >= Build.VERSION_CODES.O)

        val backgroundColor = view.getThemeBackgroundColor()
        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        val ctx = view.context
        Assert.assertTrue("View must be attached to an Activity", ctx is android.app.Activity && view.windowToken != null)

        val srcRect = computeSrcRect(view)
        val latch = CountDownLatch(1)
        var captureSucceeded = false
        var captureResult: Int = -1
        try {
            PixelCopy.request(ctx.window, srcRect, bitmap, { result ->
                captureResult = result
                captureSucceeded = result == PixelCopy.SUCCESS
                latch.countDown()
            }, Handler(Looper.getMainLooper()))
            val awaited = latch.await(3, TimeUnit.SECONDS)
            if (!awaited) {
                Assert.fail("PixelCopy timed out: callback never executed on main thread. Ensure this method is called from the test (instrumentation) thread, not from an Espresso .check callback on the main thread.")
            }
            if (!captureSucceeded) {
                Assert.fail("PixelCopy failed with result code $captureResult. Hardware window capture did not succeed.")
            }
        } catch (e: Exception) {
            Assert.fail("PixelCopy threw exception: ${e.message}")
        }

        return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height, backgroundColor = backgroundColor)
    }

    fun captureSoftwareBitmap(view: SujianEditorView): CapturedFrame {
        Assert.assertTrue("View must be laid out to capture bitmap", view.width > 0 && view.height > 0)
        val backgroundColor = view.getThemeBackgroundColor()
        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        drawViewToBitmap(view, bitmap)
        return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height, backgroundColor = backgroundColor)
    }

    private fun computeSrcRect(view: View): Rect {
        val location = IntArray(2)
        view.getLocationInWindow(location)
        return Rect(location[0], location[1], location[0] + view.width, location[1] + view.height)
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
        message: String = "Region should be background only"
    ) {
        Assert.assertFalse(
            message,
            frame.regionHasNonBackgroundPixels(left, top, right, bottom)
        )
    }
}
