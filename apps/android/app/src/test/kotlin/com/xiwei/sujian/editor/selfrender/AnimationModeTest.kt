package com.xiwei.sujian.editor.selfrender

import org.junit.Assert.*
import org.junit.Test
import java.io.File

class AnimationModeTest {
    private fun controllerSource(): String {
        val path = "src/main/kotlin/com/xiwei/sujian/editor/selfrender/SujianAnimationController.kt"
        val file = File(path)
        assertTrue("SujianAnimationController source should exist at $path", file.exists())
        return file.readText()
    }

    private fun rendererSource(): String {
        val path = "src/main/kotlin/com/xiwei/sujian/editor/selfrender/SujianEditorRenderer.kt"
        val file = File(path)
        assertTrue("SujianEditorRenderer source should exist at $path", file.exists())
        return file.readText()
    }

    @Test
    fun productionControllerDoesNotRecomputeAnimationModeLocally() {
        val source = controllerSource()
        assertFalse("Android must not keep a local chooseAnimationMode route", source.contains("fun chooseAnimationMode("))
        assertFalse("Android must not choose mode from glyphRects.size", source.contains("clusterCount = glyphRects.size"))
        assertTrue("Android must consume Core animationMode", source.contains("vt.animationMode"))
    }

    @Test
    fun systemSuppressionIsOnlyADowngradeAfterCoreMode() {
        val source = controllerSource()
        assertTrue(source.contains("AnimationModeData.SystemSuppressed"))
        assertTrue(source.contains("vt.animationMode"))
    }

    @Test
    fun snapshotModeIsSkippedUntilRealRendererExists() {
        val source = controllerSource()
        assertTrue(source.contains("decision == AnimationModeData.SystemSuppressed || decision == AnimationModeData.SnapshotAnimation"))
        assertTrue(source.contains("renderer.clearAnimations()"))
        assertFalse("Android must not create placeholder snapshot overlays", source.contains("AnimationModeData.SnapshotAnimation -> \"snapshot\""))
    }

    @Test
    fun imeComposingDoesNotClearAnimations() {
        val source = File("src/main/kotlin/com/xiwei/sujian/editor/selfrender/SujianImeController.kt").readText()
        assertFalse("IME composing must not clearAnimations, should pauseAll instead",
            source.contains("renderer.clearAnimations()"))
        assertTrue("IME composing should pauseAll", source.contains("renderer.pauseAll()"))
    }

    @Test
    fun coordinatedCursorOnlyStartsAfterTextAnimationStarted() {
        val source = controllerSource()
        assertTrue(source.contains("TextAnimationStartResult.Started"))
        assertTrue(source.contains("textAnimationResult == TextAnimationStartResult.Started"))
        assertTrue(source.contains("handleInsertTransaction(vt)"))
        assertTrue(source.contains("handleDeleteTransaction(vt)"))
    }

    @Test
    fun rendererUsesPlatformVisualTransactionModel() {
        val source = rendererSource()
        assertTrue("Renderer must use AndroidPlatformVisualTransaction", source.contains("AndroidPlatformVisualTransaction"))
        assertTrue("Renderer must use AndroidAnimatedSlice", source.contains("AndroidAnimatedSlice"))
        assertTrue("Renderer must use AndroidStaticLinePatch", source.contains("AndroidStaticLinePatch"))
    }

    @Test
    fun rendererDoesNotUsePerCharGhostModel() {
        val source = rendererSource()
        assertFalse("Renderer must not contain SujianOverlayAnim", source.contains("SujianOverlayAnim"))
        assertFalse("Renderer must not use activeInsertRanges", source.contains("activeInsertRanges"))
        assertFalse("Renderer must not draw per-char text in animations", source.contains("drawText(glyphRect.char"))
    }

    @Test
    fun insertDeleteOnlyConsumeVtAnimationMode() {
        val source = controllerSource()
        assertTrue("Insert must consume vt.animationMode", source.contains("vt.animationMode"))
        assertFalse("Insert must not recompute mode locally", source.contains("fun chooseAnimationMode("))
    }
}
