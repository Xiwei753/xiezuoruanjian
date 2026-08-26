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
 * #605 评论3: RebasePlanner 契约测试 — 验证 revealFraction 连续性契约。
 *
 * 覆盖场景：
 * 1. Insert slice rebase 时 revealSpec.initialFraction 取自旧帧的 revealFraction。
 * 2. Delete slice rebase 时 revealSpec.initialFraction 取自旧帧的 revealFraction。
 * 3. 未匹配的 Delete slice 在 revealFraction < 0.99 时继续在新事务中吞完。
 * 4. 未匹配的 Delete slice 在 revealFraction >= 0.99 时不继续。
 * 5. 向后兼容：无 revealFraction 但 currentAlpha > 0.01 时仍继续。
 * 6. 向后兼容：无 revealFraction 且 currentAlpha <= 0.01 时不继续。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RebasePlannerTest {
    private val planner = RebasePlanner()

    @Test
    fun insertRebaseCarriesOldRevealFractionIntoInitialFraction() {
        // Insert slice rebase 时 revealSpec.initialFraction = old revealFraction
        val slice = makeAnimatedSlice(SliceRole.Insert, initialFraction = 0f)
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Insert,
                revealFraction = 0.5f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull("rebased slice 应携带 revealSpec", rebased.revealSpec)
        assertEquals(
            "Insert rebase 应将旧帧 revealFraction 写入新 spec 的 initialFraction",
            0.5f,
            rebased.revealSpec!!.initialFraction,
            0.0001f,
        )
    }

    @Test
    fun deleteRebaseCarriesOldRevealFractionIntoInitialFraction() {
        // Delete slice rebase 时 revealSpec.initialFraction = old revealFraction
        val slice =
            makeAnimatedSlice(
                SliceRole.Delete,
                initialFraction = 0f,
                revealMode = TextRevealMode.SWALLOW,
            )
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.3f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull("rebased slice 应携带 revealSpec", rebased.revealSpec)
        assertEquals(
            "Delete rebase 应将旧帧 revealFraction 写入新 spec 的 initialFraction",
            0.3f,
            rebased.revealSpec!!.initialFraction,
            0.0001f,
        )
    }

    @Test
    fun unmatchedDeleteWithRevealFractionBelowThresholdContinuesInNewTransaction() {
        // 未匹配的 Delete slice 在 revealFraction < 0.99 时继续在新事务中吞完
        // #639 评论 5427183226：修复后按三条视觉轨续播，Delete 的 targetAlpha=0f
        // （淡出到 0），currentAlpha=0.5 → alphaRemaining=true → 继续。
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.5f,
                currentAlpha = 0.5f,
                targetAlpha = 0f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertEquals(
            "revealFraction=0.5 的未匹配 Delete 应继续在新事务中吞完",
            1,
            result.size,
        )
        assertEquals(
            "继续吞完的 slice 角色应保持 Delete",
            SliceRole.Delete,
            result[0].role,
        )
    }

    @Test
    fun unmatchedDeleteWithRevealFractionAtOrAboveThresholdStops() {
        // 未匹配的 Delete slice 在 revealFraction >= 0.99 时不继续
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 1.0f,
                currentAlpha = 1.0f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 1.0f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertTrue(
            "revealFraction=1.0 的未匹配 Delete 已接近完成，不应继续",
            result.isEmpty(),
        )
    }

    @Test
    fun backwardCompatibleUnmatchedDeleteWithAlphaAboveThresholdContinues() {
        // #639 评论 5427183226：修复后按三条视觉轨续播。Delete 的 targetAlpha=0f
        // （淡出到 0），currentAlpha=0.5 → alphaRemaining=true → 继续。
        // 旧格式状态（targetAlpha 默认=currentAlpha）不会触发 alpha 续播，这是预期 —
        // 新格式 computeSliceVisualStates 会正确保存 targetAlpha=slice.endAlpha。
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = null,
                currentAlpha = 0.5f,
                targetAlpha = 0f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertEquals(
            "无 revealFraction 但 currentAlpha=0.5 的未匹配 Delete 应按旧 alpha 契约继续",
            1,
            result.size,
        )
        assertEquals(
            "继续吞完的 slice 角色应保持 Delete",
            SliceRole.Delete,
            result[0].role,
        )
    }

    @Test
    fun backwardCompatibleUnmatchedDeleteWithAlphaAtOrBelowThresholdStops() {
        // 向后兼容：无 revealFraction 且 currentAlpha <= 0.01 时不继续
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = null,
                currentAlpha = 0.005f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 1.0f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertTrue(
            "无 revealFraction 且 currentAlpha=0.005 的未匹配 Delete 应按旧 alpha 契约停止",
            result.isEmpty(),
        )
    }

    /**
     * #605 评论3: 未匹配的 Delete slice 在有 cluster snapshot 时必须继续用 clip swallow
     * （携带 SWALLOW revealSpec，initialFraction = old revealFraction，alpha 固定 1f），
     * 不退回 alpha 淡出。
     */
    @Test
    fun unmatchedDeleteWithRevealFractionContinuesAsClipSwallow() {
        val snapshot = makeSnapshotWithCluster()
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.5f,
                currentAlpha = 1f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = mapOf(1L to snapshot),
            )

        assertEquals("延续的 Delete slice 应存在", 1, result.size)
        val continued = result[0]
        assertEquals("角色应保持 Delete", SliceRole.Delete, continued.role)
        assertNotNull("延续 slice 必须携带 revealSpec（clip swallow，不退回 alpha）", continued.revealSpec)
        assertEquals("revealSpec 模式应为 SWALLOW", TextRevealMode.SWALLOW, continued.revealSpec!!.mode)
        assertEquals(
            "initialFraction 应取自旧帧 revealFraction",
            0.5f,
            continued.revealSpec!!.initialFraction,
            0.0001f,
        )
        assertEquals("anchorX 应为 cluster caretStartX（收缩终点）", 0f, continued.revealSpec!!.anchorX, 0.001f)
        assertEquals("boundaryFromX 应为 cluster caretEndX（起始边界）", 100f, continued.revealSpec!!.boundaryFromX, 0.001f)
        assertEquals("boundaryToX 应为 cluster caretStartX（终止边界）", 0f, continued.revealSpec!!.boundaryToX, 0.001f)
        assertEquals("clip 绘制时 startAlpha 固定 1f", 1f, continued.startAlpha, 0.001f)
        assertEquals("clip 绘制时 endAlpha 固定 1f", 1f, continued.endAlpha, 0.001f)
    }

    /**
     * #605 评论3 反向: 无 cluster snapshot 时回退 alpha（向后兼容）。
     * #639 评论 5427183226：修复后按三条视觉轨续播，Delete 的 targetAlpha=0f。
     */
    @Test
    fun unmatchedDeleteWithRevealFractionButNoClusterFallsBackToAlpha() {
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.5f,
                currentAlpha = 0.5f,
                targetAlpha = 0f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertEquals("无 cluster 时仍应延续", 1, result.size)
        val continued = result[0]
        assertEquals("角色应保持 Delete", SliceRole.Delete, continued.role)
        assertTrue("无 cluster 时 revealSpec 应为 null（alpha 回退）", continued.revealSpec == null)
    }

    /**
     * #637 评论 5386573878：applyRebaseState 用 rebaseState.remainingFraction 构造
     * continuation 窗口，不再从 localProgress 重新推。
     *
     * 旧帧保存 remainingFraction = 0.4（剩 40ms）。rebase 后新事务窗口 end = 0.4。
     */
    @Test
    fun applyRebaseState_usesRemainingFractionForContinuationWindow() {
        val slice = makeAnimatedSlice(SliceRole.Move)
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Move,
                currentAlpha = 1f,
                remainingFraction = 0.4f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertEquals(0f, rebased.progressWindow.start, 0f)
        assertEquals("continuation 窗口 end 应取自 remainingFraction", 0.4f, rebased.progressWindow.end, 0.001f)
    }

    /**
     * #637 评论 5386573878：未匹配 Delete continuation 也用 remainingFraction。
     * #639 评论 5427183226：修复后按三条视觉轨续播，Delete 的 targetAlpha=0f。
     */
    @Test
    fun unmatchedDeleteContinuation_usesRemainingFractionForWindow() {
        val deleteState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.5f,
                currentAlpha = 0.5f,
                remainingFraction = 0.3f,
                targetAlpha = 0f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(deleteState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertEquals(1, result.size)
        assertEquals(0.3f, result[0].progressWindow.end, 0.001f)
    }

    /**
     * #637 评论 5386573878：映射成功的 Insert continuation 重建 spec 为
     * progressStart=0/progressEnd=1/initialFraction=当前 revealFraction。
     *
     * 多 cluster/run 的 reveal 本来可能有非 [0,1] 子窗口（这里 progressStart=0.3,
     * progressEnd=0.7）。rebase 后外层 progress 从 0 重新开始，继续沿用旧
     * progressStart 会先停一段再继续。重建为 [0,1] 后从新事务第一帧就继续运动。
     */
    @Test
    fun mappedInsertRebase_rebuildsSpecToFullWindowProgress() {
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
                snapshot = null,
                sourceRect = Rect(0, 0, 10, 20),
                destinationRect = RectF(0f, 0f, 100f, 20f),
                startAlpha = 1f,
                endAlpha = 1f,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                revealSpec =
                    TextRevealSpec(
                        mode = TextRevealMode.REVEAL,
                        anchorX = 0f,
                        boundaryFromX = 0f,
                        boundaryToX = 100f,
                        progressStart = 0.3f,
                        progressEnd = 0.7f,
                        initialFraction = 0f,
                    ),
            )
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Insert,
                revealFraction = 0.4f,
                remainingFraction = 0.5f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull("mapped Insert rebase 应携带重建后的 revealSpec", rebased.revealSpec)
        val spec = rebased.revealSpec!!
        assertEquals("progressStart 应重建为 0f", 0f, spec.progressStart, 0f)
        assertEquals("progressEnd 应重建为 1f", 1f, spec.progressEnd, 0f)
        assertEquals("initialFraction 应取自旧帧 revealFraction", 0.4f, spec.initialFraction, 0.0001f)
        // 从新事务第一帧（localProgress=0）就继续运动，不重新等待旧 progressStart。
        val fractionAtFirstFrame = spec.fraction(0f)
        assertEquals(
            "第一帧 revealFraction 应等于 initialFraction，不停在 0",
            0.4f,
            fractionAtFirstFrame,
            0.0001f,
        )
        val fractionAtMid = spec.fraction(0.5f)
        assertEquals(
            "中点应线性插值到 0.7",
            0.7f,
            fractionAtMid,
            0.0001f,
        )
        assertEquals("末帧应完成", 1f, spec.fraction(1f), 0.0001f)
    }

    /**
     * #637 评论 5386573878：映射成功的 Delete continuation 同样重建 spec 为 [0,1]。
     */
    @Test
    fun mappedDeleteRebase_rebuildsSpecToFullWindowProgress() {
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Delete,
                snapshot = null,
                sourceRect = Rect(0, 0, 10, 20),
                destinationRect = RectF(0f, 0f, 100f, 20f),
                startAlpha = 1f,
                endAlpha = 0f,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                revealSpec =
                    TextRevealSpec(
                        mode = TextRevealMode.SWALLOW,
                        anchorX = 0f,
                        boundaryFromX = 100f,
                        boundaryToX = 0f,
                        progressStart = 0.2f,
                        progressEnd = 0.8f,
                        initialFraction = 0f,
                    ),
            )
        val rebaseState =
            makeSliceVisualState(
                role = SliceRole.Delete,
                revealFraction = 0.6f,
                remainingFraction = 0.4f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull(rebased.revealSpec)
        val spec = rebased.revealSpec!!
        assertEquals(0f, spec.progressStart, 0f)
        assertEquals(1f, spec.progressEnd, 0f)
        assertEquals(0.6f, spec.initialFraction, 0.0001f)
        assertEquals("第一帧继续运动，不重新等待旧 progressStart", 0.6f, spec.fraction(0f), 0.0001f)
    }

    /**
     * #637 评论 5386573878：双重 rebase 端到端 — remainingFraction 在两次 rebase 后
     * 保持 0.4 → 0.2，不会像旧 localProgress 方案变 0.4 → 0.5。
     *
     * 第一次 rebase：旧帧 remainingFraction=0.4 → 新窗口 [0, 0.4]。
     * 新事务走到 global 0.2 后再次 rebase：remainingFractionAt(0.2) = 0.2。
     */
    @Test
    fun doubleRebase_remainingFractionDoesNotReinflate() {
        val slice = makeAnimatedSlice(SliceRole.Move)
        // 第一次 rebase：旧帧剩 0.4
        val rebaseState1 =
            makeSliceVisualState(
                role = SliceRole.Move,
                currentAlpha = 1f,
                remainingFraction = 0.4f,
            )
        val rebased1 = planner.applyRebaseState(slice, rebaseState1, emptyMap())
        assertEquals(0.4f, rebased1.progressWindow.end, 0.001f)

        // 新事务走到 global 0.2，窗口 [0, 0.4] 的 remainingFractionAt(0.2) = 0.2
        val remainingAfterSecond = rebased1.progressWindow.remainingFractionAt(0.2f)
        assertEquals("第二次 rebase 必须剩 0.2，不能变 0.5", 0.2f, remainingAfterSecond, 0.001f)

        // 第二次 rebase
        val rebaseState2 =
            makeSliceVisualState(
                role = SliceRole.Move,
                currentAlpha = 1f,
                remainingFraction = remainingAfterSecond,
            )
        val rebased2 = planner.applyRebaseState(slice, rebaseState2, emptyMap())
        assertEquals(0.2f, rebased2.progressWindow.end, 0.001f)
    }

    // #639 评论 5422606865 问题1：未匹配的半截 Insert 不能被直接丢掉

    /**
     * #639 评论 5422606865 问题1：未匹配的半截 Insert（revealFraction=0.5,
     * currentAlpha=1f）必须继续在新事务中 reveal 完，不能被直接丢掉。
     *
     * 修复前：Insert 的 reveal 动画不靠 alpha（startAlpha/endAlpha 一直是 1），
     * isFadingOut=false、currentAlpha=1 时三个分支都进不去 → Insert 被丢弃
     * → 下一事务静态布局直接画出完整字符 → 快速连续输入"半个字突然变成完整字"。
     *
     * 修复后：专门看 revealFraction 的 Insert continuation 分支保留半截 Insert，
     * 用 REVEAL spec 从当前 revealFraction 继续 reveal 到完整。
     */
    @Test
    fun unmatchedInsertWithRevealFractionBelowThresholdContinuesInNewTransaction() {
        val snapshot = makeSnapshotWithCluster()
        val insertState =
            makeSliceVisualState(
                role = SliceRole.Insert,
                revealFraction = 0.5f,
                currentAlpha = 1f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(insertState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = mapOf(1L to snapshot),
            )

        assertEquals(
            "revealFraction=0.5 的未匹配 Insert 应继续在新事务中 reveal 完",
            1,
            result.size,
        )
        val continued = result[0]
        assertEquals("继续 reveal 的 slice 角色应保持 Insert", SliceRole.Insert, continued.role)
        assertNotNull(
            "延续 slice 必须携带 revealSpec（REVEAL 模式，从当前 fraction 继续）",
            continued.revealSpec,
        )
        val spec = continued.revealSpec!!
        assertEquals("revealSpec 模式应为 REVEAL", TextRevealMode.REVEAL, spec.mode)
        assertEquals(
            "Insert continuation initialFraction 应取自旧帧 revealFraction",
            0.5f,
            spec.initialFraction,
            0.0001f,
        )
        assertEquals("Insert continuation startAlpha 固定 1f", 1f, continued.startAlpha, 0.001f)
        assertEquals("Insert continuation endAlpha 固定 1f", 1f, continued.endAlpha, 0.001f)
        assertEquals(
            "anchorX 应为 cluster caretStartX（reveal 起点）",
            0f,
            spec.anchorX,
            0.001f,
        )
        assertEquals(
            "boundaryFromX 应为 cluster caretStartX",
            0f,
            spec.boundaryFromX,
            0.001f,
        )
        assertEquals(
            "boundaryToX 应为 cluster caretEndX（reveal 终点）",
            100f,
            spec.boundaryToX,
            0.001f,
        )
        // destinationRect 应为 currentRect（slice 在当前位置继续 reveal 完，不移动）
        assertEquals(0f, continued.destinationRect.left, 0.001f)
        assertEquals(0f, continued.destinationRect.top, 0.001f)
        assertEquals(100f, continued.destinationRect.right, 0.001f)
        assertEquals(20f, continued.destinationRect.bottom, 0.001f)
        // fromDestinationRect 应为 null（未匹配 Insert 没有位置移动）
        assertTrue(
            "未匹配 Insert continuation 的 fromDestinationRect 应为 null",
            continued.fromDestinationRect == null,
        )
        // sourceRect 应为 matchedCluster.sourceRectInLineImage
        assertEquals(
            "sourceRect 应取自 matchedCluster.sourceRectInLineImage",
            Rect(0, 0, 10, 20),
            continued.sourceRect,
        )
    }

    /**
     * #639 评论 5422606865 问题1 反向：revealFraction >= 0.99 时未匹配 Insert 不继续
     * （reveal 已接近完成，不需要再续）。
     */
    @Test
    fun unmatchedInsertWithRevealFractionAtOrAboveThresholdStops() {
        val snapshot = makeSnapshotWithCluster()
        val insertState =
            makeSliceVisualState(
                role = SliceRole.Insert,
                revealFraction = 1.0f,
                currentAlpha = 1f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 1.0f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(insertState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = mapOf(1L to snapshot),
            )

        assertTrue(
            "revealFraction=1.0 的未匹配 Insert 已接近完成，不应继续",
            result.isEmpty(),
        )
    }

    /**
     * #639 评论 5422606865 问题1 反向：matchedCluster 为 null（找不到 cluster）时
     * 未匹配 Insert 不继续。Insert 无 cluster 几何无法重建 reveal，不应强行画
     * （与 Delete continuation 无 cluster 时回退 alpha 的语义不同）。
     */
    @Test
    fun unmatchedInsertWithoutMatchedClusterDoesNotContinue() {
        // snapshotLookup 为空 → matchedCluster 必为 null
        val insertState =
            makeSliceVisualState(
                role = SliceRole.Insert,
                revealFraction = 0.5f,
                currentAlpha = 1f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(insertState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertTrue(
            "无 matchedCluster 时未匹配 Insert 不应继续（无法重建 reveal 几何）",
            result.isEmpty(),
        )
    }

    // #639 评论 5424986783：rebase continuation 按几何判断而非旧 SliceRole 判断位置运动

    /**
     * #639 评论 5424986783 问题1：映射成功的 Insert 在旧 role 已是 Insert（上一轮
     * Move → Insert 后再次 rebase）时，必须按几何判断继承 fromDestinationRect。
     *
     * 场景：rebaseState.role=Insert, currentRect=(0,0,100,20) != destinationRect=(200,0,300,20)。
     * 期望：rebased.fromDestinationRect == currentRect，新 Insert 从当前屏幕位置继续
     * 移动到新 destination，不跳变。
     *
     * 修复前：rebaseState.role == Insert != Move，走 else 不设置 fromDestinationRect → null → 跳变。
     */
    @Test
    fun mappedInsertRebase_carriesCurrentRectWhenOldRoleIsInsert() {
        val currentRect = RectF(0f, 0f, 100f, 20f)
        val destRect = RectF(200f, 0f, 300f, 20f)
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Insert,
                snapshot = null,
                sourceRect = Rect(0, 0, 10, 20),
                destinationRect = destRect,
                startAlpha = 1f,
                endAlpha = 1f,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                revealSpec =
                    TextRevealSpec(
                        mode = TextRevealMode.REVEAL,
                        anchorX = destRect.left,
                        boundaryFromX = destRect.left,
                        boundaryToX = destRect.right,
                        progressStart = 0f,
                        progressEnd = 1f,
                        initialFraction = 0f,
                    ),
            )
        val rebaseState =
            makeMovingSliceVisualState(
                role = SliceRole.Insert,
                currentRect = currentRect,
                destinationRect = destRect,
                currentAlpha = 1f,
                revealFraction = 0.5f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull(
            "映射成功的 Insert（旧 role=Insert, currentRect!=destinationRect）应继承 fromDestinationRect",
            rebased.fromDestinationRect,
        )
        assertEquals(FROM_DEST_LEFT_MSG, 0f, rebased.fromDestinationRect!!.left, 0.001f)
        assertEquals(FROM_DEST_RIGHT_MSG, 100f, rebased.fromDestinationRect!!.right, 0.001f)
    }

    /**
     * #639 评论 5424986783 问题2：映射成功的 CrossfadeNew 在旧 role 已是 CrossfadeNew
     * 时，必须按几何判断继承 fromDestinationRect。
     *
     * 场景：rebaseState.role=CrossfadeNew, currentRect != destinationRect。
     * 期望：rebased.fromDestinationRect == currentRect。
     *
     * 修复前：rebaseState.role == CrossfadeNew != Move，不设置 fromDestinationRect → 跳变。
     */
    @Test
    fun mappedCrossfadeNewRebase_carriesCurrentRectWhenOldRoleIsCrossfadeNew() {
        val currentRect = RectF(0f, 0f, 100f, 20f)
        val destRect = RectF(200f, 0f, 300f, 20f)
        val slice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.CrossfadeNew,
                snapshot = null,
                sourceRect = Rect(0, 0, 10, 20),
                destinationRect = destRect,
                startAlpha = 1f,
                endAlpha = 1f,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
            )
        val rebaseState =
            makeMovingSliceVisualState(
                role = SliceRole.CrossfadeNew,
                currentRect = currentRect,
                destinationRect = destRect,
                currentAlpha = 1f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull(
            "映射成功的 CrossfadeNew（旧 role=CrossfadeNew, currentRect!=destinationRect）应继承 fromDestinationRect",
            rebased.fromDestinationRect,
        )
        assertEquals(FROM_DEST_LEFT_MSG, 0f, rebased.fromDestinationRect!!.left, 0.001f)
        assertEquals(FROM_DEST_RIGHT_MSG, 100f, rebased.fromDestinationRect!!.right, 0.001f)
    }

    /**
     * #639 评论 5424986783 问题3（Insert）：未映射 moving Insert 在 currentRect !=
     * destinationRect 且 revealFraction < 1 时，必须同时继续位置和 reveal。
     *
     * 期望：destinationRect == state.destinationRect（保留真实终点），
     * fromDestinationRect == currentRect（继续位置移动）。
     *
     * 修复前：shouldContinueInsertReveal 分支把 destinationRect=currentRect、
     * fromDestinationRect=null，强行截断剩余位置移动。
     */
    @Test
    fun unmatchedMovingInsert_continuesPositionTowardDestination() {
        val currentRect = RectF(100f, 0f, 200f, 20f)
        val destRect = RectF(200f, 0f, 300f, 20f)
        val snapshot = makeSnapshotWithClusterAt(currentRect, 100f, 200f)
        val insertState =
            makeMovingSliceVisualState(
                role = SliceRole.Insert,
                currentRect = currentRect,
                destinationRect = destRect,
                currentAlpha = 1f,
                revealFraction = 0.5f,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(insertState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = mapOf(1L to snapshot),
            )

        assertTrue("未映射 moving Insert 应继续，不应被丢弃", result.isNotEmpty())
        val continued = result[0]
        assertEquals(
            "destinationRect.left 应为 state.destinationRect.left（保留真实终点）",
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
            "fromDestinationRect 应为 currentRect（继续位置移动）",
            continued.fromDestinationRect,
        )
        assertEquals(FROM_DEST_LEFT_MSG, 100f, continued.fromDestinationRect!!.left, 0.001f)
    }

    /**
     * #639 评论 5424986783 问题3（CrossfadeNew）：未映射 moving CrossfadeNew 在
     * currentAlpha == 1 但 currentRect != destinationRect 时不能被丢掉。
     *
     * 期望：result 非空，fromDestinationRect == currentRect，
     * destinationRect == state.destinationRect。
     *
     * 修复前：currentAlpha=1.0 不满足 !isFadingOut && currentAlpha < 0.99f，
     * 也不满足其他续播分支 → CrossfadeNew 被直接丢弃，剩余位置移动丢失。
     */
    @Test
    fun unmatchedMovingCrossfadeNewWithAlphaOne_continuesPosition() {
        val currentRect = RectF(100f, 0f, 200f, 20f)
        val destRect = RectF(200f, 0f, 300f, 20f)
        val crossfadeNewState =
            makeMovingSliceVisualState(
                role = SliceRole.CrossfadeNew,
                currentRect = currentRect,
                destinationRect = destRect,
                currentAlpha = 1.0f,
                revealFraction = null,
            )
        val rebaseSnapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = TransactionState.Rendering,
                sliceVisualStates = listOf(crossfadeNewState),
            )

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = emptyMap(),
            )

        assertTrue(
            "未映射 moving CrossfadeNew（currentAlpha==1 但 currentRect!=destinationRect）不应被丢弃",
            result.isNotEmpty(),
        )
        val continued = result[0]
        assertEquals(
            "destinationRect.left 应为 state.destinationRect.left",
            200f,
            continued.destinationRect.left,
            0.001f,
        )
        assertNotNull(
            "fromDestinationRect 应为 currentRect",
            continued.fromDestinationRect,
        )
        assertEquals(FROM_DEST_LEFT_MSG, 100f, continued.fromDestinationRect!!.left, 0.001f)
    }

    companion object {
        // #639 评论 5424986783：rebase continuation 断言消息常量（避免 StringLiteralDuplication）
        private const val FROM_DEST_LEFT_MSG = "fromDestinationRect.left 应为 currentRect.left"
        private const val FROM_DEST_RIGHT_MSG = "fromDestinationRect.right 应为 currentRect.right"

        /**
         * 构造一个带 revealSpec 的 AnimatedSlice。
         * [initialFraction] 用于验证 rebase 后是否被正确覆盖。
         */
        private fun makeAnimatedSlice(
            role: SliceRole,
            initialFraction: Float = 0f,
            revealMode: TextRevealMode = TextRevealMode.REVEAL,
        ): PreparedVisualTransaction.AnimatedSlice {
            return PreparedVisualTransaction.AnimatedSlice(
                role = role,
                snapshot = null,
                sourceRect = Rect(0, 0, 10, 20),
                destinationRect = RectF(0f, 0f, 100f, 20f),
                startAlpha = 1f,
                endAlpha = 1f,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                revealSpec =
                    TextRevealSpec(
                        mode = revealMode,
                        anchorX = 0f,
                        boundaryFromX = 0f,
                        boundaryToX = 100f,
                        progressStart = 0f,
                        progressEnd = 1f,
                        initialFraction = initialFraction,
                    ),
            )
        }

        /** 构造一个 SliceVisualState，[revealFraction] 默认为 null 以测试向后兼容。 */
        private fun makeSliceVisualState(
            role: SliceRole,
            revealFraction: Float? = null,
            currentAlpha: Float = 1f,
            remainingFraction: Float = 1f,
            targetAlpha: Float = currentAlpha,
        ): SliceVisualState {
            return SliceVisualState(
                snapshotId = 1L,
                role = role,
                lineIndex = 0,
                documentByteStart = 0,
                documentByteEndExclusive = 10,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                currentLeft = 0f,
                currentTop = 0f,
                currentRight = 100f,
                currentBottom = 20f,
                currentAlpha = currentAlpha,
                destinationLeft = 0f,
                destinationTop = 0f,
                destinationRight = 100f,
                destinationBottom = 20f,
                targetAlpha = targetAlpha,
                revealFraction = revealFraction,
                remainingFraction = remainingFraction,
            )
        }

        /**
         * 构造带 cluster 的 AndroidLineSnapshot，cluster 的 byte range 与
         * [makeSliceVisualState] 默认值一致（0..1），
         * caretStartX=0, caretEndX=100，与 destinationRect=(0,0,100,20) 对齐。
         */
        private fun makeSnapshotWithCluster(snapshotId: Long = 1L): AndroidLineSnapshot {
            return makeSnapshotWithClusterAt(
                visualRectInDocument = RectF(0f, 0f, 100f, 20f),
                caretStartX = 0f,
                caretEndX = 100f,
                snapshotId = snapshotId,
            )
        }

        /**
         * 构造带 cluster 的 AndroidLineSnapshot，cluster 的 visualRectInDocument
         * 和 caret 几何可参数化。供需要 currentRect != (0,0,100,20) 的测试使用。
         */
        private fun makeSnapshotWithClusterAt(
            visualRectInDocument: RectF,
            caretStartX: Float,
            caretEndX: Float,
            snapshotId: Long = 1L,
        ): AndroidLineSnapshot {
            val cluster =
                LineClusterSnapshot(
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

        /**
         * 构造 currentRect != destinationRect 的 SliceVisualState，模拟"正在移动的
         * Insert/CrossfadeNew"（上一轮 Move → Insert/CrossfadeNew 产生，位置仍在途中）。
         * 与 [makeSliceVisualState]（默认 currentRect == destinationRect）互补。
         */
        private fun makeMovingSliceVisualState(
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
    }
}
