package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.*
import org.junit.Test

class ContentBoundsExclusiveBoundaryTest {

    private data class Bounds(val left: Int, val top: Int, val right: Int, val bottom: Int) {
        val width: Int get() = right - left
        val height: Int get() = bottom - top
    }

    private fun computeExclusiveBounds(
        width: Int, height: Int,
        backgroundColor: Int, pixels: Array<IntArray>
    ): Bounds {
        var minX = width
        var minY = height
        var maxX = 0
        var maxY = 0
        for (y in 0 until height) {
            for (x in 0 until width) {
                if (!ColorDistance.isClose(pixels[y][x], backgroundColor)) {
                    if (x < minX) minX = x
                    if (y < minY) minY = y
                    if (x > maxX) maxX = x
                    if (y > maxY) maxY = y
                }
            }
        }
        return Bounds(minX, minY, maxX + 1, maxY + 1)
    }

    private val BG = ColorDistance.rgb(255, 255, 255)
    private val FG = ColorDistance.rgb(0, 0, 0)

    private fun makePixels(width: Int, height: Int, nonBgPixels: Set<Pair<Int, Int>>): Array<IntArray> {
        val pixels = Array(height) { IntArray(width) { BG } }
        for ((x, y) in nonBgPixels) {
            pixels[y][x] = FG
        }
        return pixels
    }

    @Test
    fun singlePixel_exclusiveRightBottom() {
        val bounds = computeExclusiveBounds(10, 10, BG, makePixels(10, 10, setOf(Pair(3, 4))))
        assertEquals(Bounds(3, 4, 4, 5), bounds)
        assertEquals(1, bounds.width)
        assertEquals(1, bounds.height)
    }

    @Test
    fun twoPixelsWide_exclusiveRightBottom() {
        val bounds = computeExclusiveBounds(10, 10, BG, makePixels(10, 10, setOf(Pair(2, 3), Pair(3, 3))))
        assertEquals(Bounds(2, 3, 4, 4), bounds)
        assertEquals(2, bounds.width)
    }

    @Test
    fun twoPixelsTall_exclusiveRightBottom() {
        val bounds = computeExclusiveBounds(10, 10, BG, makePixels(10, 10, setOf(Pair(5, 1), Pair(5, 2))))
        assertEquals(Bounds(5, 1, 6, 3), bounds)
        assertEquals(2, bounds.height)
    }

    @Test
    fun rect_exclusiveRightBottom() {
        val pixels = mutableSetOf<Pair<Int, Int>>()
        for (y in 2..4) for (x in 1..3) pixels.add(Pair(x, y))
        val bounds = computeExclusiveBounds(10, 10, BG, makePixels(10, 10, pixels))
        assertEquals(Bounds(1, 2, 4, 5), bounds)
        assertEquals(3, bounds.width)
        assertEquals(3, bounds.height)
    }

    @Test
    fun cornerPixel_exclusiveRightBottom() {
        val bounds = computeExclusiveBounds(10, 10, BG, makePixels(10, 10, setOf(Pair(9, 9))))
        assertEquals(Bounds(9, 9, 10, 10), bounds)
        assertEquals(1, bounds.width)
        assertEquals(1, bounds.height)
    }

    @Test
    fun rgbDistance_distinguishesContentFromBackground() {
        val pixel = ColorDistance.rgb(100, 100, 100)
        val bg = ColorDistance.rgb(255, 255, 255)
        val dr = ColorDistance.red(pixel) - ColorDistance.red(bg)
        val dg = ColorDistance.green(pixel) - ColorDistance.green(bg)
        val db = ColorDistance.blue(pixel) - ColorDistance.blue(bg)
        val rgbDistSq = dr * dr + dg * dg + db * db
        assertTrue(
            "Mid-gray on white must be visually distinct: RGB²=$rgbDistSq > tolerance²=${ColorDistance.BACKGROUND_TOLERANCE * ColorDistance.BACKGROUND_TOLERANCE}",
            rgbDistSq > ColorDistance.BACKGROUND_TOLERANCE * ColorDistance.BACKGROUND_TOLERANCE
        )
    }

    @Test
    fun rgbDistance_nearBackground_isClose() {
        val pixel = ColorDistance.rgb(250, 250, 250)
        val bg = ColorDistance.rgb(255, 255, 255)
        assertTrue(ColorDistance.isClose(pixel, bg))
    }
}
