package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #641 评论1 第4/5节：[ComposeEditorVisualState] 契约测试。
 *
 * 覆盖：
 * - [onAuthoritativeLayout] 记录布局快照（previous/current）；
 * - [onVisualIntent] 设置 hiddenRanges 和动画类型；
 * - [clearAnimation] 清 hiddenRanges，系统正文马上可见；
 * - drawsVisualCursor 在 Cursor intent 时设为 true，clearAnimation 后恢复 false。
 */
class ComposeEditorVisualStateTest {
    @Test
    fun initial_state_hasNoHiddenRanges() {
        val state = ComposeEditorVisualState()
        assertTrue("初始无 hiddenRanges", state.hiddenRanges.value.isEmpty())
        assertFalse("初始不画视觉光标", state.drawsVisualCursor.value)
        assertNull("初始无活跃 intent", state.activeIntent.value)
    }

    @Test
    fun onVisualIntent_insert_setsHiddenRanges() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                affectedRanges = listOf(TextRange(0, 3)),
                kind = EditorVisualIntent.Kind.Insert,
            ),
        )
        assertEquals(listOf(TextRange(0, 3)), state.hiddenRanges.value)
        assertEquals(EditorVisualIntent.Kind.Insert, state.activeIntent.value?.kind)
    }

    @Test
    fun onVisualIntent_delete_setsHiddenRanges() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                affectedRanges = listOf(TextRange(5, 10)),
                kind = EditorVisualIntent.Kind.Delete,
            ),
        )
        assertEquals(listOf(TextRange(5, 10)), state.hiddenRanges.value)
    }

    @Test
    fun onVisualIntent_cursor_setsDrawsVisualCursor() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                affectedRanges = emptyList(),
                kind = EditorVisualIntent.Kind.Cursor,
            ),
        )
        assertTrue("Cursor intent 设 drawsVisualCursor", state.drawsVisualCursor.value)
    }

    @Test
    fun onVisualIntent_filtersEmptyRanges() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                affectedRanges = listOf(TextRange(0, 0), TextRange(3, 3), TextRange(5, 8)),
                kind = EditorVisualIntent.Kind.Insert,
            ),
        )
        assertEquals("空 range 被过滤", listOf(TextRange(5, 8)), state.hiddenRanges.value)
    }

    @Test
    fun clearAnimation_clearsHiddenRangesAndIntent() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                affectedRanges = listOf(TextRange(0, 3)),
                kind = EditorVisualIntent.Kind.Insert,
            ),
        )
        state.clearAnimation()
        assertTrue("clearAnimation 后无 hiddenRanges", state.hiddenRanges.value.isEmpty())
        assertNull("clearAnimation 后无活跃 intent", state.activeIntent.value)
        assertFalse("clearAnimation 后不画视觉光标", state.drawsVisualCursor.value)
    }

    @Test
    fun clearAnimation_afterCursorIntent_resetsDrawsVisualCursor() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                affectedRanges = emptyList(),
                kind = EditorVisualIntent.Kind.Cursor,
            ),
        )
        assertTrue(state.drawsVisualCursor.value)
        state.clearAnimation()
        assertFalse("clearAnimation 后 drawsVisualCursor 恢复 false", state.drawsVisualCursor.value)
    }

    @Test
    fun currentLayout_initiallyNull() {
        val state = ComposeEditorVisualState()
        assertNull("初始无 currentLayout", state.currentLayout())
        assertNull("初始无 previousLayout", state.previousLayout())
    }
}
