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
        assertTrue(source.contains("if (renderer.isScrolling())"))
        assertTrue(source.contains("AnimationModeData.SystemSuppressed"))
        assertTrue(source.contains("vt.animationMode"))
    }
}
