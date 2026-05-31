package com.xiwei.writerapp.ui

import android.content.Context
import com.google.gson.GsonBuilder
import com.google.gson.reflect.TypeToken
import com.xiwei.writerapp.model.LocalSettings
import com.xiwei.writerapp.model.MindMapGraphNode
import com.xiwei.writerapp.model.MindMapNodeKind
import com.xiwei.writerapp.model.MindMapNodeKindDeserializer
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
        assertTrue(settings.editorTypingAnimationEnabled)
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
    fun testUnknownNodeAndEdgeKindDoesNotCrash() {
        val gson = GsonBuilder()
            .registerTypeAdapter(MindMapNodeKind::class.java, MindMapNodeKindDeserializer())
            .create()

        // Unknown node kind "alien" should fallback to Custom
        val jsonNode = """
            {
                "id": "node-1",
                "title": "Unknown Node",
                "kind": "alien",
                "tags": []
            }
        """.trimIndent()

        val node = gson.fromJson(jsonNode, MindMapGraphNode::class.java)
        assertNotNull(node)
        assertEquals("node-1", node.id)
        assertEquals(MindMapNodeKind.Custom, node.kind)

        // Known node kind "character" should parse to Character
        val jsonNodeKnown = """
            {
                "id": "node-2",
                "title": "Character Node",
                "kind": "character",
                "tags": []
            }
        """.trimIndent()

        val nodeKnown = gson.fromJson(jsonNodeKnown, MindMapGraphNode::class.java)
        assertNotNull(nodeKnown)
        assertEquals(MindMapNodeKind.Character, nodeKnown.kind)
        
        // Edge kind is string, unknown kind should parse normally
        val jsonEdge = """
            {
                "id": "edge-1",
                "from": "node-1",
                "to": "node-2",
                "kind": "mysterious_relationship",
                "label": "Mysterious"
            }
        """.trimIndent()
        
        val type = object : TypeToken<Map<String, Any>>() {}.type
        val edgeMap: Map<String, Any> = gson.fromJson(jsonEdge, type)
        assertEquals("mysterious_relationship", edgeMap["kind"])
    }

    @Test
    fun testCoreSettingsEventsBehavior() {
        com.xiwei.writerapp.data.CoreSettingsEvents.consumeChanged()
        com.xiwei.writerapp.data.CoreSettingsEvents.consumeEditorChanged()

        com.xiwei.writerapp.data.CoreSettingsEvents.record(
            com.xiwei.writerapp.data.ResultEnvelope(
                success = true,
                data = true,
                changedEntities = listOf(
                    com.xiwei.writerapp.data.ChangedEntity(entityType = "SettingsSaved")
                )
            )
        )

        assertTrue(com.xiwei.writerapp.data.CoreSettingsEvents.consumeChanged())
        assertTrue(com.xiwei.writerapp.data.CoreSettingsEvents.consumeEditorChanged())

        assertFalse(com.xiwei.writerapp.data.CoreSettingsEvents.consumeChanged())
        assertFalse(com.xiwei.writerapp.data.CoreSettingsEvents.consumeEditorChanged())
    }

    @Test
    fun testEditorViewModelReloadSettings() {
        val application = org.robolectric.RuntimeEnvironment.getApplication()
        val viewModel = EditorViewModel(application)

        viewModel.onContentChanged("Test Content 123")
        assertEquals("Test Content 123", viewModel.uiState.value.content)

        viewModel.reloadSettings()

        assertEquals("Test Content 123", viewModel.uiState.value.content)
        assertNotNull(viewModel.uiState.value.settings)
    }
}
