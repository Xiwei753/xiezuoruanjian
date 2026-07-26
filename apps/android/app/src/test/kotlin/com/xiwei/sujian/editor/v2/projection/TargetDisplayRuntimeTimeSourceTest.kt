package com.xiwei.sujian.editor.v2.projection

import android.text.TextPaint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class TargetDisplayRuntimeTimeSourceTest {

    @Test
    fun targetDisplayRuntimeAcceptsManualTimeSource() {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        val paint = TextPaint()
        paint.textSize = 40f
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = TargetDisplayRuntime(mirror, paint, manualTimeSource, transactionIdSource)
        assertNotNull(runtime)
    }

    @Test
    fun targetDisplayRuntimeDefaultConstructorStillWorks() {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello", 5)
        val paint = TextPaint()
        paint.textSize = 40f
        val runtime = TargetDisplayRuntime(mirror, paint)
        assertNotNull(runtime)
    }

    @Test
    fun manualTimeSourceControlsDrawFrameTime() {
        val manualTimeSource = ManualAnimationTimeSource()
        manualTimeSource.advanceByMs(32)
        assertEquals(32_000_000L, manualTimeSource.nowNanos())
    }
}
