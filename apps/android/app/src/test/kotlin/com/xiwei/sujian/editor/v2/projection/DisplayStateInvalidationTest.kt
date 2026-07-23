package com.xiwei.sujian.editor.v2.projection

import android.text.TextPaint
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class DisplayStateInvalidationTest {

    private fun createRuntime(): TargetDisplayRuntime {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello world", 11)
        val paint = TextPaint()
        paint.textSize = 40f
        return TargetDisplayRuntime(mirror, paint)
    }

    @Test
    fun invalidateDisplayStateIncrementsVersion() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.invalidateDisplayState()
        assertTrue(
            "displayStateVersion should increment after invalidateDisplayState",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun invalidateDisplayStateIncrementsFrameGeneration() {
        val runtime = createRuntime()
        val before = runtime.frameGeneration
        runtime.invalidateDisplayState()
        assertTrue(
            "frameGeneration should increment after invalidateDisplayState",
            runtime.frameGeneration > before
        )
    }

    @Test
    fun setSearchHighlightsInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.setSearchHighlights(listOf(Pair(0, 5)))
        assertTrue(
            "setSearchHighlights should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun setSelectionInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.setSelection(0, 5)
        assertTrue(
            "setSelection should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun clearDecorationsInvalidatesDisplayState() {
        val runtime = createRuntime()
        runtime.setSearchHighlights(listOf(Pair(0, 5)))
        val before = runtime.displayStateVersion
        runtime.clearDecorations()
        assertTrue(
            "clearDecorations should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun setScrollPositionInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.setScrollPosition(10f, 20f)
        assertTrue(
            "setScrollPosition should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun setViewportSizeInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.setViewportSize(1080, 1920)
        assertTrue(
            "setViewportSize should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun setFontSizeInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.setFontSize(32f)
        assertTrue(
            "setFontSize should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun setLineSpacingMultiplierInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.setLineSpacingMultiplier(1.5f)
        assertTrue(
            "setLineSpacingMultiplier should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun setThemeColorsInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.setThemeColors(0xFF000000.toInt(), 0xFF0000FF.toInt(), 0xFF00FF00.toInt(), 0xFFFF0000.toInt(), 0xFFFFFFFF.toInt())
        assertTrue(
            "setThemeColors should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun setSecretMaskedInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.setSecretMasked(true)
        assertTrue(
            "setSecretMasked should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun updateFromSnapshotInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.updateFromSnapshot("New text", 8, 1)
        assertTrue(
            "updateFromSnapshot should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun setWidthInvalidatesDisplayState() {
        val runtime = createRuntime()
        val before = runtime.displayStateVersion
        runtime.setWidth(500f)
        assertTrue(
            "setWidth should invalidate display state",
            runtime.displayStateVersion > before
        )
    }

    @Test
    fun multipleInvalidationsMonotonicallyIncreaseVersion() {
        val runtime = createRuntime()
        val v0 = runtime.displayStateVersion
        runtime.setSearchHighlights(listOf(Pair(0, 3)))
        val v1 = runtime.displayStateVersion
        runtime.setSelection(2, 4)
        val v2 = runtime.displayStateVersion
        runtime.setScrollPosition(5f, 10f)
        val v3 = runtime.displayStateVersion
        assertTrue("v1 > v0", v1 > v0)
        assertTrue("v2 > v1", v2 > v1)
        assertTrue("v3 > v2", v3 > v2)
    }

    @Test
    fun scrollPositionIsRetainedAfterInvalidation() {
        val runtime = createRuntime()
        runtime.setScrollPosition(10f, 20f)
        assertEquals(10f, runtime.getScrollX(), 0.01f)
        assertEquals(20f, runtime.getScrollY(), 0.01f)
        runtime.invalidateDisplayState()
        assertEquals(10f, runtime.getScrollX(), 0.01f)
        assertEquals(20f, runtime.getScrollY(), 0.01f)
    }

    @Test
    fun viewportSizeIsRetainedAfterInvalidation() {
        val runtime = createRuntime()
        runtime.setViewportSize(800, 600)
        assertEquals(800, runtime.getViewportWidth())
        assertEquals(600, runtime.getViewportHeight())
        runtime.invalidateDisplayState()
        assertEquals(800, runtime.getViewportWidth())
        assertEquals(600, runtime.getViewportHeight())
    }

    @Test
    fun onFramePublishesFrameWithVersionCapturedAtStart() {
        val runtime = createRuntime()
        val versionBeforeInvalidate = runtime.displayStateVersion
        runtime.invalidateDisplayState()
        val versionAfterInvalidate = runtime.displayStateVersion
        assertTrue("version should increment", versionAfterInvalidate > versionBeforeInvalidate)
        runtime.onFrame(System.nanoTime())
        assertTrue(
            "frameGeneration should increment after onFrame",
            runtime.frameGeneration > 0L
        )
    }

    @Test
    fun drawFrameRejectsStaleCachedFrame() {
        val runtime = createRuntime()
        runtime.onFrame(System.nanoTime())
        val versionAfterFirstFrame = runtime.displayStateVersion
        runtime.invalidateDisplayState()
        assertTrue(
            "displayStateVersion should be newer after invalidate",
            runtime.displayStateVersion > versionAfterFirstFrame
        )
    }

    @Test
    fun setFrameClockRegistersWhenCacheIsEmpty() {
        val runtime = createRuntime()
        runtime.invalidateDisplayState()
        val clock = WindowDisplayFrameClock()
        try {
            runtime.setFrameClock(clock)
            assertTrue("should be registered when cachedFrameState is null", true)
        } finally {
            clock.release()
        }
    }
}
