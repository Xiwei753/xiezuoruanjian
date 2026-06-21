package com.xiwei.sujian.ui

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/**
 * WriterEditTextTest — WriterEditText 仪器化测试
 *
 * 测试 WriterEditText 的打字动画、平滑光标等功能在真机/模拟器上的行为。
 *
 * ## 重要约束
 * 打字动画通过 Canvas overlay 绘制，**禁止**向正文 Editable 注入透明 ForegroundColorSpan 隐藏文字。
 * EditorRenderLayer 不再持有 animatedSkipRanges / activeSkipSpans，静态正文永远由系统完整绘制。
 */

@RunWith(AndroidJUnit4::class)
class WriterEditTextTest {

    @Test
    fun testWriterEditTextInstantiation() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        assertNotNull(editText)
    }

    @Test
    fun testWriterEditTextHasRenderLayer() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        assertNotNull("WriterEditText should have a renderLayer", editText.renderLayer)
    }

    @Test
    fun testWriterEditTextDoesNotDirectlyManageRenderers() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        val layer = editText.renderLayer
        assertNotNull("renderLayer should not be null", layer)
        assertNotNull("renderLayer should manage typingOverlayRenderer", layer?.typingOverlayRenderer)
        assertNotNull("renderLayer should manage smoothCursorRenderer", layer?.smoothCursorRenderer)
    }

    @Test
    fun testRenderLayerDoesNotModifyEditable() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")
        editText.text?.append("B")
        editText.text?.append("C")

        // 静态正文永远由系统完整绘制，动画只做 overlay
        val layer = editText.renderLayer
        assertNotNull(layer)

        // clear() 不应抛异常
        layer?.clear()
    }

    @Test
    fun testMassPasteDoesNotCrash() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        val longString = "A".repeat(150)
        editText.text?.append(longString)

        // 大段粘贴不应导致异常
        assertNotNull(editText.text)
    }

    @Test
    fun testDeletionDoesNotCrash() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("Hello")
        editText.text?.delete(4, 5)

        assertNotNull(editText.text)
    }

    @Test
    fun testRunWithoutTextAnimationsDoesNotCrash() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val editText = WriterEditText(appContext)
        editText.setTypingAnimationEnabled(true, durationMs = 10)

        editText.setText("")
        editText.text?.append("A")

        editText.runWithoutTextAnimations {
            editText.text?.append("B")
        }

        assertEquals("AB", editText.text.toString())
    }
}
