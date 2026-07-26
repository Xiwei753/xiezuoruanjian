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

    @Test
    fun colorDistance_material3DarkTheme_surfaceNotWhite_isNotClose() {
        val darkSurface = ColorDistance.rgb(30, 30, 30)
        val whiteBg = ColorDistance.rgb(255, 255, 255)
        assertFalse("Dark theme surface should not be close to white background", ColorDistance.isClose(darkSurface, whiteBg))
    }

    @Test
    fun colorDistance_monetDynamicColor_tonalElevation_isClose() {
        val baseSurface = ColorDistance.rgb(251, 252, 243)
        val elevatedSurface = ColorDistance.rgb(253, 253, 246)
        assertTrue("Monet tonal elevation should be close to base surface", ColorDistance.isClose(baseSurface, elevatedSurface))
    }

    @Test
    fun manualAnimationTimeSource_advanceToProgress_exactPercentages() {
        val source = ManualAnimationTimeSource()
        val durationMs = 200L
        val startTimeMs = 16L

        source.advanceToProgress(0f, durationMs, startTimeMs)
        assertEquals("0% should be at startTime", startTimeMs * 1_000_000L, source.nowNanos())

        source.advanceToProgress(0.25f, durationMs, startTimeMs)
        assertEquals("25% should be at startTime + 50ms", (startTimeMs + 50L) * 1_000_000L, source.nowNanos())

        source.advanceToProgress(0.5f, durationMs, startTimeMs)
        assertEquals("50% should be at startTime + 100ms", (startTimeMs + 100L) * 1_000_000L, source.nowNanos())

        source.advanceToProgress(0.75f, durationMs, startTimeMs)
        assertEquals("75% should be at startTime + 150ms", (startTimeMs + 150L) * 1_000_000L, source.nowNanos())

        source.advanceToProgress(1f, durationMs, startTimeMs)
        assertEquals("100% should be at startTime + 200ms", (startTimeMs + 200L) * 1_000_000L, source.nowNanos())
    }

    @Test
    fun manualAnimationTimeSource_advanceToProgress_differentDurations() {
        val source = ManualAnimationTimeSource()
        val durationMs = 300L
        val startTimeMs = 0L

        source.advanceToProgress(0.5f, durationMs, startTimeMs)
        assertEquals("50% of 300ms should be 150ms", 150L * 1_000_000L, source.nowNanos())
    }

    @Test
    fun colorDistance_backgroundTolerance_coversTypicalThemeVariation() {
        val white = ColorDistance.rgb(255, 255, 255)
        val surfaceWithTint = ColorDistance.rgb(252, 252, 245)
        assertTrue("Typical Material 3 surface tint should be within tolerance of white", ColorDistance.isClose(white, surfaceWithTint))
    }

    @Test
    fun sampleBackgroundColor_uniformBackground_returnsBackground() {
        val corners = intArrayOf(
            ColorDistance.rgb(250, 250, 243),
            ColorDistance.rgb(250, 250, 243),
            ColorDistance.rgb(250, 250, 243),
            ColorDistance.rgb(250, 250, 243)
        )
        val grouped = corners.toList().groupBy { it }
        val mostCommon = grouped.maxByOrNull { it.value.size }!!.key
        assertEquals("Uniform corners should return the same color", ColorDistance.rgb(250, 250, 243), mostCommon)
    }

    @Test
    fun sampleBackgroundColor_mostlyBackgroundWithOneContent_returnsBackground() {
        val bgColor = ColorDistance.rgb(250, 250, 243)
        val contentColor = ColorDistance.rgb(28, 27, 31)
        val corners = intArrayOf(bgColor, bgColor, bgColor, contentColor)
        val grouped = corners.toList().groupBy { it }
        val mostCommon = grouped.maxByOrNull { it.value.size }!!.key
        assertEquals("3 background + 1 content should return background", bgColor, mostCommon)
    }

    @Test
    fun sampleBackgroundColor_darkThemeBackground_returnsDarkBackground() {
        val darkBg = ColorDistance.rgb(30, 30, 30)
        val corners = intArrayOf(darkBg, darkBg, darkBg, darkBg)
        val grouped = corners.toList().groupBy { it }
        val mostCommon = grouped.maxByOrNull { it.value.size }!!.key
        assertEquals("Dark theme corners should return dark background", darkBg, mostCommon)
    }
}
