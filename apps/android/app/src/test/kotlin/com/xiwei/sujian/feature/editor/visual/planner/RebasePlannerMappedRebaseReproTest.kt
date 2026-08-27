package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.render.AndroidTextAnimationRenderer
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
import com.xiwei.sujian.feature.editor.visual.StaticSuppressionMode
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
        fixedClipBaseRect: RectF? = null,
        staticSuppressionMode: com.xiwei.sujian.feature.editor.visual.StaticSuppressionMode? = null,
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
            staticSuppressionMode = staticSuppressionMode,
            fixedClipBaseRect = fixedClipBaseRect,
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
        // #639 评论 5428952431 缺陷1：CrossfadeOld 的 endAlpha 必须是 0f（"旧像素必须退出"的目标语义），
        // 不能继承旧 Delete 的 targetAlpha=1f。否则旧像素永远不会淡出消失。
        assertEquals(
            "Delete -> CrossfadeOld：新 CrossfadeOld endAlpha 必须是 0f（旧像素必须退出），" +
                "实际 endAlpha=${rebased.endAlpha} → CrossfadeOld 永远不淡出，旧像素残留",
            0f,
            rebased.endAlpha,
            0.001f,
        )
    }

    // ---- 场景 2b：Move(targetAlpha=1) -> CrossfadeOld，startAlpha=1, endAlpha=0 ----

    /**
     * #639 评论 5428952431 缺陷1：旧 Move（currentAlpha=1, targetAlpha=1，Move 不淡出）
     * rebase 成新 CrossfadeOld。
     *
     * 期望：startAlpha 继承旧 currentAlpha=1（当前屏幕真实 alpha），endAlpha=0f
     * （CrossfadeOld 是"旧像素必须退出"的目标语义，必须来自新 slice 而非旧 targetAlpha）。
     * 旧 targetAlpha=1 不能被继承成 endAlpha，否则 Move -> CrossfadeOld 后旧像素永远不淡出。
     */
    @Test
    fun repro2b_moveToCrossfadeOld_endAlphaShouldBeZeroNotInheritMoveTargetAlpha() {
        val rect = RectF(0f, 0f, 100f, 20f)
        // 新 slice 是 CrossfadeOld（旧像素必须退出）；旧 state 是 Move（alpha 保持 1，不淡出）
        val crossfadeOldSlice = makeCrossfadeOldSlice(rect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Move,
                rect = rect,
                currentAlpha = 1f,
                targetAlpha = 1f,
            )

        val rebased = planner.applyRebaseState(crossfadeOldSlice, rebaseState, emptyMap())

        // startAlpha 应继承旧 currentAlpha=1
        assertEquals(
            "Move -> CrossfadeOld：新 CrossfadeOld startAlpha 应继承旧 currentAlpha=1f",
            1f,
            rebased.startAlpha,
            0.001f,
        )
        // endAlpha 必须是 0f（CrossfadeOld 目标语义），不能继承旧 Move targetAlpha=1f
        assertEquals(
            "Move -> CrossfadeOld：新 CrossfadeOld endAlpha 必须是 0f（旧像素必须退出），" +
                "实际 endAlpha=${rebased.endAlpha} → 继承了旧 Move targetAlpha=1f，旧像素永远不淡出",
            0f,
            rebased.endAlpha,
            0.001f,
        )
    }

    // ---- 场景 2c：Insert(reveal=0.4,targetAlpha=1) -> CrossfadeOld，fixed clip 非空且 endAlpha=0 ----

    /**
     * #639 评论 5428952431 缺陷1：旧 Insert reveal 到 40%（revealMode=REVEAL, revealFraction=0.4），
     * currentAlpha=1, targetAlpha=1（Insert 不淡出）。rebase 成新 CrossfadeOld。
     *
     * 期望：新 CrossfadeOld 应携带 fixedRevealClipRect（冻结旧 Insert 的半 reveal 可见部分），
     * 且 endAlpha=0f（CrossfadeOld 目标语义），不能继承旧 Insert targetAlpha=1f。
     */
    @Test
    fun repro2c_insertToCrossfadeOld_fixedClipNonEmptyAndEndAlphaZero() {
        val rect = RectF(0f, 0f, 100f, 20f)
        // 新 slice 是 CrossfadeOld（旧像素必须退出）；旧 state 是 Insert（reveal 到 0.4，alpha 保持 1）
        val crossfadeOldSlice = makeCrossfadeOldSlice(rect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Insert,
                rect = rect,
                currentAlpha = 1f,
                targetAlpha = 1f,
                revealMode = TextRevealMode.REVEAL,
                revealFraction = 0.4f,
            )

        val rebased = planner.applyRebaseState(crossfadeOldSlice, rebaseState, emptyMap())

        // 新 CrossfadeOld 应携带 fixedRevealClipRect 冻结旧 Insert 的半 reveal 可见部分
        assertNotNull(
            "Insert -> CrossfadeOld：旧 Insert reveal 到 0.4，新 CrossfadeOld 应携带 " +
                "fixedRevealClipRect 冻结半 reveal 可见部分，实际 fixedRevealClipRect=null → " +
                "新事务第一帧画完整字再淡出，旧帧半 reveal 的字瞬间补全 → 闪",
            rebased.fixedRevealClipRect,
        )
        // endAlpha 必须是 0f（CrossfadeOld 目标语义），不能继承旧 Insert targetAlpha=1f
        assertEquals(
            "Insert -> CrossfadeOld：新 CrossfadeOld endAlpha 必须是 0f（旧像素必须退出），" +
                "实际 endAlpha=${rebased.endAlpha} → 继承了旧 Insert targetAlpha=1f，旧像素永远不淡出",
            0f,
            rebased.endAlpha,
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

    // ---- 场景 6b：mapped CrossfadeOld 位置必须锁死在 oldCurrentRect，禁止移动 ----

    /**
     * #639 评论 5433268179：旧 Move(currentRect=[50,150]) rebase 成 CrossfadeOld。
     *
     * CrossfadeOld 是"旧像素原地退场"，位置必须锁在 rebase 当下的 oldCurrentRect，
     * 禁止旧字从旧行斜飞到新行。新 planner 的 CrossfadeOld.destinationRect 被忽略。
     *
     * 断言：
     * - destinationRect == oldCurrentRect
     * - fromDestinationRect == null
     * - visualDestinationRectAt(0f) == oldCurrentRect
     * - visualDestinationRectAt(0.5f) == oldCurrentRect
     * - visualDestinationRectAt(1f) == oldCurrentRect
     * - startAlpha == old currentAlpha
     * - endAlpha == 0f
     */
    @Test
    fun repro6b_mappedCrossfadeOldDestinationChanged_positionLockedAtOldCurrentRect() {
        val oldCurrentRect = RectF(50f, 0f, 150f, 20f)
        val newPlannerDestRect = RectF(100f, 0f, 200f, 20f)

        // 旧 Move 正在从 [0,100] 移动到 [50,150]，当前在 [50,150]
        val slice = makeCrossfadeOldSlice(newPlannerDestRect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Move,
                rect = oldCurrentRect,
                currentAlpha = 0.8f,
                targetAlpha = 1f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // destinationRect 必须锁在 oldCurrentRect，不能是新 planner 的 destination
        assertEquals(
            "CrossfadeOld destinationRect.left 必须锁在 oldCurrentRect.left=50f",
            oldCurrentRect.left,
            rebased.destinationRect.left,
            0.001f,
        )
        assertEquals(
            "CrossfadeOld destinationRect.right 必须锁在 oldCurrentRect.right=150f",
            oldCurrentRect.right,
            rebased.destinationRect.right,
            0.001f,
        )
        // fromDestinationRect 必须为 null（禁止位置运动）
        assertEquals(
            "CrossfadeOld fromDestinationRect 必须为 null（原地退场，禁止移动）",
            null,
            rebased.fromDestinationRect,
        )
        // visualDestinationRectAt 在所有 progress 都必须是 oldCurrentRect
        assertEquals(
            "CrossfadeOld visualDestinationRectAt(0f) 必须是 oldCurrentRect",
            oldCurrentRect.left,
            rebased.visualDestinationRectAt(0f).left,
            0.001f,
        )
        assertEquals(
            "CrossfadeOld visualDestinationRectAt(0.5f) 必须是 oldCurrentRect",
            oldCurrentRect.left,
            rebased.visualDestinationRectAt(0.5f).left,
            0.001f,
        )
        assertEquals(
            "CrossfadeOld visualDestinationRectAt(1f) 必须是 oldCurrentRect",
            oldCurrentRect.left,
            rebased.visualDestinationRectAt(1f).left,
            0.001f,
        )
        // alpha 轨：startAlpha 继承旧 currentAlpha，endAlpha=0f
        assertEquals(
            "CrossfadeOld startAlpha 应继承旧 currentAlpha=0.8f",
            0.8f,
            rebased.startAlpha,
            0.001f,
        )
        assertEquals(
            "CrossfadeOld endAlpha 必须是 0f",
            0f,
            rebased.endAlpha,
            0.001f,
        )
    }

    // ---- 场景 6c：Insert(reveal=0.4) -> CrossfadeOld，fixed clip 稳定不移动 ----

    /**
     * #639 评论 5433268179：旧 Insert 正在 reveal 到 40%，currentRect 正在从 from 移动到
     * destination 中间。rebase 成 CrossfadeOld 后：
     *
     * - 位置锁在 oldCurrentRect（原地退场）
     * - fixed clip 第一帧必须是 rebase 当下的 effective clip
     * - progress 0 / 0.5 / 1 的 effective fixed clip 坐标都不变
     * - 只允许 alpha 下降，不允许 clip/bitmap 继续平移
     */
    @Test
    fun repro6c_insertRevealingToCrossfadeOld_fixedClipStableNoMovement() {
        val oldCurrentRect = RectF(30f, 0f, 130f, 20f)
        val newPlannerDestRect = RectF(200f, 0f, 300f, 20f)

        val slice = makeCrossfadeOldSlice(newPlannerDestRect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Insert,
                rect = oldCurrentRect,
                currentAlpha = 1f,
                targetAlpha = 1f,
                revealMode = TextRevealMode.REVEAL,
                revealFraction = 0.4f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // 位置锁在 oldCurrentRect
        assertEquals(
            "Insert->CrossfadeOld destinationRect.left 必须锁在 oldCurrentRect.left=30f",
            oldCurrentRect.left,
            rebased.destinationRect.left,
            0.001f,
        )
        assertEquals(
            "Insert->CrossfadeOld destinationRect.right 必须锁在 oldCurrentRect.right=130f",
            oldCurrentRect.right,
            rebased.destinationRect.right,
            0.001f,
        )
        assertEquals(
            "Insert->CrossfadeOld fromDestinationRect 必须为 null",
            null,
            rebased.fromDestinationRect,
        )
        // fixed clip 必须存在（冻结旧 Insert 的半 reveal 可见部分）
        assertNotNull(
            "Insert->CrossfadeOld 应携带 fixedRevealClipRect 冻结半 reveal 可见部分",
            rebased.fixedRevealClipRect,
        )
        // fixedClipBaseRect 必须为 null（CrossfadeOld 不移动，不需要 base）
        assertEquals(
            "Insert->CrossfadeOld fixedClipBaseRect 必须为 null（不移动）",
            null,
            rebased.fixedClipBaseRect,
        )
        // visualDestinationRectAt 在所有 progress 都必须是 oldCurrentRect
        assertEquals(
            "Insert->CrossfadeOld visualDestinationRectAt(0.5f) 必须是 oldCurrentRect",
            oldCurrentRect.left,
            rebased.visualDestinationRectAt(0.5f).left,
            0.001f,
        )
        // alpha 轨：startAlpha=1f（继承旧 currentAlpha），endAlpha=0f
        assertEquals(
            "Insert->CrossfadeOld startAlpha 应继承旧 currentAlpha=1f",
            1f,
            rebased.startAlpha,
            0.001f,
        )
        assertEquals(
            "Insert->CrossfadeOld endAlpha 必须是 0f",
            0f,
            rebased.endAlpha,
            0.001f,
        )
    }

    // ---- 场景 7：第二次带 base 的 mapped rebase 不应把之前累计的平移清零 ----

    /**
     * #639 评论 5428952431 缺陷2：mapped fixed clip 先归一化成 effective clip 再建立新 base。
     *
     * 场景：旧 state 已带 raw fixedRevealClipRect=[0,60] 和 fixedClipBaseRect=[0,100]，
     * currentRect=[50,150]。rebase 前 effective clip = raw + (currentRect.left - base.left)
     * = [0,60] + (50-0) = [50,110]。再 mapped 到 destination=[200,300]。
     *
     * 期望（修复后）：新事务 progress=0 时 renderer/suppression 算出的 effective clip
     * 仍必须是 [50,110]，不能因为第二次 mapped rebase 把之前累计的平移清零。
     *
     * 当前 buggy：mapped rebase 直接继承旧 raw fixedRevealClipRect=[0,60] 和旧
     * fixedClipBaseRect=[0,100]，新 base=oldCurrentRect=[50,150]，renderer progress=0:
     * effectiveClip = [0,60] + (50-50) = [0,60] → 平移丢失。
     *
     * 通过 AndroidTextAnimationRenderer.computeStaticSuppressionRegions(transaction, 0f) 验证。
     */
    @Test
    fun repro7_secondMappedRebaseWithBaseShouldNotResetAccumulatedTranslation() {
        val renderer = AndroidTextAnimationRenderer()
        // 旧 state：raw clip=[0,60], base=[0,100], currentRect=[50,150]
        // rebase 前 effective clip = [0,60] + (50-0) = [50,110]
        val currentRect = RectF(50f, 0f, 150f, 20f) // left=50, right=150
        val newDestRect = RectF(200f, 0f, 300f, 20f) // mapped 到新 destination
        val rebaseState =
            makeRebaseState(
                role = SliceRole.CrossfadeOld,
                rect = currentRect,
                currentAlpha = 0.5f,
                targetAlpha = 0f,
                // raw clip left=0, right=60
                fixedRevealClipRect = RectF(0f, 0f, 60f, 20f),
                // old base left=0, right=100
                fixedClipBaseRect = RectF(0f, 0f, 100f, 20f),
                staticSuppressionMode = StaticSuppressionMode.VISIBLE_CLIP,
            )

        val slice = makeCrossfadeOldSlice(newDestRect)
        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // rebased 应携带 fixedRevealClipRect（归一化后的 effective clip）
        assertNotNull(
            "第二次 mapped rebase：rebased slice 应携带 fixedRevealClipRect，实际=null",
            rebased.fixedRevealClipRect,
        )

        // 构造 transaction，通过 renderer 验证 progress=0 时的 effective clip
        val transaction =
            PreparedVisualTransaction(
                transactionId = 1L,
                oldRevision = null,
                newRevision = null,
                staticPatches = emptyList(),
                animatedSlices = listOf(rebased),
                ownedSnapshotIds = emptySet(),
                referencedSnapshotIds = emptySet(),
                selectionDecoration = null,
                preeditDecoration = null,
                cursorTransition = null,
                durationMs = 300L,
            )
        val regions = renderer.computeStaticSuppressionRegions(transaction, 0f)

        // 应返回一个 region，且 effective clip = [50,110]（不因第二次 mapped rebase 清零平移）
        assertTrue(
            "第二次 mapped rebase：computeStaticSuppressionRegions 应返回非空 region，" +
                "实际 regions=$regions → VISIBLE_CLIP 分支未生效",
            regions.isNotEmpty(),
        )
        if (regions.isNotEmpty()) {
            val region = regions[0]
            assertEquals(
                "第二次 mapped rebase：effective clip.left 应为 50f（rebase 前 effective clip [50,110]），" +
                    "实际 left=${region.left} → 第二次 mapped rebase 把累计平移清零",
                50f,
                region.left,
                0.001f,
            )
            assertEquals(
                "第二次 mapped rebase：effective clip.right 应为 110f（rebase 前 effective clip [50,110]），" +
                    "实际 right=${region.right} → 第二次 mapped rebase 把累计平移清零",
                110f,
                region.right,
                0.001f,
            )
        }
    }

    // ---- 场景 8：unmapped fixed clip + alpha 0->0，不应产生 continuation ----

    /**
     * #639 评论 5428952431 缺陷3：fixed clip 只是裁剪修饰，不是 liveness 轨。
     *
     * 场景：旧 state 带 fixedRevealClipRect，但 alpha 已完成（currentAlpha=0, targetAlpha=0）、
     * position 已完成（currentRect==destinationRect）、reveal 已完成（revealFraction=null）。
     *
     * 期望（修复后）：unmapped result 应为空（不产生 continuation）。fixed clip 不单独
     * 维持 slice 存活，否则会留下透明 slice 继续挖静态正文。
     *
     * 当前 buggy：fixedClipActive = state.fixedRevealClipRect != null = true，
     * !positionRemaining && !alphaRemaining && !revealRemaining && !fixedClipActive = false，
     * 产生 continuation → 透明 slice 继续挖静态正文。
     */
    @Test
    fun repro8_unmappedFixedClipWithAlphaDone_shouldNotProduceContinuation() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val state =
            makeRebaseState(
                role = SliceRole.CrossfadeOld,
                rect = rect,
                currentAlpha = 0f,
                targetAlpha = 0f,
                fixedRevealClipRect = RectF(0f, 0f, 60f, 20f),
                staticSuppressionMode = StaticSuppressionMode.VISIBLE_CLIP,
            )
        val snapshot =
            VisualFrameSnapshot(
                progress = 1f,
                state = com.xiwei.sujian.feature.editor.visual.TransactionState.Rendering,
                sliceVisualStates = listOf(state),
            )

        val result = planner.applyRebaseToSlices(emptyList(), snapshot, emptyMap(), emptyList())

        assertTrue(
            "unmapped fixed clip + alpha 0->0：不应产生 continuation（fixed clip 不是 liveness 轨），" +
                "实际 result.size=${result.size} → 留下透明 slice 继续挖静态正文",
            result.isEmpty(),
        )
    }

    // ---- 场景 9：unmapped fixed clip + alpha 0.5->0，应 continuation 并保留 fixed clip ----

    /**
     * #639 评论 5428952431 缺陷3：fixed clip 只是裁剪修饰，不是 liveness 轨。
     *
     * 场景：旧 state 带 fixedRevealClipRect，alpha 未完成（currentAlpha=0.5, targetAlpha=0）、
     * position 已完成、reveal 已完成。
     *
     * 期望（修复后）：仍应 continuation（alphaRemaining=true），并保留 fixedRevealClipRect。
     * fixed clip 跟随 alpha 轨一起存活，alpha 轨结束后一起销毁。
     */
    @Test
    fun repro9_unmappedFixedClipWithAlphaRemaining_shouldProduceContinuationWithFixedClip() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val fixedClip = RectF(0f, 0f, 60f, 20f)
        val state =
            makeRebaseState(
                role = SliceRole.CrossfadeOld,
                rect = rect,
                currentAlpha = 0.5f,
                targetAlpha = 0f,
                fixedRevealClipRect = fixedClip,
                staticSuppressionMode = StaticSuppressionMode.VISIBLE_CLIP,
            )
        val snapshot =
            VisualFrameSnapshot(
                progress = 0.5f,
                state = com.xiwei.sujian.feature.editor.visual.TransactionState.Rendering,
                sliceVisualStates = listOf(state),
            )

        val result = planner.applyRebaseToSlices(emptyList(), snapshot, emptyMap(), emptyList())

        assertTrue(
            "unmapped fixed clip + alpha 0.5->0：应产生 continuation（alphaRemaining=true），" +
                "实际 result.size=${result.size} → alpha 未完成但 continuation 丢失",
            result.isNotEmpty(),
        )
        if (result.isNotEmpty()) {
            assertNotNull(
                "unmapped fixed clip + alpha 0.5->0：continuation 应保留 fixedRevealClipRect，" +
                    "实际 fixedRevealClipRect=null → fixed clip 在 alpha 继续时丢失",
                result[0].fixedRevealClipRect,
            )
        }
    }

    // ---- 场景 10：Move(DESTINATION_RECT) -> CrossfadeOld，staticSuppressionMode 应翻成 NONE ----

    /**
     * #639 评论 5433981610：旧 Move 的默认 staticSuppressionMode 是 DESTINATION_RECT
     * （Move 负责 suppress 新位置的静态字）。Core pair-aware mapping 把旧 Move 接到
     * CrossfadeOld 时，视觉 ownership 已翻面成"旧侧覆盖层"，suppression 必须翻成 NONE。
     *
     * 期望：rebased staticSuppressionMode == NONE；renderer 的 suppression regions 不能
     * 包含 oldCurrentRect（否则 renderer 会在旧位置挖静态洞，旁边文字被切掉/闪空块）。
     */
    @Test
    fun repro10_moveToCrossfadeOld_suppressionModeShouldFlipToNone() {
        val oldCurrentRect = RectF(50f, 0f, 150f, 20f)
        val newDestRect = RectF(200f, 0f, 300f, 20f)

        val slice = makeCrossfadeOldSlice(newDestRect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Move,
                rect = oldCurrentRect,
                currentAlpha = 1f,
                targetAlpha = 1f,
                staticSuppressionMode = StaticSuppressionMode.DESTINATION_RECT,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // staticSuppressionMode 必须翻成 NONE
        assertEquals(
            "Move(DESTINATION_RECT) -> CrossfadeOld：staticSuppressionMode 必须翻成 NONE，" +
                "实际=${rebased.staticSuppressionMode} → renderer 会在 oldCurrentRect 挖静态洞",
            StaticSuppressionMode.NONE,
            rebased.staticSuppressionMode,
        )

        // renderer 的 suppression regions 不能包含 oldCurrentRect
        val renderer = AndroidTextAnimationRenderer()
        val transaction =
            PreparedVisualTransaction(
                transactionId = 1L,
                oldRevision = null,
                newRevision = null,
                staticPatches = emptyList(),
                animatedSlices = listOf(rebased),
                ownedSnapshotIds = emptySet(),
                referencedSnapshotIds = emptySet(),
                selectionDecoration = null,
                preeditDecoration = null,
                cursorTransition = null,
                durationMs = 300L,
            )
        val regions = renderer.computeStaticSuppressionRegions(transaction, 0f)
        val containsOldRect =
            regions.any { r ->
                kotlin.math.abs(r.left - oldCurrentRect.left) < 0.01f &&
                    kotlin.math.abs(r.right - oldCurrentRect.right) < 0.01f
            }
        assertFalse(
            "Move(DESTINATION_RECT) -> CrossfadeOld：suppression regions 不能包含 oldCurrentRect=$oldCurrentRect，" +
                "实际 regions=$regions → renderer 在旧位置挖静态洞，旁边文字被切掉",
            containsOldRect,
        )
    }

    // ---- 场景 11：Insert(reveal=0.4, DESTINATION_RECT) -> CrossfadeOld(fixedClip)，NONE ----

    /**
     * #639 评论 5433981610：旧 Insert reveal 到 40%，默认 staticSuppressionMode 是
     * DESTINATION_RECT。Core pair-aware mapping 接到 CrossfadeOld 后，视觉 ownership 翻面，
     * suppression 必须翻成 NONE。fixed clip 仍存在、位置仍锁死。
     *
     * 期望：fixed clip 仍存在、位置仍锁死在 oldCurrentRect、staticSuppressionMode == NONE；
     * renderer 画冻结半截旧字淡出，但不能在旧位置挖静态洞。
     */
    @Test
    fun repro11_insertRevealingToCrossfadeOld_suppressionModeShouldFlipToNoneFixedClipPreserved() {
        val oldCurrentRect = RectF(30f, 0f, 130f, 20f)
        val newDestRect = RectF(200f, 0f, 300f, 20f)

        val slice = makeCrossfadeOldSlice(newDestRect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Insert,
                rect = oldCurrentRect,
                currentAlpha = 1f,
                targetAlpha = 1f,
                revealMode = TextRevealMode.REVEAL,
                revealFraction = 0.4f,
                staticSuppressionMode = StaticSuppressionMode.DESTINATION_RECT,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // fixed clip 仍存在（冻结旧 Insert 的半 reveal 可见部分）
        assertNotNull(
            "Insert(reveal=0.4, DESTINATION_RECT) -> CrossfadeOld：fixedRevealClipRect 应存在",
            rebased.fixedRevealClipRect,
        )
        // 位置仍锁死在 oldCurrentRect
        assertEquals(
            "Insert -> CrossfadeOld：destinationRect.left 必须锁在 oldCurrentRect.left=30f",
            oldCurrentRect.left,
            rebased.destinationRect.left,
            0.001f,
        )
        assertEquals(
            "Insert -> CrossfadeOld：fromDestinationRect 必须为 null（原地退场）",
            null,
            rebased.fromDestinationRect,
        )
        // staticSuppressionMode 必须翻成 NONE
        assertEquals(
            "Insert(DESTINATION_RECT) -> CrossfadeOld：staticSuppressionMode 必须翻成 NONE，" +
                "实际=${rebased.staticSuppressionMode} → renderer 会在旧位置挖静态洞",
            StaticSuppressionMode.NONE,
            rebased.staticSuppressionMode,
        )

        // renderer 的 suppression regions 不能包含 oldCurrentRect
        val renderer = AndroidTextAnimationRenderer()
        val transaction =
            PreparedVisualTransaction(
                transactionId = 1L,
                oldRevision = null,
                newRevision = null,
                staticPatches = emptyList(),
                animatedSlices = listOf(rebased),
                ownedSnapshotIds = emptySet(),
                referencedSnapshotIds = emptySet(),
                selectionDecoration = null,
                preeditDecoration = null,
                cursorTransition = null,
                durationMs = 300L,
            )
        val regions = renderer.computeStaticSuppressionRegions(transaction, 0f)
        val containsOldRect =
            regions.any { r ->
                kotlin.math.abs(r.left - oldCurrentRect.left) < 0.01f &&
                    kotlin.math.abs(r.right - oldCurrentRect.right) < 0.01f
            }
        assertFalse(
            "Insert(DESTINATION_RECT) -> CrossfadeOld：suppression regions 不能包含 oldCurrentRect=$oldCurrentRect，" +
                "实际 regions=$regions → renderer 在旧位置挖静态洞",
            containsOldRect,
        )
    }

    // ---- 场景 12：Delete(VISIBLE_CLIP) -> CrossfadeOld，仍保持 VISIBLE_CLIP ----

    /**
     * #639 评论 5433981610：旧 Delete 的默认 staticSuppressionMode 是 VISIBLE_CLIP。
     * Delete -> CrossfadeOld 不是 emergence role -> CrossfadeOld 翻面路径，
     * suppression 必须保持 VISIBLE_CLIP（吞字 ownership 连续性）。
     *
     * 期望：staticSuppressionMode 仍为 VISIBLE_CLIP，确认这次修正没有把上一轮的
     * Delete/CrossfadeOld ownership 连续性改坏。
     */
    @Test
    fun repro12_deleteToCrossfadeOld_suppressionModeShouldStayVisibleClip() {
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
                staticSuppressionMode = StaticSuppressionMode.VISIBLE_CLIP,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // Delete 不是 emergence role，suppression 必须保持 VISIBLE_CLIP
        assertEquals(
            "Delete(VISIBLE_CLIP) -> CrossfadeOld：staticSuppressionMode 必须保持 VISIBLE_CLIP，" +
                "实际=${rebased.staticSuppressionMode} → 这次修正把 Delete/CrossfadeOld ownership 连续性改坏了",
            StaticSuppressionMode.VISIBLE_CLIP,
            rebased.staticSuppressionMode,
        )
    }
}
