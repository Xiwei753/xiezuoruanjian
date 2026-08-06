package com.xiwei.sujian.editor.v2.visual

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #595 七：文字轨与光标轨都必须进入终态契约测试。
 *
 * 旧缺陷：completeTransaction 只 complete 文字 timeline，cursorTimeline 保持
 * Rendering/Completed 之外的状态，hasActiveAnimation() 永远返回 true，
 * FrameClock 无限 repost 造成无意义耗电；同时文字轨结束后较长光标轨
 * （非协同模式）无法继续渲染。
 *
 * 本测试验证：两条 timeline 都结束后 hasActiveAnimation() 必须返回 false，
 * 事务完成语义是“文字轨 + 光标轨”的合取（#595 六）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CursorTimelineTerminalContractTest {

    private fun createEngine(): AndroidTextAnimationEngine {
        return AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            ChoreographerAnimationTimeSource(),
            TransactionIdSource(),
        )
    }

    private fun cursorOnlyTransaction(transactionId: Long, durationMs: Long): PreparedVisualTransaction {
        return PreparedVisualTransaction(
            transactionId = transactionId,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = emptyList(),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = PreparedVisualTransaction.CursorTransition(
                fromX = 0f, fromY = 0f, fromHeight = 20f,
                toX = 100f, toY = 0f, toHeight = 20f,
                shouldAnimate = true,
            ),
            durationMs = durationMs,
        )
    }

    @Test
    fun completeIfFinished_completesBothTimelines_hasActiveAnimationFalse() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        engine.setCoordinatedAnimationEnabled(false)
        // 文字轨 100ms、光标轨 80ms（非协同，光标时长不受文字时长限制）。
        engine.submit(cursorOnlyTransaction(transactionId = 1L, durationMs = 100L), submittedAtMs = 0L)
        assertTrue("Active animation before completion", engine.hasActiveAnimation())

        // 200ms 时两条轨道都已结束：completeIfFinished 必须把两条 timeline
        // 都置为 Completed，否则 hasActiveAnimation 永久 true（VSync 死循环）。
        val completed = engine.completeIfFinished(200L)
        assertTrue("Transaction must complete when both tracks finished", completed)
        assertFalse(
            "After both timelines completed, hasActiveAnimation must be false — " +
            "otherwise FrameClock reposts forever (#595 七)",
            engine.hasActiveAnimation(),
        )
    }

    @Test
    fun completeIfFinished_textTrackFinishedOnly_keepsAnimationActive() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 200L)
        engine.setCoordinatedAnimationEnabled(false)
        // 文字轨 100ms、光标轨 200ms（非协同：文字结束后光标继续走完自己的时长）。
        engine.submit(cursorOnlyTransaction(transactionId = 2L, durationMs = 100L), submittedAtMs = 0L)

        // 150ms：文字轨已结束（100ms），光标轨未结束（200ms）。
        val completed = engine.completeIfFinished(150L)
        assertFalse("Text-only completion must not end the whole transaction", completed)
        assertTrue(
            "Cursor track still running must keep animation active — " +
            "text finished but cursor continues (#595 六)",
            engine.hasActiveAnimation(),
        )
    }

    @Test
    fun submit_rebasesBothTimelinesIntoTerminalState() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 80L)
        engine.submit(cursorOnlyTransaction(transactionId = 3L, durationMs = 100L), submittedAtMs = 0L)
        // 连续输入重基：新事务提交时旧事务两条 timeline 都被 complete。
        engine.submit(cursorOnlyTransaction(transactionId = 4L, durationMs = 100L), submittedAtMs = 200L)
        assertTrue("New transaction active after rebase", engine.hasActiveAnimation())
        engine.completeIfFinished(400L)
        assertFalse("Rebased transaction reaches terminal state", engine.hasActiveAnimation())
    }
}
