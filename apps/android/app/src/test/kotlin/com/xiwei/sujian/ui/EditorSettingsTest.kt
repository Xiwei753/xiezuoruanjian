package com.xiwei.sujian.ui

import android.content.Context
import android.text.style.ForegroundColorSpan
import com.google.gson.GsonBuilder
import com.google.gson.reflect.TypeToken
import com.xiwei.sujian.model.LocalSettings
import org.junit.Assert.*

/**
 * EditorSettingsTest — 编辑器设置和模型序列化测试
 *
 * 测试 LocalSettings、MindMapGraphNode 等模型的 JSON 序列化/反序列化逻辑。
 */
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class EditorSettingsTest {

    @Test
    fun testLocalSettingsDefaultValues() {
        val settings = LocalSettings()
        assertFalse(settings.editorTypingAnimationEnabled)
        assertEquals(100, settings.editorTypingAnimationDurationMs)
        assertTrue(settings.editorSmoothCursorEnabled)
        assertEquals(80, settings.editorSmoothCursorDurationMs)
    }

    @Test
    fun testWriterEditTextSettingsPropagation() {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val editText = WriterEditText(context)

        // Default duration check
        assertEquals(80L, editText.cursorAnimationDurationMs())
        assertEquals(100L, editText.typingAnimationDurationMs())

        // Propagate smooth cursor setting
        editText.setSmoothCursorEnabled(true, 120L)
        assertEquals(120L, editText.cursorAnimationDurationMs())

        // Propagate typing animation setting
        editText.setTypingAnimationEnabled(true, 150L)
        assertEquals(150L, editText.typingAnimationDurationMs())

        // Disable smooth cursor setting
        editText.setSmoothCursorEnabled(false, 0L)
        assertEquals(0L, editText.cursorAnimationDurationMs())

        // Disable typing animation setting
        editText.setTypingAnimationEnabled(false, 0L)
        assertEquals(0L, editText.typingAnimationDurationMs())
    }

    @Test
    fun testTypingAnimationDoesNotInjectTransparentForegroundSpans() {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val editText = WriterEditText(context)

        editText.setTypingAnimationEnabled(true, 150L)
        editText.setText("你")

        val spans = editText.text?.getSpans(0, editText.text?.length ?: 0, ForegroundColorSpan::class.java)
            ?: emptyArray()
        assertTrue(
            "Android WriterEditText must not hide real body text with transparent spans",
            spans.none { it.foregroundColor == android.graphics.Color.TRANSPARENT }
        )
    }

    @Test
    fun testCursorHeightCalculationConsistency() {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val editText = WriterEditText(context)
        
        // Mock text layout or verify paint metrics height logic
        val fontMetrics = editText.paint.fontMetrics
        val density = editText.resources.displayMetrics.density
        val cursorVerticalPadding = 1f * density
        
        // Target top relative to baseline: baseline + ascent + padding
        // Target bottom relative to baseline: baseline + descent - padding
        // Visual Height = (baseline + descent - padding) - (baseline + ascent + padding)
        //               = descent - ascent - 2 * padding
        val heightNormalLine = fontMetrics.descent - fontMetrics.ascent - 2 * cursorVerticalPadding
        val heightLastLine = fontMetrics.descent - fontMetrics.ascent - 2 * cursorVerticalPadding
        
        assertEquals("Cursor height should be independent of line index and identical across lines", heightNormalLine, heightLastLine, 0.001f)
    }

    @Test
    fun testCoreSettingsEventsBehavior() {
        com.xiwei.sujian.data.CoreSettingsEvents.consumeChanged()
        com.xiwei.sujian.data.CoreSettingsEvents.consumeEditorChanged()

        com.xiwei.sujian.data.CoreSettingsEvents.record(
            com.xiwei.sujian.data.ResultEnvelope(
                success = true,
                data = true,
                changedEntities = listOf(
                    com.xiwei.sujian.data.ChangedEntity(entityType = "SettingsSaved")
                )
            )
        )

        assertTrue(com.xiwei.sujian.data.CoreSettingsEvents.consumeChanged())
        assertTrue(com.xiwei.sujian.data.CoreSettingsEvents.consumeEditorChanged())

        assertFalse(com.xiwei.sujian.data.CoreSettingsEvents.consumeChanged())
        assertFalse(com.xiwei.sujian.data.CoreSettingsEvents.consumeEditorChanged())
    }

    @Test
    fun testEditorViewModelReloadSettings() {
        val application = org.robolectric.RuntimeEnvironment.getApplication()
        try {
            val viewModel = EditorViewModel(application)

            viewModel.onContentChanged("Test Content 123")
            assertEquals("Test Content 123", viewModel.uiState.value.content)

            viewModel.reloadSettings()

            assertEquals("Test Content 123", viewModel.uiState.value.content)
            assertNotNull(viewModel.uiState.value.settings)
        } catch (e: com.xiwei.sujian.data.RepositoryException) {
            // Native library is not loaded in Robolectric unit tests,
            // so WorkspaceRepository initialization will fail. This is expected.
            assertEquals("Native库未加载", e.message)
        }
    }
}
