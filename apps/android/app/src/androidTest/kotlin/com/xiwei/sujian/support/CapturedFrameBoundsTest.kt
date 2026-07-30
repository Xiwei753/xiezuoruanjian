package com.xiwei.sujian.support

import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Rect
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.editor.v2.visual.ColorDistance
import com.xiwei.sujian.support.SujianSmallTest
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
@SujianSmallTest
class CapturedFrameBoundsTest {

    private fun makeFrame(
        width: Int, height: Int, backgroundColor: Int,
        contentPixels: Set<Pair<Int, Int>>, contentColor: Int
    ): EditorBitmapCapture.CapturedFrame {
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        for (y in 0 until height) {
            for (x in 0 until width) {
                bitmap.setPixel(x, y, backgroundColor)
            }
        }
        for ((x, y) in contentPixels) {
            bitmap.setPixel(x, y, contentColor)
        }
        return EditorBitmapCapture.CapturedFrame(
            bitmap = bitmap, width = width, height = height,
            backgroundColor = backgroundColor
        )
    }

    @Test
    fun contentBounds_singlePixel_exclusiveRightBottom() {
        val frame = makeFrame(10, 10, Color.WHITE, setOf(3 to 5), Color.BLACK)
        val bounds = frame.contentBounds()
        assertEquals("left should be 3", 3, bounds.left)
        assertEquals("top should be 5", 5, bounds.top)
        assertEquals("right should be 4 (exclusive)", 4, bounds.right)
        assertEquals("bottom should be 6 (exclusive)", 6, bounds.bottom)
        assertEquals("width should be 1", 1, bounds.width())
        assertEquals("height should be 1", 1, bounds.height())
    }

    @Test
    fun contentBounds_twoPixelWide_exclusiveBounds() {
        val frame = makeFrame(10, 10, Color.WHITE, setOf(2 to 3, 3 to 3), Color.BLACK)
        val bounds = frame.contentBounds()
        assertEquals("left should be 2", 2, bounds.left)
        assertEquals("right should be 4 (exclusive)", 4, bounds.right)
        assertEquals("width should be 2", 2, bounds.width())
    }

    @Test
    fun contentBounds_twoPixelTall_exclusiveBounds() {
        val frame = makeFrame(10, 10, Color.WHITE, setOf(2 to 3, 2 to 4), Color.BLACK)
        val bounds = frame.contentBounds()
        assertEquals("top should be 3", 3, bounds.top)
        assertEquals("bottom should be 5 (exclusive)", 5, bounds.bottom)
        assertEquals("height should be 2", 2, bounds.height())
    }

    @Test
    fun contentBounds_rectMatchesAndroidRectSemantics() {
        val frame = makeFrame(20, 20, Color.WHITE,
            setOf(5 to 7, 5 to 8, 5 to 9, 6 to 7, 6 to 8, 6 to 9),
            Color.BLACK)
        val bounds = frame.contentBounds()
        val manualRect = Rect(5, 7, 7, 10)
        assertEquals("left", manualRect.left, bounds.left)
        assertEquals("top", manualRect.top, bounds.top)
        assertEquals("right (exclusive)", manualRect.right, bounds.right)
        assertEquals("bottom (exclusive)", manualRect.bottom, bounds.bottom)
        assertEquals("width", manualRect.width(), bounds.width())
        assertEquals("height", manualRect.height(), bounds.height())
    }

    @Test
    fun regionClipping_negativeCoordinates_clippedToZero() {
        val frame = makeFrame(10, 10, Color.WHITE, setOf(0 to 0), Color.BLACK)
        assertTrue(
            "Region with negative left/top should clip to 0 and find content at (0,0)",
            frame.regionHasNonBackgroundPixels(-2, -2, 2, 2)
        )
    }

    @Test
    fun regionClipping_coordinatesExceedingBounds_clippedToWidthHeight() {
        val frame = makeFrame(10, 10, Color.WHITE, setOf(9 to 9), Color.BLACK)
        assertTrue(
            "Region exceeding bitmap bounds should clip and find content at (9,9)",
            frame.regionHasNonBackgroundPixels(8, 8, 15, 15)
        )
    }

    @Test
    fun regionClipping_fullyOutOfBounds_returnsFalse() {
        val frame = makeFrame(10, 10, Color.WHITE, setOf(3 to 3), Color.BLACK)
        assertFalse(
            "Region fully outside bitmap should return false",
            frame.regionHasNonBackgroundPixels(15, 15, 20, 20)
        )
    }

    @Test
    fun countNonBackgroundPixels_clippingMatchesRegion() {
        val frame = makeFrame(10, 10, Color.WHITE,
            setOf(0 to 0, 1 to 0, 9 to 9), Color.BLACK)
        assertEquals(
            "Count with clipping should find 2 pixels in top-left region",
            2, frame.countNonBackgroundPixels(-1, -1, 3, 3)
        )
        assertEquals(
            "Count with clipping should find 1 pixel in bottom-right region",
            1, frame.countNonBackgroundPixels(8, 8, 12, 12)
        )
    }
}
