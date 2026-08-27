package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
import com.xiwei.sujian.feature.editor.visual.TransactionState
import com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #639 评论 5427183226 复现测试 — SliceVisualState 还不是"上一帧实际画出来的完整 slice 状态"。
 *
 * 两个确定缺口：
 *
 * 缺口1：sourceRect 没进 SliceVisualState，未映射 continuation 会拿整行 bitmap 当旧 slice 像素。
 *  - SliceVisualState 没有 sourceRect 字段（AnimationTimeline.kt）。
 *  - computeSliceVisualStates 不保存 slice.sourceRect（AndroidTextAnimationEngine.kt）。
 *  - handleUnmappedRebaseState 在 caretRevealGeometry != null（新格式）时 matchedCluster == null，
 *    sourceRect 退到 snapshot.sourceRect（整行 bitmap 的 source crop）。
 *  - 现有 repro5_syntheticRun... 只检查 result 和 revealFraction，没检查 sourceRect，漏过这个错。
 *
 * 缺口2：appearance 轨没有真正跨"第二次 rebase"保存：缺 targetAlpha/revealMode/fixedRevealClipRect。
 *  - SliceVisualState 没有 targetAlpha/revealMode/fixedRevealClipRect 字段。
 *  - handleUnmappedRebaseState 按 role 分支：Move 进 buildMoveContinuation（丢 revealSpec），
 *    CrossfadeNew 进 buildAlphaOrPositionContinuation（丢 reveal）。
 *  - buildFadingOutContinuation 只要有 revealSpec 就硬写 startAlpha=1/endAlpha=1，把 alpha 抬回 1。
 *  - fixedRevealClipRect 只在 AnimatedSlice，computeSliceVisualStates 根本没保存。
 *
 * 本测试在当前 buggy 代码上会失败，从而证明两个缺口存在。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue639Comment5427183226ReproTest {
    private val planner = RebasePlanner()

    // ---- 缺口1：synthetic run rebase 后 sourceRect 变成整行 snapshot.sourceRect ----

    /**
     * 缺口1：synthetic run（byte 0..3 跨 3 个 cluster）rebase 后，continuation 的 sourceRect
     * 应该是原 AnimatedSlice.sourceRect（合并后的几个字，如 Rect(0,0,30,20)），而不是整行
     * snapshot.sourceRect（Rect(0,0,300,20)）。
     *
     * 当前 buggy：state.caretRevealGeometry != null（新格式 SliceVisualState）→ matchedCluster == null
     * → sourceRect 退到 snapshot.sourceRect = Rect(0,0,300,20)（整行 bitmap 的 source crop），
     * 把整行 bitmap 压进 run 的 destinationRect。
     *
     * 现有 repro5_syntheticRun... 只检查 result 非空和 revealSpec.initialFraction==0.4，
     * 没检查 sourceRect，所以把这个错漏过去了。
     */
    @Test
    fun gap1_syntheticRunUnmappedSourceRectShouldNotBeWholeLineSnapshotSourceRect() {
        val rect = RectF(0f, 0f, 300f, 20f)
        val clusters =
            listOf(
                com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot(
                    clusterId = 0L,
                    documentByteStart = 0,
                    documentByteEndExclusive = 1,
                    documentUtf16Start = 0,
                    documentUtf16EndExclusive = 1,
                    sourceRectInLineImage = Rect(0, 0, 10, 20),
                    visualRectInDocument = RectF(0f, 0f, 100f, 20f),
                    shapingFingerprint = "fp0",
                    shapingIdentityConfident = true,
                    caretStartX = 0f,
                    caretEndX = 100f,
                ),
                com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot(
                    clusterId = 1L,
                    documentByteStart = 1,
                    documentByteEndExclusive = 2,
                    documentUtf16Start = 1,
                    documentUtf16EndExclusive = 2,
                    sourceRectInLineImage = Rect(10, 0, 20, 20),
                    visualRectInDocument = RectF(100f, 0f, 200f, 20f),
                    shapingFingerprint = "fp1",
                    shapingIdentityConfident = true,
                    caretStartX = 100f,
                    caretEndX = 200f,
                ),
                com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot(
                    clusterId = 2L,
                    documentByteStart = 2,
                    documentByteEndExclusive = 3,
                    documentUtf16Start = 2,
                    documentUtf16EndExclusive = 3,
                    sourceRectInLineImage = Rect(20, 0, 30, 20),
                    visualRectInDocument = RectF(200f, 0f, 300f, 20f),
                    shapingFingerprint = "fp2",
                    shapingIdentityConfident = true,
                    caretStartX = 200f,
                    caretEndX = 300f,
                ),
            )
        // 整行 snapshot：sourceRect 是整行 bitmap (0,0,300,20)。
        val snapshot =
            com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot(
                snapshotId = 1L,
                bitmap = null,
                lineIndex = 0,
                sourceRect = Rect(0, 0, 300, 20),
                destinationRect = rect,
                clusters = clusters,
                documentByteStart = 0,
                documentByteEndExclusive = 3,
                documentUtf16Start = 0,
                documentUtf16EndExclusive = 3,
                baseline = 16f,
                lineHeight = 20f,
            )
        // synthetic run 的 SliceVisualState：byte 0..3 跨 3 个 cluster，reveal=0.4，
        // caretRevealGeometry 非 null（新格式 — planner 已给正常 slice 写了 caretRevealGeometry）。
        // #639 评论 5427183226 缺口1修复后：sourceRect 从 active slice 原样保存进 SliceVisualState，
        // synthetic run 的 sourceRect 是合并后的几个字 Rect(0,0,30,20)，不是整行 snapshot.sourceRect。
        val syntheticRunSourceRect = Rect(0, 0, 30, 20)
        val syntheticRunState =
            SliceVisualState(
                snapshotId = 1L,
                role = SliceRole.Insert,
                lineIndex = 0,
                documentByteStart = 0,
                documentByteEndExclusive = 3,
                clusterByteStart = 0,
                clusterByteEndExclusive = 3,
                currentLeft = rect.left,
                currentTop = rect.top,
                currentRight = rect.right,
                currentBottom = rect.bottom,
                currentAlpha = 1f,
                destinationLeft = rect.left,
                destinationTop = rect.top,
                destinationRight = rect.right,
                destinationBottom = rect.bottom,
                sourceRect = syntheticRunSourceRect,
                targetAlpha = 1f,
                revealMode = com.xiwei.sujian.feature.editor.visual.TextRevealMode.REVEAL,
                revealFraction = 0.4f,
                remainingFraction = 1f,
                caretRevealGeometry =
                    PreparedVisualTransaction.CaretRevealGeometry(
                        visualRect = rect,
                        caretStartX = 0f,
                        caretEndX = 300f,
                    ),
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.4f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(syntheticRunState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = mapOf(1L to snapshot),
            )

        assertTrue(
            "synthetic run 未映射时应产生 continuation，实际 result 为空",
            result.isNotEmpty(),
        )
        val continued = result[0]
        val wholeLineSourceRect = snapshot.sourceRect
        // synthetic run 的真实 sourceRect 应是合并后的几个字（Rect(0,0,30,20)），
        // 绝不应是整行 snapshot.sourceRect = Rect(0,0,300,20)。
        assertEquals(
            "缺口1修复后：synthetic run continuation 的 sourceRect 应是原 AnimatedSlice.sourceRect" +
                "（合并后的几个字 Rect(0,0,30,20)），实际 $wholeLineSourceRect 则是整行 bitmap。" +
                "修复前 buggy：caretRevealGeometry != null → matchedCluster == null → sourceRect 退到" +
                " snapshot.sourceRect = $wholeLineSourceRect，把整行 bitmap 压进 run 的 destinationRect。" +
                "现有 repro5_syntheticRun... 没检查 sourceRect，把这个错漏过去了。",
            syntheticRunSourceRect,
            continued.sourceRect,
        )
        assertNotEquals(
            "synthetic run continuation 的 sourceRect 不应等于整行 snapshot.sourceRect",
            wholeLineSourceRect,
            continued.sourceRect,
        )
    }

    // ---- 缺口2：appearance 轨跨第二次 rebase 丢失 ----

    /**
     * 缺口2a：由 Insert -> Move 产生、当前仍有 revealFraction=0.5 的 Move，再未映射时直接进
     * buildMoveContinuation()，revealSpec 被丢掉，半截字瞬间补全。
     *
     * 当前 buggy：handleUnmappedRebaseState 第 131 行 `else if (state.role == SliceRole.Move)`
     * 分支 → buildMoveContinuation 不传 revealSpec → result[0].revealSpec == null。
     *
     * 期望：旧 state 有 revealFraction=0.5，无论 role 是 Move，都应继续 reveal，revealSpec != null。
     */
    @Test
    fun gap2a_moveWithRevealFractionUnmappedShouldContinueRevealNotDropIt() {
        val fromRect = RectF(0f, 0f, 100f, 20f)
        val destRect = RectF(50f, 0f, 150f, 20f)
        // 旧 state：role=Move，revealFraction=0.5（由 Insert -> Move 产生），caretRevealGeometry 非 null。
        // currentRect != destRect 让 buildMoveContinuation 返回非 null（有位置运动）。
        val state =
            SliceVisualState(
                snapshotId = 1L,
                role = SliceRole.Move,
                lineIndex = 0,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                currentLeft = fromRect.left,
                currentTop = fromRect.top,
                currentRight = fromRect.right,
                currentBottom = fromRect.bottom,
                currentAlpha = 1f,
                destinationLeft = destRect.left,
                destinationTop = destRect.top,
                destinationRight = destRect.right,
                destinationBottom = destRect.bottom,
                revealFraction = 0.5f,
                remainingFraction = 1f,
                caretRevealGeometry =
                    PreparedVisualTransaction.CaretRevealGeometry(
                        visualRect = destRect,
                        caretStartX = destRect.left,
                        caretEndX = destRect.right,
                    ),
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(state),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertTrue(
            "带 revealFraction=0.5 的 Move 未映射时应产生 continuation，实际 result 为空",
            result.isNotEmpty(),
        )
        assertNotNull(
            "缺口2a：由 Insert -> Move 产生、仍有 revealFraction=0.5 的 Move，再未映射时应继续 reveal" +
                "（revealSpec != null），半截字不应瞬间补全。当前 buggy：state.role == Move 分支 →" +
                " buildMoveContinuation 不传 revealSpec → revealSpec == null，半截字瞬间补全。",
            result[0].revealSpec,
        )
    }

    /**
     * 缺口2b：由 Insert -> CrossfadeNew 产生、仍有 reveal 的 CrossfadeNew，再未映射时会进
     * buildAlphaOrPositionContinuation()，一样把 reveal 丢掉。
     *
     * 当前 buggy：handleUnmappedRebaseState 第 135 行 else 分支 → buildAlphaOrPositionContinuation
     * 不传 revealSpec → result[0].revealSpec == null。
     *
     * 期望：旧 state 有 revealFraction=0.5，无论 role 是 CrossfadeNew，都应继续 reveal。
     */
    @Test
    fun gap2b_crossfadeNewWithRevealFractionUnmappedShouldContinueRevealNotDropIt() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val state =
            SliceVisualState(
                snapshotId = 1L,
                role = SliceRole.CrossfadeNew,
                lineIndex = 0,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                currentLeft = rect.left,
                currentTop = rect.top,
                currentRight = rect.right,
                currentBottom = rect.bottom,
                currentAlpha = 1f,
                destinationLeft = rect.left,
                destinationTop = rect.top,
                destinationRight = rect.right,
                destinationBottom = rect.bottom,
                revealFraction = 0.5f,
                remainingFraction = 1f,
                caretRevealGeometry =
                    PreparedVisualTransaction.CaretRevealGeometry(
                        visualRect = rect,
                        caretStartX = rect.left,
                        caretEndX = rect.right,
                    ),
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(state),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertTrue(
            "带 revealFraction=0.5 的 CrossfadeNew 未映射时应产生 continuation，实际 result 为空",
            result.isNotEmpty(),
        )
        assertNotNull(
            "缺口2b：由 Insert -> CrossfadeNew 产生、仍有 reveal 的 CrossfadeNew，再未映射时应继续 reveal" +
                "（revealSpec != null）。当前 buggy：进 buildAlphaOrPositionContinuation 不传 revealSpec →" +
                " revealSpec == null，reveal 被丢掉。",
            result[0].revealSpec,
        )
    }

    /**
     * 缺口2c：buildFadingOutContinuation 只要有 revealSpec 就硬写 startAlpha=1/endAlpha=1，
     * 如果这个 Delete 本身已经同时带 alpha 淡出（currentAlpha=0.4），下一次 rebase 会把 alpha
     * 又抬回 1。
     *
     * 当前 buggy：buildFadingOutContinuation 第 180-181 行
     * `startAlpha = if (continueRevealSpec != null) 1f else state.currentAlpha` → startAlpha = 1f。
     *
     * 期望：应保持当前 alpha（startAlpha == currentAlpha == 0.4），不应抬回 1。
     */
    @Test
    fun gap2c_deleteWithRevealAndFadingAlphaShouldKeepCurrentAlphaNotResetToOne() {
        val rect = RectF(0f, 0f, 100f, 20f)
        // Delete 同时带 reveal（revealFraction=0.5）和 alpha 淡出（currentAlpha=0.4）。
        val state =
            SliceVisualState(
                snapshotId = 1L,
                role = SliceRole.Delete,
                lineIndex = 0,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                currentLeft = rect.left,
                currentTop = rect.top,
                currentRight = rect.right,
                currentBottom = rect.bottom,
                currentAlpha = 0.4f,
                destinationLeft = rect.left,
                destinationTop = rect.top,
                destinationRight = rect.right,
                destinationBottom = rect.bottom,
                revealFraction = 0.5f,
                remainingFraction = 1f,
                caretRevealGeometry =
                    PreparedVisualTransaction.CaretRevealGeometry(
                        visualRect = rect,
                        caretStartX = rect.left,
                        caretEndX = rect.right,
                    ),
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(state),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertTrue(
            "带 reveal 和 alpha 淡出的 Delete 未映射时应产生 continuation，实际 result 为空",
            result.isNotEmpty(),
        )
        assertEquals(
            "缺口2c：Delete 同时带 reveal 和 alpha 淡出（currentAlpha=0.4）时，continuation 的" +
                " startAlpha 应保持当前 alpha=0.4，不应被抬回 1。当前 buggy：buildFadingOutContinuation" +
                " 只要有 revealSpec 就硬写 startAlpha=1 → 下一次 rebase 把 alpha 又抬回 1。",
            0.4f,
            result[0].startAlpha,
            0.001f,
        )
    }

    /**
     * 缺口2d（修复后验证）：SliceVisualState 现在已有 fixedRevealClipRect 字段。
     *
     * 修复前：fixedRevealClipRect 只存在 AnimatedSlice 和 RebasePlanner.applyRebaseState 的
     * CrossfadeOld 分支，computeSliceVisualStates 根本没有保存。旧 Insert -> CrossfadeOld
     * 冻结出来的半截 clip，下一次 mapped/unmapped rebase 后会消失，重新画完整字。
     *
     * 修复后（#639 评论 5427183226）：SliceVisualState 增加了 fixedRevealClipRect 字段，
     * computeSliceVisualStates 从 active slice.fixedRevealClipRect 原样保存，未映射
     * continuation 原样带下去，renderer 提到 drawOrthogonalSlice 正交化。
     */
    @Test
    fun gap2d_sliceVisualStateShouldHaveFixedRevealClipRectField() {
        val field =
            SliceVisualState::class.java.declaredFields.firstOrNull {
                it.name == "fixedRevealClipRect"
            }
        assertNotNull(
            "缺口2d修复后：SliceVisualState 应有 fixedRevealClipRect 字段。" +
                "issue 描述：fixedRevealClipRect 仍只存在 AnimatedSlice，computeSliceVisualStates" +
                " 根本没有保存 → 旧 Insert -> CrossfadeOld 冻结出来的半截 clip，下一次 rebase 后" +
                " 会消失，重新画完整字。修复后该字段已补上。",
            field,
        )
    }
}
