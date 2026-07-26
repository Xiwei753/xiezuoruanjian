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
import androidx.test.platform.app.InstrumentationRegistry
import com.xiwei.sujian.R
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.visual.CaptureMethod
import com.xiwei.sujian.editor.v2.visual.ColorDistance
import com.xiwei.sujian.editor.v2.visual.PixelCopyResult
import org.junit.Assert
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

object EditorBitmapCapture {

    data class CapturedFrame(
        val bitmap: Bitmap,
        val width: Int,
        val height: Int,
        val backgroundColor: Int,
        val captureMethod: CaptureMethod = CaptureMethod.SOFTWARE_DRAW
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
            val clippedLeft = left.coerceIn(0, width)
            val clippedTop = top.coerceIn(0, height)
            val clippedRight = right.coerceIn(0, width)
            val clippedBottom = bottom.coerceIn(0, height)
            for (y in clippedTop until clippedBottom) {
                for (x in clippedLeft until clippedRight) {
                    if (!ColorDistance.isClose(bitmap.getPixel(x, y), backgroundColor)) {
                        return true
                    }
                }
            }
            return false
        }

        fun countNonBackgroundPixels(
            left: Int, top: Int, right: Int, bottom: Int
        ): Int {
            val clippedLeft = left.coerceIn(0, width)
            val clippedTop = top.coerceIn(0, height)
            val clippedRight = right.coerceIn(0, width)
            val clippedBottom = bottom.coerceIn(0, height)
            var count = 0
            for (y in clippedTop until clippedBottom) {
                for (x in clippedLeft until clippedRight) {
                    if (!ColorDistance.isClose(bitmap.getPixel(x, y), backgroundColor)) {
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
            if (maxX < minX || maxY < minY) {
                return Rect(0, 0, 0, 0)
            }
            return Rect(minX, minY, maxX + 1, maxY + 1)
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

        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)

        val pixelCopyResult = tryPixelCopy(view, bitmap)
        when (pixelCopyResult) {
            PixelCopyResult.TIMED_OUT ->
                Assert.fail("PixelCopy timed out during bitmap capture. Use captureSoftwareBitmap() for software rendering tests.")
            PixelCopyResult.FAILED ->
                Assert.fail("PixelCopy failed during bitmap capture. Use captureSoftwareBitmap() for software rendering tests.")
            PixelCopyResult.SUCCESS -> {
                val backgroundColor = sampleBackgroundColorFromCorners(bitmap)
                return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height, backgroundColor = backgroundColor, captureMethod = CaptureMethod.PIXEL_COPY)
            }
            PixelCopyResult.NOT_SUPPORTED -> {}
        }

        drawViewToBitmap(view, bitmap)
        val backgroundColor = sampleBackgroundColorFromCorners(bitmap)
        return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height, backgroundColor = backgroundColor, captureMethod = CaptureMethod.SOFTWARE_DRAW)
    }

    fun capturePixelCopyOnly(view: SujianEditorView): CapturedFrame {
        Assert.assertTrue("View must be laid out to capture bitmap", view.width > 0 && view.height > 0)
        Assert.assertTrue("PixelCopy requires API 26+", Build.VERSION.SDK_INT >= Build.VERSION_CODES.O)

        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        val ctx = view.context
        Assert.assertTrue("View must be attached to an Activity", ctx is android.app.Activity && view.windowToken != null)

        val result = tryPixelCopy(view, bitmap)
        when (result) {
            PixelCopyResult.TIMED_OUT ->
                Assert.fail("PixelCopy timed out: callback never executed on main thread. Ensure this method is called from the test (instrumentation) thread, not from an Espresso .check callback on the main thread.")
            PixelCopyResult.FAILED ->
                Assert.fail("PixelCopy failed. Hardware window capture did not succeed.")
            PixelCopyResult.NOT_SUPPORTED ->
                Assert.fail("PixelCopy not supported on this device/API level.")
            PixelCopyResult.SUCCESS -> {}
        }

        val backgroundColor = sampleBackgroundColorFromCorners(bitmap)
        return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height, backgroundColor = backgroundColor, captureMethod = CaptureMethod.PIXEL_COPY)
    }

    fun captureSoftwareBitmap(view: SujianEditorView): CapturedFrame {
        Assert.assertTrue("View must be laid out to capture bitmap", view.width > 0 && view.height > 0)
        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        drawViewToBitmap(view, bitmap)
        val backgroundColor = sampleBackgroundColorFromCorners(bitmap)
        return CapturedFrame(bitmap = bitmap, width = view.width, height = view.height, backgroundColor = backgroundColor, captureMethod = CaptureMethod.SOFTWARE_DRAW)
    }

    private fun tryPixelCopy(view: SujianEditorView, bitmap: Bitmap): PixelCopyResult {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return PixelCopyResult.NOT_SUPPORTED
        }
        val ctx = view.context
        if (ctx !is android.app.Activity || view.windowToken == null) {
            return PixelCopyResult.NOT_SUPPORTED
        }
        if (Looper.myLooper() == Looper.getMainLooper()) {
            Assert.fail(
                "tryPixelCopy must not be called on the main thread. " +
                "PixelCopy callback runs on the main thread Handler; calling latch.await() on the main thread would deadlock. " +
                "Call from the instrumentation/test thread instead."
            )
        }

        val srcRect = computeSrcRect(view)
        val latch = CountDownLatch(1)
        val captureSucceeded = AtomicBoolean(false)
        try {
            PixelCopy.request(ctx.window, srcRect, bitmap, { result ->
                captureSucceeded.set(result == PixelCopy.SUCCESS)
                latch.countDown()
            }, Handler(Looper.getMainLooper()))
            val awaited = latch.await(3, TimeUnit.SECONDS)
            if (!awaited) {
                return PixelCopyResult.TIMED_OUT
            }
            if (!captureSucceeded.get()) {
                return PixelCopyResult.FAILED
            }
            return PixelCopyResult.SUCCESS
        } catch (_: Exception) {
            return PixelCopyResult.FAILED
        }
    }

    private fun computeSrcRect(view: View): Rect {
        val location = IntArray(2)
        view.getLocationInWindow(location)
        return Rect(location[0], location[1], location[0] + view.width, location[1] + view.height)
    }

    fun sampleBackgroundColorFromCorners(bitmap: Bitmap): Int {
        val w = bitmap.width
        val h = bitmap.height
        if (w < 8 || h < 8) return bitmap.getPixel(0, 0)
        val margin = 4
        val cornerPixels = intArrayOf(
            bitmap.getPixel(margin, margin),
            bitmap.getPixel(w - margin - 1, margin),
            bitmap.getPixel(margin, h - margin - 1),
            bitmap.getPixel(w - margin - 1, h - margin - 1)
        )
        val grouped = cornerPixels.toList().groupBy { it }
        val mostCommon = grouped.maxByOrNull { it.value.size }!!.key
        return mostCommon
    }

    private fun drawViewToBitmap(view: View, bitmap: Bitmap) {
        val canvas = Canvas(bitmap)
        val latch = CountDownLatch(1)
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.runOnMainSync {
            try {
                view.draw(canvas)
            } finally {
                latch.countDown()
            }
        }
        latch.await(5, TimeUnit.SECONDS)
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
