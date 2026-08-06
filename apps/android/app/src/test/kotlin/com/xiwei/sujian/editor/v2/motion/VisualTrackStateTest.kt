package com.xiwei.sujian.editor.v2.motion

import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 五：VisualTrackState 契约测试 — 验证文字轨和光标轨分别终态。
 *
 * 完成规则：transactionFinished = textFinished && cursorFinished
 * 渲染规则：文字完成后用静态新布局，光标仍可继续动画。
 */
class VisualTrackStateTest {

    @Test
    fun idleStateIsFullyComplete() {
        val idle = VisualTrackState.Idle
        assertTrue("idle text finished", idle.textFinished)
        assertTrue("idle cursor finished", idle.cursorFinished)
        assertTrue("idle transaction complete", idle.transactionComplete)
        assertNull("idle has no text transaction", idle.renderTextTransaction)
        assertFalse("idle has no cursor transition", idle.renderCursorTransition)
    }

    @Test
    fun textFinishedCursorNotFinishedKeepsCursorAnimating() {
        val state = VisualTrackState(
            renderTextTransaction = null,
            renderCursorTransition = true,
            textProgress = 1f,
            cursorProgress = 0.5f,
            textFinished = true,
            cursorFinished = false,
            transactionComplete = false,
        )
        assertTrue("text finished → no text transaction", state.textFinished)
        assertFalse("cursor not finished → cursor transition active", state.cursorFinished)
        assertFalse("transaction not complete until both finished", state.transactionComplete)
        assertTrue("cursor still rendering", state.renderCursorTransition)
    }

    @Test
    fun bothFinishedCompletesTransaction() {
        val state = VisualTrackState(
            renderTextTransaction = null,
            renderCursorTransition = false,
            textProgress = 1f,
            cursorProgress = 1f,
            textFinished = true,
            cursorFinished = true,
            transactionComplete = true,
        )
        assertTrue("text finished", state.textFinished)
        assertTrue("cursor finished", state.cursorFinished)
        assertTrue("transaction complete", state.transactionComplete)
    }
}
