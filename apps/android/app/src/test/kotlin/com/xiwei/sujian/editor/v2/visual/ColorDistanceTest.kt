package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.*
import org.junit.Test

class ColorDistanceTest {

    @Test
    fun exactSameColor_isClose() {
        val white = ColorDistance.rgb(255, 255, 255)
        assertTrue(ColorDistance.isClose(white, white))
    }

    @Test
    fun blackOnWhite_isNotClose() {
        val black = ColorDistance.rgb(0, 0, 0)
        val white = ColorDistance.rgb(255, 255, 255)
        assertFalse(ColorDistance.isClose(black, white))
    }

    @Test
    fun nearWhite_onWhite_isClose() {
        val nearWhite = ColorDistance.rgb(250, 250, 250)
        val white = ColorDistance.rgb(255, 255, 255)
        assertTrue(ColorDistance.isClose(nearWhite, white))
    }

    @Test
    fun gray200_onWhite_isNotClose() {
        val gray = ColorDistance.rgb(200, 200, 200)
        val white = ColorDistance.rgb(255, 255, 255)
        assertFalse(ColorDistance.isClose(gray, white))
    }

    @Test
    fun whiteOnDarkBg_isNotClose() {
        val darkBg = ColorDistance.rgb(30, 30, 30)
        val white = ColorDistance.rgb(255, 255, 255)
        assertFalse(ColorDistance.isClose(white, darkBg))
    }

    @Test
    fun darkBgPixel_onDarkBg_isClose() {
        val darkBg = ColorDistance.rgb(30, 30, 30)
        assertTrue(ColorDistance.isClose(darkBg, darkBg))
    }

    @Test
    fun nearDarkBg_onDarkBg_isClose() {
        val darkBg = ColorDistance.rgb(30, 30, 30)
        val nearBg = ColorDistance.rgb(35, 35, 35)
        assertTrue(ColorDistance.isClose(nearBg, darkBg))
    }

    @Test
    fun material3Surface_onWhite_isClose() {
        val surface = ColorDistance.rgb(253, 253, 246)
        val white = ColorDistance.rgb(255, 255, 255)
        assertTrue(ColorDistance.isClose(surface, white))
    }

    @Test
    fun redOnWhite_isNotClose() {
        val red = ColorDistance.rgb(255, 0, 0)
        val white = ColorDistance.rgb(255, 255, 255)
        assertFalse(ColorDistance.isClose(red, white))
    }

    @Test
    fun tolerance48_boundaryExact() {
        val bg = ColorDistance.rgb(0, 0, 0)
        val atBoundary = ColorDistance.rgb(48, 0, 0)
        assertTrue("Exactly at tolerance boundary should be close", ColorDistance.isClose(atBoundary, bg))
    }

    @Test
    fun tolerance48_justBeyond() {
        val bg = ColorDistance.rgb(0, 0, 0)
        val beyond = ColorDistance.rgb(49, 0, 0)
        assertFalse("Just beyond tolerance should not be close", ColorDistance.isClose(beyond, bg))
    }

    @Test
    fun rgb_componentsRoundTrip() {
        val color = ColorDistance.rgb(128, 64, 32)
        assertEquals(128, ColorDistance.red(color))
        assertEquals(64, ColorDistance.green(color))
        assertEquals(32, ColorDistance.blue(color))
        assertEquals(255, ColorDistance.alpha(color))
    }
}
