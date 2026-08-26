package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #639 评论 5427812180 复现测试 — mapped rebase 视觉轨未正交化。
 *
 * 上一轮修复（d48681ff）做对了 unmapped continuation，但 mapped rebase（applyRebaseState）
 * 还没同步到同一套结构。本测试覆盖 issue 指出的 6 个 mapped rebase 场景：
 *
 * 1. Delete -> Delete：旧 SWALLOW=0.4, currentAlpha=1, targetAlpha=1, mapped 后仍 SWALLOW
 *    initial=0.4, endAlpha=1，不能变成 0。当前 buggy：Delete 分支写死 endAlpha=0f。
 * 2. Delete -> CrossfadeOld：旧半吞字第一帧可见范围完全不变；第二次 rebase 后仍继续，不补整字。
 *    当前 buggy：CrossfadeOld 分支旧 role=Delete 走 else 不算 fixed clip。
 * 3. CrossfadeOld(fixedClip) -> CrossfadeOld：fixed clip 保留。
 *    当前 buggy：else 分支不复制 fixedRevealClipRect。
 * 4. CrossfadeOld(fixedClip) -> Delete：第一帧 fixed clip 保留，不能重启完整 SWALLOW。
 *    当前 buggy：Delete 分支不消费 fixedRevealClipRect，revealFraction==null 时 swallow 从 0f 重新开始。
 * 5. 场景 2/4 分别再做一次第三次 rebase，确认 targetAlpha/revealMode/fixedClip 都继续存在。
 * 6. mapped disappearing slice 的 destination 改变时：第一帧从旧 currentRect 开始，fixed clip 跟 bitmap 一起移动。
 *
 * 本测试断言 issue 描述的期望（修复后）行为。在当前 buggy 代码（基线 d48681ff）上这些断言会失败，
 * 从而证明 bug 存在。测试能编译通过（即使断言失败），Phase B 修复后可直接转成回归测试。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RebasePlannerMappedRebaseReproTest {
    private val planner = RebasePlanner()

    // ---- helpers ----

    /**
     * 正常 Delete planner 创建的 slice：startAlpha=1f, endAlpha=1f, revealSpec=SWALLOW
     * （只吞字不淡出）。这是 issue 缺陷 1 描述的"正常 Delete planner 本来是"的形态。
     */
    private fun makeDeleteSlice(destinationRect: RectF): PreparedVisualTransaction.AnimatedSlice {
        return PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Delete,
            snapshot = null,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = destinationRect,
            startAlpha = 1f,
            endAlpha = 1f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 1,
            revealSpec =
                TextRevealSpec(
                    mode = TextRevealMode.SWALLOW,
                    anchorX = destinationRect.left,
                    boundaryFromX = destinationRect.right,
                    boundaryToX = destinationRect.left,
                    progressStart = 0f,
                    progressEnd = 1f,
                    initialFraction = 0f,
                ),
            caretRevealGeometry =
                PreparedVisualTransaction.CaretRevealGeometry(
                    visualRect = destinationRect,
                    caretStartX = destinationRect.left,
                    caretEndX = destinationRect.right,
                ),
        )
    }

    private fun makeCrossfadeOldSlice(destinationRect: RectF): PreparedVisualTransaction.AnimatedSlice {
        return PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.CrossfadeOld,
            snapshot = null,
            sourceRect = Rect(0, 0, 100, 20),
            destinationRect = destinationRect,
            startAlpha = 1f,
            endAlpha = 0f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 1,
        )
    }

    /**
     * 构造 SliceVisualState。currentRect == destinationRect（字在位置上），
     * 模拟旧 slice 已到位但外观状态（reveal/alpha/fixedClip）可能未走完。
     */
    @Suppress("LongParameterList")
    private fun makeRebaseState(
        role: SliceRole,
        rect: RectF,
        currentAlpha: Float = 1f,
        targetAlpha: Float = currentAlpha,
        revealMode: TextRevealMode? = null,
        revealFraction: Float? = null,
        fixedRevealClipRect: RectF? = null,
    ): SliceVisualState {
        return SliceVisualState(
            snapshotId = 1L,
            role = role,
            lineIndex = 0,
            documentByteStart = 0,
            documentByteEndExclusive = 10,
            clusterByteStart = 0,
            clusterByteEndExclusive = 1,
            currentLeft = rect.left,
            currentTop = rect.top,
            currentRight = rect.right,
            currentBottom = rect.bottom,
            currentAlpha = currentAlpha,
            destinationLeft = rect.left,
            destinationTop = rect.top,
            destinationRight = rect.right,
            destinationBottom = rect.bottom,
            targetAlpha = targetAlpha,
            revealMode = revealMode,
            revealFraction = revealFraction,
            remainingFraction = 1f,
            fixedRevealClipRect = fixedRevealClipRect,
            caretRevealGeometry =
                PreparedVisualTransaction.CaretRevealGeometry(
                    visualRect = rect,
                    caretStartX = rect.left,
                    caretEndX = rect.right,
                ),
        )
    }

    /**
     * 模拟 AndroidTextAnimationEngine.computeSliceVisualStates 从 active slice 保存
     * SliceVisualState。把 rebased slice 转成 state 供下一次 rebase 使用。
     */
    private fun sliceToState(
        slice: PreparedVisualTransaction.AnimatedSlice,
        currentAlpha: Float = slice.startAlpha,
        revealFraction: Float? = slice.revealSpec?.initialFraction,
    ): SliceVisualState {
        return SliceVisualState(
            snapshotId = slice.snapshot?.snapshotId ?: 1L,
            role = slice.role,
            lineIndex = 0,
            documentByteStart = 0,
            documentByteEndExclusive = 10,
            clusterByteStart = slice.clusterByteStart,
            clusterByteEndExclusive = slice.clusterByteEndExclusive,
            currentLeft = slice.destinationRect.left,
            currentTop = slice.destinationRect.top,
            currentRight = slice.destinationRect.right,
            currentBottom = slice.destinationRect.bottom,
            currentAlpha = currentAlpha,
            destinationLeft = slice.destinationRect.left,
            destinationTop = slice.destinationRect.top,
            destinationRight = slice.destinationRect.right,
            destinationBottom = slice.destinationRect.bottom,
            sourceRect = Rect(slice.sourceRect),
            targetAlpha = slice.endAlpha,
            revealMode = slice.revealSpec?.mode,
            revealFraction = revealFraction,
            remainingFraction = 1f,
            fixedRevealClipRect = slice.fixedRevealClipRect?.let { RectF(it) },
            caretRevealGeometry = slice.caretRevealGeometry,
        )
    }

    // ---- 场景 1：Delete -> Delete，endAlpha 应继承旧 targetAlpha=1，不能变成 0 ----

    /**
     * 场景：旧 Delete 已 swallow 到 40%（revealMode=SWALLOW, revealFraction=0.4），
     * currentAlpha=1, targetAlpha=1（正常 Delete 只吞字不淡出）。rebase 成新 Delete。
     *
     * 期望（修复后，issue 缺陷 1）：mapped 后仍 SWALLOW initial=0.4, endAlpha=1，
     * 不能变成 0。新事务从 40% 继续 swallow，alpha 保持 1。
     *
     * 当前 buggy：applyRebaseState Delete 分支写死 endAlpha=0f（RebasePlanner.kt line 327）。
     * 上一轮 renderer 已改成 reveal 和 alpha 正交同时生效，以前"无效"的 endAlpha=0f 现在真的
     * 会生效，导致删除动画突然变暗，且下一次 SliceVisualState.targetAlpha 被保存成 0，
     * 之后再 rebase 沿错误 alpha 轨走。
     */
    @Test
    fun repro1_deleteToDelete_endAlphaShouldInheritTargetAlphaNotZero() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val slice = makeDeleteSlice(rect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Delete,
                rect = rect,
                currentAlpha = 1f,
                targetAlpha = 1f,
                revealMode = TextRevealMode.SWALLOW,
                revealFraction = 0.4f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // SWALLOW initial 应继承旧 revealFraction=0.4
        assertNotNull(
            "Delete -> Delete：新 Delete 应携带 revealSpec 继续 swallow，实际 revealSpec=null",
            rebased.revealSpec,
        )
        if (rebased.revealSpec != null) {
            assertEquals(
                "Delete -> Delete：新 Delete revealSpec.initialFraction 应为旧 revealFraction=0.4，" +
                    "实际 initialFraction=${rebased.revealSpec!!.initialFraction}",
                0.4f,
                rebased.revealSpec!!.initialFraction,
                0.001f,
            )
            assertEquals(
                "Delete -> Delete：新 Delete revealSpec.mode 应继续 SWALLOW",
                TextRevealMode.SWALLOW,
                rebased.revealSpec!!.mode,
            )
        }
        // endAlpha 应继承旧 targetAlpha=1，不能变成 0
        assertEquals(
            "Delete -> Delete：新 Delete endAlpha 应继承旧 targetAlpha=1f（正常 Delete 只吞字不淡出），" +
                "实际 endAlpha=${rebased.endAlpha} → 上一轮 renderer 已让 endAlpha=0f 真正生效，" +
                "删除动画会突然变暗，且下一次 targetAlpha 被保存成 0，之后再 rebase 沿错误 alpha 轨走",
            1f,
            rebased.endAlpha,
            0.001f,
        )
        // startAlpha 应继承旧 currentAlpha=1
        assertEquals(
            "Delete -> Delete：新 Delete startAlpha 应继承旧 currentAlpha=1f",
            1f,
            rebased.startAlpha,
            0.001f,
        )
    }

    // ---- 场景 2：Delete -> CrossfadeOld，旧半吞字第一帧可见范围完全不变 ----

    /**
     * 场景：旧 Delete swallow 到一半（revealMode=SWALLOW, revealFraction=0.4），
     * currentAlpha=1, targetAlpha=1。rebase 成新 CrossfadeOld。
     *
     * 期望（修复后，issue 缺陷 2）：旧半吞字第一帧可见范围完全不变。新 CrossfadeOld 应携带
     * fixedRevealClipRect（冻结旧 Delete 的半吞可见部分），第一帧 clip 与旧帧完全一致，
     * 只让 alpha 从 1 淡出到 0，不补整字。
     *
     * 当前 buggy：CrossfadeOld 分支只有旧 role 是 Move/CrossfadeNew/Insert 时才算 fixed clip
     * （RebasePlanner.kt line 290-293），旧 role=Delete 走 else（line 304-311）直接 endAlpha=0f，
     * 不处理 reveal。结果：旧帧剩 60% 的字，新事务第一帧又变回完整字再淡出 → 闪。
     */
    @Test
    fun repro2_deleteToCrossfadeOld_halfSwallowedCharShouldNotRefillAtFirstFrame() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val slice = makeCrossfadeOldSlice(rect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Delete,
                rect = rect,
                currentAlpha = 1f,
                targetAlpha = 1f,
                revealMode = TextRevealMode.SWALLOW,
                revealFraction = 0.4f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // 新 CrossfadeOld 应携带 fixedRevealClipRect 冻结旧 Delete 的半吞可见部分
        assertNotNull(
            "Delete -> CrossfadeOld：旧 Delete swallow 到 0.4（剩 60% 的字），新 CrossfadeOld 应携带 " +
                "fixedRevealClipRect 冻结半吞可见部分，实际 fixedRevealClipRect=null → " +
                "新事务第一帧画完整字再淡出，旧帧剩 60% 的字瞬间补全 → 闪",
            rebased.fixedRevealClipRect,
        )
        // startAlpha 应继承旧 currentAlpha=1
        assertEquals(
            "Delete -> CrossfadeOld：新 CrossfadeOld startAlpha 应继承旧 currentAlpha=1f",
            1f,
            rebased.startAlpha,
            0.001f,
        )
    }

    // ---- 场景 3：CrossfadeOld(fixedClip) -> CrossfadeOld，fixed clip 保留 ----

    /**
     * 场景：旧 CrossfadeOld 带固定 clip（fixedRevealClipRect=半截字 clip），currentAlpha=0.5，
     * 正在淡出。rebase 成新 CrossfadeOld。
     *
     * 期望（修复后，issue 缺陷 2）：fixed clip 保留。新 CrossfadeOld 应继承旧 fixedRevealClipRect，
     * 冻结半截字继续淡出，下一笔输入时不瞬间补成完整字。
     *
     * 当前 buggy：CrossfadeOld 分支旧 role=CrossfadeOld 走 else（line 304-311）不复制
     * fixedRevealClipRect。结果：冻结半截字下一笔输入时又瞬间补成完整字。
     */
    @Test
    fun repro3_crossfadeOldWithFixedClipToCrossfadeOld_fixedClipShouldBePreserved() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val slice = makeCrossfadeOldSlice(rect)
        val fixedClip = RectF(0f, 0f, 60f, 20f) // 旧 Insert reveal 到 60% 冻结的半截字
        val rebaseState =
            makeRebaseState(
                role = SliceRole.CrossfadeOld,
                rect = rect,
                currentAlpha = 0.5f,
                targetAlpha = 0f,
                fixedRevealClipRect = fixedClip,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull(
            "CrossfadeOld(fixedClip) -> CrossfadeOld：新 CrossfadeOld 应继承旧 fixedRevealClipRect，" +
                "实际 fixedRevealClipRect=null → 冻结半截字下一笔输入时又瞬间补成完整字",
            rebased.fixedRevealClipRect,
        )
        if (rebased.fixedRevealClipRect != null) {
            assertEquals(
                "CrossfadeOld(fixedClip) -> CrossfadeOld：fixedRevealClipRect 应与旧值一致",
                fixedClip,
                rebased.fixedRevealClipRect,
            )
        }
        // startAlpha 应继承旧 currentAlpha=0.5
        assertEquals(
            "CrossfadeOld(fixedClip) -> CrossfadeOld：startAlpha 应继承旧 currentAlpha=0.5f",
            0.5f,
            rebased.startAlpha,
            0.001f,
        )
    }

    // ---- 场景 4：CrossfadeOld(fixedClip) -> Delete，第一帧 fixed clip 保留，不重启完整 SWALLOW ----

    /**
     * 场景：旧 CrossfadeOld 带固定 clip（fixedRevealClipRect=半截字 clip），currentAlpha=0.5。
     * rebase 成新 Delete。
     *
     * 期望（修复后，issue 缺陷 2）：第一帧 fixed clip 保留，不能重启完整 SWALLOW。
     * 新 Delete 应继承旧 fixedRevealClipRect，继续画半截字，不从 0f 重新 swallow。
     *
     * 当前 buggy：Delete 分支不消费 fixedRevealClipRect（line 313-331 不处理 fixedClip），
     * 且 `initialFraction = rebaseState.revealFraction ?: 0f`，旧 CrossfadeOld 的 revealFraction=null，
     * 所以 initialFraction=0f，新 Delete swallow 从 0f 重新开始。结果：旧帧半截字 -> 新事务第一帧完整字。
     */
    @Test
    fun repro4_crossfadeOldWithFixedClipToDelete_fixedClipShouldBePreservedNotRestartSwallow() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val slice = makeDeleteSlice(rect)
        val fixedClip = RectF(0f, 0f, 60f, 20f)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.CrossfadeOld,
                rect = rect,
                currentAlpha = 0.5f,
                targetAlpha = 0f,
                fixedRevealClipRect = fixedClip,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        assertNotNull(
            "CrossfadeOld(fixedClip) -> Delete：新 Delete 应继承旧 fixedRevealClipRect，" +
                "实际 fixedRevealClipRect=null → 旧帧半截字 -> 新事务第一帧完整字",
            rebased.fixedRevealClipRect,
        )
        if (rebased.fixedRevealClipRect != null) {
            assertEquals(
                "CrossfadeOld(fixedClip) -> Delete：fixedRevealClipRect 应与旧值一致",
                fixedClip,
                rebased.fixedRevealClipRect,
            )
        }
        // 不应重启完整 SWALLOW：如果有 revealSpec，initialFraction 不应是 0f（应继承旧可见状态）
        if (rebased.revealSpec != null) {
            assertTrue(
                "CrossfadeOld(fixedClip) -> Delete：不应重启完整 SWALLOW（initialFraction 不应是 0f），" +
                    "实际 initialFraction=${rebased.revealSpec!!.initialFraction}",
                rebased.revealSpec!!.initialFraction > 0.001f,
            )
        }
    }

    // ---- 场景 5a：场景 2 (Delete->CrossfadeOld) 第三次 rebase，fixedClip/targetAlpha/revealMode 继续 ----

    /**
     * 场景 2 之后再做一次第三次 rebase（CrossfadeOld -> CrossfadeOld），
     * 确认 targetAlpha/revealMode/fixedClip 都继续存在。
     *
     * 当前 buggy：第一次 rebase（场景 2）就没产生 fixedClip，第二次更不会有。
     */
    @Test
    fun repro5a_deleteToCrossfadeOldToCrossfadeOld_thirdRebaseShouldPreserveFixedClipAndTargetAlpha() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val fixedClip = RectF(0f, 0f, 60f, 20f)

        // 第一次 rebase: Delete -> CrossfadeOld（模拟场景 2 修复后的状态：带 fixedClip）
        val slice1 = makeCrossfadeOldSlice(rect)
        val state0 =
            makeRebaseState(
                role = SliceRole.Delete,
                rect = rect,
                currentAlpha = 1f,
                targetAlpha = 1f,
                revealMode = TextRevealMode.SWALLOW,
                revealFraction = 0.4f,
            )
        val rebased1 = planner.applyRebaseState(slice1, state0, emptyMap())
        // 场景 2 修复后 rebased1 应带 fixedClip；这里用 rebased1 继续往下走
        // 若 rebased1.fixedRevealClipRect==null（当前 buggy），用期望值构造 state1 让第三次 rebase 可执行
        val effectiveFixedClip = rebased1.fixedRevealClipRect ?: fixedClip

        // 第二次 rebase: CrossfadeOld -> CrossfadeOld（第三次 rebase 总计）
        val slice2 = makeCrossfadeOldSlice(rect)
        val state1 =
            makeRebaseState(
                role = SliceRole.CrossfadeOld,
                rect = rect,
                currentAlpha = 0.5f,
                targetAlpha = 0f,
                fixedRevealClipRect = effectiveFixedClip,
            )
        val rebased2 = planner.applyRebaseState(slice2, state1, emptyMap())

        assertNotNull(
            "Delete->CrossfadeOld->CrossfadeOld：第三次 rebase 后 fixedRevealClipRect 应继续存在，" +
                "实际=null → 冻结半截字在后续 rebase 中丢失，瞬间补成完整字",
            rebased2.fixedRevealClipRect,
        )
        assertEquals(
            "Delete->CrossfadeOld->CrossfadeOld：第三次 rebase 后 endAlpha 应继续 0f（淡出到消失）",
            0f,
            rebased2.endAlpha,
            0.001f,
        )
        assertEquals(
            "Delete->CrossfadeOld->CrossfadeOld：第三次 rebase 后 startAlpha 应继承当前 alpha=0.5f",
            0.5f,
            rebased2.startAlpha,
            0.001f,
        )
    }

    // ---- 场景 5b：场景 4 (CrossfadeOld->Delete) 第三次 rebase，fixedClip/revealMode 继续 ----

    /**
     * 场景 4 之后再做一次第三次 rebase（Delete -> Delete），
     * 确认 fixedClip/revealMode/targetAlpha 都继续存在。
     */
    @Test
    fun repro5b_crossfadeOldToDeleteToDelete_thirdRebaseShouldPreserveFixedClipAndRevealMode() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val fixedClip = RectF(0f, 0f, 60f, 20f)

        // 第一次 rebase: CrossfadeOld(fixedClip) -> Delete（模拟场景 4 修复后的状态：带 fixedClip）
        val slice1 = makeDeleteSlice(rect)
        val state0 =
            makeRebaseState(
                role = SliceRole.CrossfadeOld,
                rect = rect,
                currentAlpha = 0.5f,
                targetAlpha = 0f,
                fixedRevealClipRect = fixedClip,
            )
        val rebased1 = planner.applyRebaseState(slice1, state0, emptyMap())
        val effectiveFixedClip = rebased1.fixedRevealClipRect ?: fixedClip

        // 第二次 rebase: Delete -> Delete（第三次 rebase 总计）
        val slice2 = makeDeleteSlice(rect)
        val state1 =
            makeRebaseState(
                role = SliceRole.Delete,
                rect = rect,
                currentAlpha = 0.5f,
                targetAlpha = 1f,
                revealMode = TextRevealMode.SWALLOW,
                revealFraction = 0.4f,
                fixedRevealClipRect = effectiveFixedClip,
            )
        val rebased2 = planner.applyRebaseState(slice2, state1, emptyMap())

        assertNotNull(
            "CrossfadeOld->Delete->Delete：第三次 rebase 后 fixedRevealClipRect 应继续存在，" +
                "实际=null → 冻结半截字在后续 rebase 中丢失",
            rebased2.fixedRevealClipRect,
        )
        // endAlpha 应继承 targetAlpha=1，不能变成 0（同场景 1）
        assertEquals(
            "CrossfadeOld->Delete->Delete：第三次 rebase 后 endAlpha 应继承 targetAlpha=1f，" +
                "实际 endAlpha=${rebased2.endAlpha}",
            1f,
            rebased2.endAlpha,
            0.001f,
        )
    }

    // ---- 场景 6：mapped disappearing slice destination 改变时，第一帧从旧 currentRect 开始 ----

    /**
     * 场景：旧 Delete currentRect=A=(0,0,100,20)，destinationRect=A，带 fixedRevealClipRect。
     * 新 Delete slice 的 destinationRect=B=(200,0,300,20)（位置改变）。
     *
     * 期望（修复后，issue 缺陷 5）：第一帧从旧 currentRect=A 开始（fromDestinationRect=A），
     * fixed clip 跟 bitmap 一起移动（clip 应相对于 currentRect，不是钉在 A 的绝对坐标）。
     *
     * 当前 buggy：Delete 分支不设置 fromDestinationRect（保持 null）→ 第一帧从 destinationRect=B
     * 开始，位置跳变。且不继承 fixedRevealClipRect → clip 丢失。
     */
    @Test
    fun repro6_mappedDeleteDestinationChanged_firstFrameShouldStartFromOldCurrentRectAndClipFollows() {
        val oldRect = RectF(0f, 0f, 100f, 20f)
        val newDestRect = RectF(200f, 0f, 300f, 20f)
        val fixedClipAtOld = RectF(0f, 0f, 60f, 20f) // 旧帧 fixed clip 在 oldRect 坐标系

        val slice = makeDeleteSlice(newDestRect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Delete,
                rect = oldRect,
                currentAlpha = 1f,
                targetAlpha = 1f,
                revealMode = TextRevealMode.SWALLOW,
                revealFraction = 0.4f,
                fixedRevealClipRect = fixedClipAtOld,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // 第一帧应从旧 currentRect=oldRect 开始，不是新 destinationRect
        assertNotNull(
            "mapped Delete destination 改变：fromDestinationRect 应为旧 currentRect=oldRect，" +
                "实际=null → 第一帧从新 destinationRect 开始，位置跳变",
            rebased.fromDestinationRect,
        )
        if (rebased.fromDestinationRect != null) {
            assertEquals(
                "fromDestinationRect.left 应为旧 currentRect.left=0f",
                0f,
                rebased.fromDestinationRect!!.left,
                0.001f,
            )
            assertEquals(
                "fromDestinationRect.right 应为旧 currentRect.right=100f",
                100f,
                rebased.fromDestinationRect!!.right,
                0.001f,
            )
        }
        // fixed clip 应继承（跟 bitmap 一起移动）
        assertNotNull(
            "mapped Delete destination 改变：fixedRevealClipRect 应继承旧值（跟 bitmap 一起移动），" +
                "实际=null → clip 丢失",
            rebased.fixedRevealClipRect,
        )
    }

    // ---- 场景 6b：mapped CrossfadeOld destination 改变时，第一帧从旧 currentRect 开始 ----

    /**
     * 补充：CrossfadeOld 的 fromDestinationRect 在旧 role=Move/CrossfadeNew/Insert 时
     * 会设置（line 298 destinationRect = fromRect），但旧 role=Delete 走 else 不设置。
     * mapped CrossfadeOld destination 改变时也应从旧 currentRect 开始。
     */
    @Test
    fun repro6b_mappedCrossfadeOldDestinationChanged_firstFrameShouldStartFromOldCurrentRect() {
        val oldRect = RectF(0f, 0f, 100f, 20f)
        val newDestRect = RectF(200f, 0f, 300f, 20f)

        val slice = makeCrossfadeOldSlice(newDestRect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Delete,
                rect = oldRect,
                currentAlpha = 1f,
                targetAlpha = 1f,
                revealMode = TextRevealMode.SWALLOW,
                revealFraction = 0.4f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // CrossfadeOld 分支旧 role=Move/CrossfadeNew/Insert 时设 destinationRect=fromRect，
        // 但旧 role=Delete 走 else 不设。mapped destination 改变时应从旧 currentRect 开始。
        // 注意：CrossfadeOld 的 destinationRect 在 fixed clip 分支被设成 fromRect（line 298），
        // 这里验证 fromDestinationRect 或 destinationRect 反映旧 currentRect。
        val reflectsOldRect =
            rebased.fromDestinationRect != null ||
                (rebased.destinationRect.left == oldRect.left && rebased.destinationRect.right == oldRect.right)
        assertTrue(
            "mapped CrossfadeOld destination 改变：第一帧应从旧 currentRect=oldRect 开始，" +
                "实际 fromDestinationRect=${rebased.fromDestinationRect}, " +
                "destinationRect=${rebased.destinationRect} → 位置跳变",
            reflectsOldRect,
        )
    }
}
