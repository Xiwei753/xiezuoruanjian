package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.*
import org.junit.Test

class CaptureMethodTest {

    @Test
    fun captureMethod_hasTwoValues() {
        assertEquals(2, CaptureMethod.entries.size)
    }

    @Test
    fun pixelCopyResult_hasFourValues() {
        assertEquals(4, PixelCopyResult.entries.size)
    }

    @Test
    fun captureMethod_pixelCopyAndSoftwareDraw_areDistinct() {
        assertNotEquals(CaptureMethod.PIXEL_COPY, CaptureMethod.SOFTWARE_DRAW)
    }

    @Test
    fun pixelCopyResult_allDistinct() {
        val values = PixelCopyResult.entries
        assertEquals(values.size, values.toSet().size)
    }

    @Test
    fun colorDistance_material3DarkSurface_onDarkBg_isClose() {
        val darkSurface = ColorDistance.rgb(30, 30, 30)
        val darkBg = ColorDistance.rgb(28, 28, 28)
        assertTrue("Material 3 dark surface should be close to dark background", ColorDistance.isClose(darkSurface, darkBg))
    }

    @Test
    fun colorDistance_dynamicColorSurface_onLightBg_isClose() {
        val dynamicSurface = ColorDistance.rgb(251, 251, 243)
        val lightBg = ColorDistance.rgb(255, 255, 255)
        assertTrue("Dynamic color surface should be close to light background", ColorDistance.isClose(dynamicSurface, lightBg))
    }

    @Test
    fun colorDistance_textOnBackground_isNotClose() {
        val textPixel = ColorDistance.rgb(28, 27, 31)
        val lightBg = ColorDistance.rgb(255, 255, 255)
        assertFalse("Text pixel should not be close to light background", ColorDistance.isClose(textPixel, lightBg))
    }
}
