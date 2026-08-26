package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot
import com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import com.xiwei.sujian.feature.editor.visual.TransactionState
import com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #639 评论 5424986783 复现测试 — RebasePlanner rebase continuation 按旧 SliceRole
 * 判断而非按实际视觉几何（currentRect vs destinationRect）续播的问题。
 *
 * 本复现测试断言 issue 描述的期望（修复后）行为。在当前 buggy 代码上这些断言会失败，
 * 从而证明 bug 存在。覆盖三处问题：
 *  1. 映射成功的 Insert：旧 role 已是 Insert（上一轮 Move → Insert）时丢当前位置。
 *  2. 映射成功的 CrossfadeNew：旧 role 已是 CrossfadeNew 时丢当前位置。
 *  3. 未映射的 moving Insert / CrossfadeNew：位置未走完时被截断或丢弃。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RebasePlannerReproTest {
    private val planner = RebasePlanner()

    // ---- helpers（独立于 RebasePlannerTest 的 private helper） ----

    private fun makeInsertSlice(destinationRect: RectF): PreparedVisualTransaction.AnimatedSlice {
        return PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Insert,
            snapshot = null,
            sourceRect = Rect(0, 0, 10, 20),
            destinationRect = destinationRect,
            startAlpha = 1f,
            endAlpha = 1f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 1,
            revealSpec = TextRevealSpec(
                mode = TextRevealMode.REVEAL,
                anchorX = destinationRect.left,
                boundaryFromX = destinationRect.left,
                boundaryToX = destinationRect.right,
                progressStart = 0f,
                progressEnd = 1f,
                initialFraction = 0f,
            ),
        )
    }

    private fun makeCrossfadeNewSlice(destinationRect: RectF): PreparedVisualTransaction.AnimatedSlice {
        return PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.CrossfadeNew,
            snapshot = null,
            sourceRect = Rect(0, 0, 10, 20),
            destinationRect = destinationRect,
            startAlpha = 1f,
            endAlpha = 1f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 1,
        )
    }

    /**
     * 构造 SliceVisualState。currentRect 与 destinationRect 可不同，
     * 模拟"正在移动的 Insert/CrossfadeNew"（上一轮 Move → Insert/CrossfadeNew 产生）。
     */
    private fun makeMovingState(
        role: SliceRole,
        currentRect: RectF,
        destinationRect: RectF,
        currentAlpha: Float = 1f,
        revealFraction: Float? = null,
    ): SliceVisualState {
        return SliceVisualState(
            snapshotId = 1L,
            role = role,
            lineIndex = 0,
            documentByteStart = 0,
            documentByteEndExclusive = 10,
            clusterByteStart = 0,
            clusterByteEndExclusive = 1,
            currentLeft = currentRect.left,
            currentTop = currentRect.top,
            currentRight = currentRect.right,
            currentBottom = currentRect.bottom,
            currentAlpha = currentAlpha,
            destinationLeft = destinationRect.left,
            destinationTop = destinationRect.top,
            destinationRight = destinationRect.right,
            destinationBottom = destinationRect.bottom,
            revealFraction = revealFraction,
            remainingFraction = 1f,
        )
    }

    private fun makeSnapshotWithCluster(
        visualRectInDocument: RectF,
        caretStartX: Float,
        caretEndX: Float,
        snapshotId: Long = 1L,
    ): AndroidLineSnapshot {
        val cluster = LineClusterSnapshot(
            clusterId = 0L,
            documentByteStart = 0,
            documentByteEndExclusive = 1,
            documentUtf16Start = 0,
            documentUtf16EndExclusive = 1,
            sourceRectInLineImage = Rect(0, 0, 10, 20),
            visualRectInDocument = visualRectInDocument,
            shapingFingerprint = "fp",
            shapingIdentityConfident = true,
            caretStartX = caretStartX,
            caretEndX = caretEndX,
        )
        return AndroidLineSnapshot(
            snapshotId = snapshotId,
            bitmap = null,
            lineIndex = 0,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = visualRectInDocument,
            clusters = listOf(cluster),
            documentByteStart = 0,
            documentByteEndExclusive = 10,
            documentUtf16Start = 0,
            documentUtf16EndExclusive = 10,
            baseline = 16f,
            lineHeight = 20f,
        )
    }

    // ---- 复现 1：映射成功的 Insert 会丢当前位置（issue 问题1） ----

    /**
     * 场景：上一轮 Move → Insert 后，rebaseState.role 已是 Insert，
     * currentRect=(0,0,100,20) 仍在 from→destination 中间，destinationRect=(200,0,300,20)。
     * 下一笔再 rebase 传入新 Insert slice（destinationRect=(200,0,300,20)）。
     *
     * 期望（修复后）：fromDestinationRect = currentRect = (0,0,100,20)，
     * 新 Insert 从当前屏幕位置继续移动到新 destination。
     *
     * 当前 buggy：rebaseState.role == Insert != Move，走 else 分支不设置 fromDestinationRect，
     * 保持 null → 新 Insert 第一帧从 destinationRect 开始，位置跳变。
     */
    @Test
    fun repro1_mappedInsertRebase_carriesCurrentRectWhenOldRoleIsInsert() {
        val currentRect = RectF(0f, 0f, 100f, 20f)
        val destRect = RectF(200f, 0f, 300f, 20f)
        val slice = makeInsertSlice(destRect)
        val rebaseState = makeMovingState(
            role = SliceRole.Insert,
            currentRect = currentRect,
            destinationRect = destRect,
            currentAlpha = 1f,
            revealFraction = 0.5f,
        )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull(
            "映射成功的 Insert（旧 role=Insert, currentRect!=destinationRect）应继承 fromDestinationRect，" +
                "实际为 null → 新 Insert 第一帧从 destinationRect 开始，位置跳变",
            rebased.fromDestinationRect,
        )
        if (rebased.fromDestinationRect != null) {
            assertEquals(FROM_DEST_LEFT_MSG, 0f, rebased.fromDestinationRect!!.left, 0.001f)
            assertEquals(FROM_DEST_RIGHT_MSG, 100f, rebased.fromDestinationRect!!.right, 0.001f)
        }
    }

    // ---- 复现 2：映射成功的 CrossfadeNew 也有同样问题（issue 问题2） ----

    /**
     * 场景：上一轮 Move → CrossfadeNew 后，rebaseState.role 已是 CrossfadeNew，
     * currentRect=(0,0,100,20) 仍在 from→destination 中间，destinationRect=(200,0,300,20)。
     * 下一笔再 rebase 传入新 CrossfadeNew slice。
     *
     * 期望（修复后）：fromDestinationRect = currentRect。
     * 当前 buggy：rebaseState.role == CrossfadeNew != Move，不设置 fromDestinationRect → 位置跳变。
     */
    @Test
    fun repro2_mappedCrossfadeNewRebase_carriesCurrentRectWhenOldRoleIsCrossfadeNew() {
        val currentRect = RectF(0f, 0f, 100f, 20f)
        val destRect = RectF(200f, 0f, 300f, 20f)
        val slice = makeCrossfadeNewSlice(destRect)
        val rebaseState = makeMovingState(
            role = SliceRole.CrossfadeNew,
            currentRect = currentRect,
            destinationRect = destRect,
            currentAlpha = 1f,
        )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull(
            "映射成功的 CrossfadeNew（旧 role=CrossfadeNew, currentRect!=destinationRect）应继承 fromDestinationRect，" +
                "实际为 null → 位置跳变",
            rebased.fromDestinationRect,
        )
        if (rebased.fromDestinationRect != null) {
            assertEquals(FROM_DEST_LEFT_MSG, 0f, rebased.fromDestinationRect!!.left, 0.001f)
            assertEquals(FROM_DEST_RIGHT_MSG, 100f, rebased.fromDestinationRect!!.right, 0.001f)
        }
    }

    // ---- 复现 3：未映射的 moving Insert 位置截断（issue 问题3 Insert） ----

    /**
     * 场景：未映射的 moving Insert，currentRect=(100,0,200,20) != destinationRect=(200,0,300,20)，
     * revealFraction=0.5（reveal 也没走完）。
     *
     * 期望（修复后）：保留旧状态真实终点，
     *   fromDestinationRect = currentRect = (100,0,200,20)，
     *   destinationRect = state.destinationRect = (200,0,300,20)，
     * 位置和 reveal 同时继续。
     *
     * 当前 buggy：shouldContinueInsertReveal 分支把 destinationRect = currentRect、
     * fromDestinationRect = null，强行截断剩余位置移动。
     */
    @Test
    fun repro3_unmatchedMovingInsert_continuesPositionTowardDestination() {
        val currentRect = RectF(100f, 0f, 200f, 20f)
        val destRect = RectF(200f, 0f, 300f, 20f)
        val snapshot = makeSnapshotWithCluster(
            visualRectInDocument = currentRect,
            caretStartX = 100f,
            caretEndX = 200f,
        )
        val insertState = makeMovingState(
            role = SliceRole.Insert,
            currentRect = currentRect,
            destinationRect = destRect,
            currentAlpha = 1f,
            revealFraction = 0.5f,
        )
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(insertState),
        )

        val result = planner.applyRebaseToSlices(
            newSlices = emptyList(),
            rebaseSnapshot = rebaseSnapshot,
            snapshotLookup = mapOf(1L to snapshot),
        )

        assertTrue(
            "未映射 moving Insert（currentRect!=destinationRect, revealFraction<1）应继续，不应被丢弃",
            result.isNotEmpty(),
        )
        if (result.isNotEmpty()) {
            val continued = result[0]
            assertEquals(
                "未映射 moving Insert 的 destinationRect 应为 state.destinationRect，保留真实终点，" +
                    "实际 destinationRect.left=${continued.destinationRect.left}",
                200f,
                continued.destinationRect.left,
                0.001f,
            )
            assertEquals(
                "destinationRect.right 应为 state.destinationRect.right",
                300f,
                continued.destinationRect.right,
                0.001f,
            )
            assertNotNull(
                "未映射 moving Insert 的 fromDestinationRect 应为 currentRect，继续位置移动，" +
                    "实际为 null → 位置移动被截断",
                continued.fromDestinationRect,
            )
            if (continued.fromDestinationRect != null) {
                assertEquals(
                    FROM_DEST_LEFT_MSG,
                    100f,
                    continued.fromDestinationRect!!.left,
                    0.001f,
                )
            }
        }
    }

    // ---- 复现 4：未映射的 moving CrossfadeNew 在 alpha==1 但位置未走完时被丢弃（issue 问题3 CrossfadeNew） ----

    /**
     * 场景：未映射的 moving CrossfadeNew，currentAlpha=1.0f（已全亮），
     * currentRect=(100,0,200,20) != destinationRect=(200,0,300,20)（位置还没走完）。
     *
     * 期望（修复后）：继续位置移动，result 非空，
     *   fromDestinationRect = currentRect，destinationRect = state.destinationRect。
     *
     * 当前 buggy：currentAlpha=1.0f 不满足 !isFadingOut && currentAlpha < 0.99f，
     * 也不满足其他续播分支 → CrossfadeNew 被直接丢弃，剩余位置移动丢失。
     */
    @Test
    fun repro4_unmatchedMovingCrossfadeNewWithAlphaOne_continuesPosition() {
        val currentRect = RectF(100f, 0f, 200f, 20f)
        val destRect = RectF(200f, 0f, 300f, 20f)
        val crossfadeNewState = makeMovingState(
            role = SliceRole.CrossfadeNew,
            currentRect = currentRect,
            destinationRect = destRect,
            currentAlpha = 1.0f,
            revealFraction = null,
        )
        val rebaseSnapshot = VisualFrameSnapshot(
            progress = 0.5f,
            state = TransactionState.Rendering,
            sliceVisualStates = listOf(crossfadeNewState),
        )

        val result = planner.applyRebaseToSlices(
            newSlices = emptyList(),
            rebaseSnapshot = rebaseSnapshot,
            snapshotLookup = emptyMap(),
        )

        assertTrue(
            "未映射 moving CrossfadeNew（currentAlpha==1 但 currentRect!=destinationRect）不应被丢弃，" +
                "应继续位置移动，实际 result 为空",
            result.isNotEmpty(),
        )
        if (result.isNotEmpty()) {
            val continued = result[0]
            assertEquals(
                "未映射 moving CrossfadeNew 的 destinationRect 应为 state.destinationRect",
                200f,
                continued.destinationRect.left,
                0.001f,
            )
            assertNotNull(
                "未映射 moving CrossfadeNew 的 fromDestinationRect 应为 currentRect",
                continued.fromDestinationRect,
            )
            if (continued.fromDestinationRect != null) {
                assertEquals(
                    FROM_DEST_LEFT_MSG,
                    100f,
                    continued.fromDestinationRect!!.left,
                    0.001f,
                )
            }
        }
    }

    companion object {
        private const val FROM_DEST_LEFT_MSG = "fromDestinationRect.left 应为 currentRect.left"
        private const val FROM_DEST_RIGHT_MSG = "fromDestinationRect.right 应为 currentRect.right"
    }
}
