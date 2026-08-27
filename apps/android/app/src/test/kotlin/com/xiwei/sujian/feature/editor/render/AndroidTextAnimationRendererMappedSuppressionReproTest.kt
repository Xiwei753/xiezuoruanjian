package com.xiwei.sujian.feature.editor.render

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.StaticSuppressionMode
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #639 评论 5427812180 复现测试 — static suppression 按 SliceRole 判断而非按独立 mode。
 *
 * issue 缺陷 4：computeStaticSuppressionRegions() 仍按 when(slice.role)：
 * Insert/Move/CrossfadeNew/Static -> destinationRect hole, Delete -> reveal/fixed clip hole,
 * CrossfadeOld -> no hole。mapped rebase 继续旧视觉轨后 role 和"静态底图怎么挖洞"会不一致。
 *
 * 修复方案（issue 缺陷 4）：给 AnimatedSlice 增加独立
 * StaticSuppressionMode { NONE, DESTINATION_RECT, VISIBLE_CLIP } 字段，planner 初次创建时
 * 按 role 设定，continuation 继续旧 state，renderer 改按 slice.staticSuppressionMode。
 *
 * 场景 7：
 * - Delete -> CrossfadeOld 继续旧 VISIBLE_CLIP：旧 Delete suppress 吞字区域（VISIBLE_CLIP），
 *   新 CrossfadeOld 应继续 suppress fixedClip（让动画 slice 独占 fixedClip 区域，避免双影）。
 *   当前 buggy：CrossfadeOld 分支不 suppress 任何区域 → 底图也画 fixedClip → 双影。
 * - CrossfadeOld -> Delete 继续旧 NONE：旧 CrossfadeOld alpha 混合（NONE，底图画完整字），
 *   新 Delete 应继续 NONE。当前 buggy：Delete 分支按 role 重新判断 suppression。
 *
 * 本测试断言 issue 描述的期望（修复后）行为。在当前 buggy 代码（基线 d48681ff）上这些断言会失败，
 * 从而证明 bug 存在。测试能编译通过（即使断言失败），Phase B 修复后可直接转成回归测试。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AndroidTextAnimationRendererMappedSuppressionReproTest {
    private val renderer = AndroidTextAnimationRenderer()
    private val eps = 0.001f

    private fun makeTransaction(slices: List<PreparedVisualTransaction.AnimatedSlice>): PreparedVisualTransaction {
        return PreparedVisualTransaction(
            transactionId = 1L,
            oldRevision = null,
            newRevision = null,
            staticPatches = emptyList(),
            animatedSlices = slices,
            ownedSnapshotIds = emptySet(),
            referencedSnapshotIds = emptySet(),
            selectionDecoration = null,
            preeditDecoration = null,
            cursorTransition = null,
            durationMs = 300L,
        )
    }

    /**
     * 场景 7a：Delete -> CrossfadeOld 继续旧 VISIBLE_CLIP。
     *
     * 旧 Delete swallow 到一半，suppress 吞字区域（VISIBLE_CLIP）。rebase 成 CrossfadeOld 后，
     * CrossfadeOld 携带 fixedRevealClipRect（冻结半截字，见 RebasePlannerMappedRebaseReproTest 场景 2）。
     * 新 CrossfadeOld 应继续 suppress fixedClip 区域（VISIBLE_CLIP），让动画 slice 独占 fixedClip，
     * 避免底图双影。
     *
     * 当前 buggy：computeStaticSuppressionRegions CrossfadeOld 分支（renderer line 230-232）
     * 不 suppress 任何区域（NONE），返回空 → 底图也画 fixedClip 区域 → 双影。
     *
     * 修复后：renderer 应按 slice.staticSuppressionMode 判断，CrossfadeOld continuation 继承旧 Delete
     * 的 VISIBLE_CLIP，suppress fixedRevealClipRect。
     */
    @Test
    fun repro7a_deleteToCrossfadeOld_suppressionShouldContinueVisibleClipNotSwitchToNone() {
        val dest = RectF(0f, 0f, 100f, 20f)
        val fixedClip = RectF(0f, 0f, 60f, 20f) // 旧 Delete swallow 到 40% 冻结的可见部分
        // rebase 后的 CrossfadeOld slice，携带 fixedRevealClipRect（场景 2 修复后的状态）。
        // #639 评论 5427812180 缺陷4：mapped rebase 继续旧 Delete 的 VISIBLE_CLIP，
        // 不因新 role=CrossfadeOld 瞬间切换到 NONE。
        val crossfadeOldSlice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.CrossfadeOld,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 1f,
                endAlpha = 0f,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                fixedRevealClipRect = fixedClip,
                staticSuppressionMode = StaticSuppressionMode.VISIBLE_CLIP,
            )
        val transaction = makeTransaction(listOf(crossfadeOldSlice))

        val regions = renderer.computeStaticSuppressionRegions(transaction, 0f)

        // 应 suppress fixedClip 区域（继续旧 Delete 的 VISIBLE_CLIP）
        assertFalse(
            "Delete -> CrossfadeOld：suppression 应继续旧 Delete 的 VISIBLE_CLIP，suppress fixedRevealClipRect，" +
                "实际 regions 为空 → CrossfadeOld 分支按 role 判断不 suppress，底图也画 fixedClip 区域 → 双影",
            regions.isEmpty(),
        )
        if (regions.isNotEmpty()) {
            // suppress 的区域应覆盖 fixedClip（让动画 slice 独占 fixedClip）
            val coversFixedClip =
                regions.any { r ->
                    r.left <= fixedClip.left + eps && r.right >= fixedClip.right - eps
                }
            assertTrue(
                "Delete -> CrossfadeOld：suppression 区域应覆盖 fixedRevealClipRect=$fixedClip，" +
                    "实际 regions=$regions → 底图在 fixedClip 区域双影",
                coversFixedClip,
            )
        }
    }

    /**
     * 场景 7b：CrossfadeOld -> Delete 继续旧 NONE。
     *
     * 旧 CrossfadeOld alpha 淡出中，不 suppress 任何区域（NONE，底图画完整字，动画 slice alpha 混合）。
     * rebase 成 Delete 后，Delete 不应因 role 变了瞬间切换底图 ownership。应继续 NONE（不 suppress），
     * 让底图继续画完整字，动画 slice 继续淡出。
     *
     * 当前 buggy：Delete 分支按 role 判断，如果有 revealSpec 会 suppress 吞字区域（VISIBLE_CLIP），
     * 瞬间从 NONE 切换到 VISIBLE_CLIP → 底图突然挖洞。
     *
     * 注意：当前 AnimatedSlice 没有 staticSuppressionMode 字段，Delete slice 带 revealSpec 就会
     * suppress。本测试构造一个带 revealSpec 的 Delete slice（模拟 rebase 后 planner 给了 revealSpec
     * 但应继续旧 NONE 的情形），断言不应 suppress。当前 buggy 会 suppress → FAIL。
     *
     * 修复后：renderer 应按 slice.staticSuppressionMode 判断，Delete continuation 继承旧 CrossfadeOld
     * 的 NONE，不 suppress（即使有 revealSpec）。
     */
    @Test
    fun repro7b_crossfadeOldToDelete_suppressionShouldContinueNoneNotSwitchToVisibleClip() {
        val dest = RectF(0f, 0f, 100f, 20f)
        // rebase 后的 Delete slice，带 revealSpec=SWALLOW（planner 给的），但没有 fixedRevealClipRect
        // （旧 CrossfadeOld 是 alpha 淡出，没有固定 clip）。按 issue 缺陷 4，此 slice 的
        // staticSuppressionMode 应继续旧 CrossfadeOld 的 NONE，不 suppress。
        val swallowSpec =
            TextRevealSpec(
                mode = TextRevealMode.SWALLOW,
                anchorX = 0f,
                boundaryFromX = 100f,
                boundaryToX = 0f,
                progressStart = 0f,
                progressEnd = 1f,
                initialFraction = 0f,
            )
        val deleteSlice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Delete,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 0.5f,
                endAlpha = 0f,
                clusterByteStart = 0,
                clusterByteEndExclusive = 1,
                revealSpec = swallowSpec,
                // #639 评论 5427812180 缺陷4：mapped rebase 继续旧 CrossfadeOld 的 NONE，
                // 不因新 role=Delete 瞬间切换到 VISIBLE_CLIP。
                staticSuppressionMode = StaticSuppressionMode.NONE,
            )
        val transaction = makeTransaction(listOf(deleteSlice))

        val regions = renderer.computeStaticSuppressionRegions(transaction, 0f)

        // 应继续旧 CrossfadeOld 的 NONE，不 suppress（即使 role=Delete 且有 revealSpec）
        assertTrue(
            "CrossfadeOld -> Delete：suppression 应继续旧 CrossfadeOld 的 NONE，不 suppress，" +
                "实际 regions=$regions → Delete 分支按 role 判断 suppress 吞字区域，" +
                "瞬间从 NONE 切换到 VISIBLE_CLIP，底图突然挖洞 → 底图 ownership 瞬间切换",
            regions.isEmpty(),
        )
    }

    /**
     * 场景 7c：mapped rebase 后 role 和 suppression mode 不一致的核心证据。
     *
     * 构造两个 slice，视觉状态相同（都在淡出半截字），但 role 不同（一个 CrossfadeOld 一个 Delete）。
     * 当前 computeStaticSuppressionRegions 按 role 判断会给出不同 suppression 结果，
     * 证明 suppression 按 role 而非按独立 mode — 这是缺陷 4 的核心。
     *
     * 修复后：两个 slice 的 staticSuppressionMode 应相同（都是 VISIBLE_CLIP 或都是 NONE），
     * suppression 结果应一致。
     */
    @Test
    fun repro7c_sameVisualStateDifferentRole_suppressionShouldNotDivergeByRole() {
        val dest = RectF(0f, 0f, 100f, 20f)
        val fixedClip = RectF(0f, 0f, 60f, 20f)

        // 两个 slice 视觉状态相同：都带 fixedRevealClipRect 淡出半截字。
        // #639 评论 5427812180 缺陷4：mapped rebase 继续旧视觉轨后，两个 slice 的
        // staticSuppressionMode 应相同（都 VISIBLE_CLIP），suppression 结果应一致。
        val crossfadeOldSlice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.CrossfadeOld,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 0.5f,
                endAlpha = 0f,
                fixedRevealClipRect = fixedClip,
                staticSuppressionMode = StaticSuppressionMode.VISIBLE_CLIP,
            )
        val deleteSlice =
            PreparedVisualTransaction.AnimatedSlice(
                role = SliceRole.Delete,
                snapshot = null,
                sourceRect = Rect(0, 0, 100, 20),
                destinationRect = dest,
                startAlpha = 0.5f,
                endAlpha = 0f,
                fixedRevealClipRect = fixedClip,
                staticSuppressionMode = StaticSuppressionMode.VISIBLE_CLIP,
            )

        val regionsCrossfadeOld =
            renderer.computeStaticSuppressionRegions(
                makeTransaction(listOf(crossfadeOldSlice)),
                0f,
            )
        val regionsDelete = renderer.computeStaticSuppressionRegions(makeTransaction(listOf(deleteSlice)), 0f)

        // 两个视觉状态相同的 slice，suppression 结果应一致（都 suppress fixedClip 或都不 suppress）
        val sameSuppression = (regionsCrossfadeOld.isEmpty() == regionsDelete.isEmpty())
        assertTrue(
            "mapped rebase 后视觉状态相同（都带 fixedClip 淡出半截字）的 slice，suppression 应一致，" +
                "但按 role 判断会分歧：CrossfadeOld regions=$regionsCrossfadeOld (按 role 不 suppress), " +
                "Delete regions=$regionsDelete (按 role suppress fixedClip) → " +
                "role 变了瞬间切换底图 ownership，这是缺陷 4 的核心",
            sameSuppression,
        )
    }
}
