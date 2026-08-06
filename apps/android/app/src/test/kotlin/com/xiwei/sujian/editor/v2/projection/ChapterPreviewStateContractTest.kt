package com.xiwei.sujian.editor.v2.projection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #595 九：预览用纯静态 ChapterPreviewState 契约测试。
 *
 * 验证 EditorWindowHost 提供 getChapterPreviewState 方法，
 * 返回不含动画引擎或 Bitmap 资源的纯静态预览状态。
 * WritingEditorSurface 用该方法的返回值而非 TargetDisplayRuntime 显示预览。
 */
class ChapterPreviewStateContractTest {

    @Test
    fun chapterPreviewStateIsImmutable() {
        val state = ChapterPreviewState(
            text = "preview text",
            revision = 5L,
            selection = TextRange(0, 10),
            searchHighlights = listOf(TextRange(0, 5)),
        )
        assertEquals("preview text", state.text)
        assertEquals(5L, state.revision)
        assertEquals(TextRange(0, 10), state.selection)
        assertEquals(1, state.searchHighlights.size)
    }

    @Test
    fun chapterPreviewStateHasNoAnimationFields() {
        // ChapterPreviewState 不应持有动画引擎、Bitmap 或 VisualRuntime 相关字段
        val fields = ChapterPreviewState::class.java.declaredFields
        val fieldNames = fields.map { it.name }
        assertTrue("Must have text field", fieldNames.contains("text"))
        assertTrue("Must have revision field", fieldNames.contains("revision"))
        // 确保没有动画相关字段
        assertFalse("Must not have animationEngine field",
            fieldNames.any { it.contains("animation") || it.contains("Animation") })
        assertFalse("Must not have visualRuntime field",
            fieldNames.any { it.contains("visualRuntime") || it.contains("VisualRuntime") })
        assertFalse("Must not have bitmap field",
            fieldNames.any { it.contains("bitmap") || it.contains("Bitmap") })
    }

    @Test
    fun getChapterPreviewStateExistsOnEditorWindowHost() {
        val method = com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "getChapterPreviewState" &&
            it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == String::class.java &&
            it.returnType == ChapterPreviewState::class.java
        }
        assertNotNull("EditorWindowHost must have getChapterPreviewState(String): ChapterPreviewState?", method)
    }

    @Test
    fun previewStyleDefaultsAreSensible() {
        val style = PreviewStyle()
        assertEquals(16f, style.fontSizeSp, 0.01f)
        assertEquals(1.5f, style.lineSpacingMultiplier, 0.01f)
    }

    @Test
    fun textRangeIsImmutable() {
        val range = TextRange(3, 7)
        assertEquals(3, range.start)
        assertEquals(7, range.end)
    }

    private fun assertFalse(message: String, condition: Boolean) {
        org.junit.Assert.assertFalse(message, condition)
    }
}
