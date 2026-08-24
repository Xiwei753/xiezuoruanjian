package com.xiwei.sujian.feature.editor.visual

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #637 评论 5389230907：renderer 和 rebase snapshot 对光标 progressWindow 的计算
 * 必须一致 — engine 的 captureFrame().cursorRect 必须等于 renderer 实际绘制的几何，
 * Pending continuation 的 cursorRemainingFraction 不能写死 1f 把 continuation window
 * 重新膨胀成 Full。
 *
 * 旧 bug：AndroidTextAnimationEngine.computeCurrentCursorRect 直接用全局 progress 插值，
 * renderer 却先 progressWindow.map(progress) 再插值。rebase continuation（window=[0,0.4]）
 * 时全局 progress=0.2 对应 renderer localProgress=0.5，engine 记成 0.2 的位置，下一次
 * rebase 光标从一个比屏幕实际位置更靠后的错误 cursorRect 起步，向回跳/突然变速。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CursorRebaseConsistencyTest {
    private fun createEngine(): AndroidTextAnimationEngine =
        AndroidTextAnimationEngine(
            AndroidVisualPlanner(),
            VisualResourceStore(),
            ChoreographerAnimationTimeSource(),
            TransactionIdSource(),
        )

    /**
     * 第一次 rebase 得到 cursor window [0, 0.4]，新事务 global progress=0.2 时，
     * captureFrame().cursorRect 必须等于 renderer 的 localProgress=0.5 几何。
     */
    @Test
    fun captureFrameCursorRectMatchesRendererLocalProgressAfterRebaseContinuation() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 100L)
        engine.setCoordinatedAnimationEnabled(true)
        val tx =
            continuationTransaction(
                transactionId = 1L,
                durationMs = 100L,
                cursorWindow = VisualProgressWindow(start = 0f, end = 0.4f),
                fromX = 0f,
                toX = 100f,
            )
        engine.submit(tx, submittedAtMs = 0L)

        // 20ms 时主 timeline global progress = 0.2；cursor window [0,0.4] 的 map(0.2)=0.5
        val frame = engine.captureFrame(20L)
        assertNotNull("Frame must be captured at 20ms", frame)
        val cursorRect = frame!!.cursorRect
        assertNotNull("Cursor rect must exist", cursorRect)

        // renderer 与 engine 共用 CursorTransition.rectAt — 同一份几何计算
        val rendererRect = tx.cursorTransition!!.rectAt(0.2f)
        assertEquals(
            "captureFrame cursorRect.left must equal renderer localProgress=0.5 geometry",
            rendererRect.left,
            cursorRect!!.left,
            0.001f,
        )
        assertEquals(
            "captureFrame cursorRect.top must equal renderer geometry",
            rendererRect.top,
            cursorRect.top,
            0.001f,
        )
        assertEquals(
            "captureFrame cursorRect.bottom must equal renderer geometry",
            rendererRect.bottom,
            cursorRect.bottom,
            0.001f,
        )

        // 锁住具体值：localProgress=0.5 → x = 0 + (100-0)*0.5 = 50
        assertEquals(
            "cursorRect.left must be 50f at localProgress=0.5",
            50f,
            cursorRect.left,
            0.001f,
        )
        // 旧 bug 直接用全局 progress=0.2 会得到 20f — 显式排除
        assertNotEquals(
            "cursorRect.left must not be 20f (old global-progress bug)",
            20f,
            cursorRect.left,
            0.001f,
        )
    }

    /**
     * Pending continuation [0, 0.4] 上立即再次 rebase，
     * cursorRemainingFraction 必须仍为 0.4，不能变回 1.0。
     */
    @Test
    fun pendingContinuationCursorRemainingFractionDoesNotReinflate() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 100L)
        engine.setCoordinatedAnimationEnabled(true)
        val tx =
            continuationTransaction(
                transactionId = 1L,
                durationMs = 100L,
                cursorWindow = VisualProgressWindow(start = 0f, end = 0.4f),
                fromX = 0f,
                toX = 100f,
            )
        // submittedAtMs = Long.MIN_VALUE → timeline 进入 Pending（第一帧真正画出来之前）
        engine.submit(tx, submittedAtMs = Long.MIN_VALUE)

        val frame = engine.captureFrame(0L)
        assertNotNull("Pending frame must be captured", frame)
        assertEquals(
            "cursorRemainingFraction must stay 0.4 in Pending continuation, not reinflate to 1.0",
            0.4f,
            frame!!.cursorRemainingFraction,
            0.001f,
        )
    }

    /**
     * Full window（新事务首次播放）的 Pending 帧仍剩 1f — 不破坏正常路径。
     */
    @Test
    fun pendingFullWindowCursorRemainingFractionIsOne() {
        val engine = createEngine()
        engine.setSmoothCursor(true, 100L)
        engine.setCoordinatedAnimationEnabled(true)
        val tx =
            continuationTransaction(
                transactionId = 1L,
                durationMs = 100L,
                cursorWindow = VisualProgressWindow.Full,
                fromX = 0f,
                toX = 100f,
            )
        engine.submit(tx, submittedAtMs = Long.MIN_VALUE)

        val frame = engine.captureFrame(0L)
        assertNotNull("Pending frame must be captured", frame)
        assertEquals(
            "cursorRemainingFraction must be 1f for Full window",
            1f,
            frame!!.cursorRemainingFraction,
            0.001f,
        )
    }

    private fun continuationTransaction(
        transactionId: Long,
        durationMs: Long,
        cursorWindow: VisualProgressWindow,
        fromX: Float,
        toX: Float,
    ): PreparedVisualTransaction =
        PreparedVisualTransaction(
            transactionId = transactionId,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = emptyList(),
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition =
                PreparedVisualTransaction.CursorTransition(
                    fromX = fromX,
                    fromY = 0f,
                    fromHeight = 20f,
                    toX = toX,
                    toY = 0f,
                    toHeight = 20f,
                    shouldAnimate = true,
                    progressWindow = cursorWindow,
                ),
            durationMs = durationMs,
            blockShifts =
                listOf(
                    PreparedVisualTransaction.BlockShift(
                        startLineIndex = 1,
                        endLineIndexExclusive = 3,
                        top = 40f,
                        bottom = 80f,
                        left = 0f,
                        right = 200f,
                        deltaY = 20f,
                    ),
                ),
        )
}
