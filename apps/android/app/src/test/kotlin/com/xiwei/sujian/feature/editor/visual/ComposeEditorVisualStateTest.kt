package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #641 评论1 第4/5节 / 问题2 / 问题3：[ComposeEditorVisualState] 契约测试。
 *
 * 覆盖：
 * - [onAuthoritativeLayout] 记录布局快照（previous/current）；
 * - [onVisualIntent] 设置 hiddenRanges、动画类型、cursor 和 transaction；
 * - [clearAnimation] 清 hiddenRanges，系统正文马上可见；
 * - drawsVisualCursor 在 cursor animate=true 时设为 true，clearAnimation 后恢复 false；
 * - #641 评论 问题2：cursor?.animate == true 时画视觉光标，不管 textKind。
 */
class ComposeEditorVisualStateTest {
    @Test
    fun initial_state_hasNoHiddenRanges() {
        val state = ComposeEditorVisualState()
        assertTrue("初始无 hiddenRanges", state.hiddenRanges.value.isEmpty())
        assertFalse("初始不画视觉光标", state.drawsVisualCursor.value)
        assertNull("初始无活跃 intent", state.activeIntent.value)
        assertNull("初始无活跃 transaction", state.activeTransaction.value)
    }

    @Test
    fun onVisualIntent_insert_setsHiddenRanges() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        assertEquals(listOf(TextRange(0, 3)), state.hiddenRanges.value)
        assertEquals(TextVisualKind.Insert, state.activeIntent.value?.textKind)
    }

    @Test
    fun onVisualIntent_delete_doesNotHideOldRanges() {
        // #641 评论 5457777142 问题3:Delete 不把 deleted oldRange 放进 hiddenRanges —
        // OutputTransformation 作用的是新正文,oldRange 在新正文里可能指向其他字符。
        // 删除字符只从 previous TextLayoutResult 由 overlay 画旧字离场。
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                oldRanges = listOf(TextRange(5, 10)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        assertTrue("Delete 不隐藏 oldRanges", state.hiddenRanges.value.isEmpty())
    }

    @Test
    fun onVisualIntent_cursorAnimate_setsDrawsVisualCursor() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                oldRanges = emptyList(),
                newRanges = emptyList(),
                textKind = TextVisualKind.None,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 1, animate = true),
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )
        assertTrue("cursor animate=true 设 drawsVisualCursor", state.drawsVisualCursor.value)
    }

    @Test
    fun onVisualIntent_insertWithCursorAnimate_setsDrawsVisualCursor() {
        // #641 评论 问题2：Insert + cursor animate=true 也应画视觉光标。
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 1, animate = true),
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        assertTrue("Insert + cursor animate=true 设 drawsVisualCursor", state.drawsVisualCursor.value)
    }

    @Test
    fun onVisualIntent_filtersEmptyRanges() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 0), TextRange(3, 3), TextRange(5, 8)),
                textKind = TextVisualKind.Insert,
                cursor = null,
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        assertEquals("空 range 被过滤", listOf(TextRange(5, 8)), state.hiddenRanges.value)
    }

    @Test
    fun onVisualIntent_createsTransactionWithIncrementingId() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        val firstId = state.activeTransaction.value?.id
        assertEquals("首个事务 ID 为 1", 1L, firstId)
        assertEquals("事务 motionPolicy 正确", 100L, state.activeTransaction.value?.motionPolicy?.textDurationMillis)

        state.onVisualIntent(
            EditorVisualIntent(
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 5)),
                textKind = TextVisualKind.Insert,
                cursor = null,
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 200L),
        )
        val secondId = state.activeTransaction.value?.id
        assertEquals("第二个事务 ID 为 2", 2L, secondId)
        assertEquals("事务 motionPolicy 更新", 200L, state.activeTransaction.value?.motionPolicy?.textDurationMillis)
    }

    @Test
    fun clearAnimation_clearsHiddenRangesAndIntent() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.clearAnimation()
        assertTrue("clearAnimation 后无 hiddenRanges", state.hiddenRanges.value.isEmpty())
        assertNull("clearAnimation 后无活跃 intent", state.activeIntent.value)
        assertFalse("clearAnimation 后不画视觉光标", state.drawsVisualCursor.value)
        assertNull("clearAnimation 后无活跃 transaction", state.activeTransaction.value)
    }

    @Test
    fun clearAnimation_afterCursorIntent_resetsDrawsVisualCursor() {
        val state = ComposeEditorVisualState()
        state.onVisualIntent(
            EditorVisualIntent(
                oldRanges = emptyList(),
                newRanges = emptyList(),
                textKind = TextVisualKind.None,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 1, animate = true),
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
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
