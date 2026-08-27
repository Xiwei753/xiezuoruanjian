package com.xiwei.sujian.feature.editor.visual.planner

import android.graphics.Rect
import android.graphics.RectF
import com.xiwei.sujian.feature.editor.visual.PreparedVisualTransaction
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceVisualState
import com.xiwei.sujian.feature.editor.visual.TextRevealMode
import com.xiwei.sujian.feature.editor.visual.TextRevealSpec
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #639 评论 5425871530 复现测试 — 跨 role rebase 外观续播未闭环。
 *
 * SliceRole 同时承担"这个 slice 是什么逻辑角色"和"当前怎么显示"两件事。Core 明确允许
 * Move / Insert / CrossfadeNew 三者互相映射，但 Android 三种角色的可见程度不是同一种状态：
 * - Insert 靠 revealFraction / revealSpec，startAlpha=endAlpha=1；renderer 有 revealSpec 时
 *   还会直接把 alpha 设成 255（[AndroidTextAnimationRenderer.drawRevealSlice]）。
 * - CrossfadeNew 靠 currentAlpha。
 * - Move 默认整字可见，只做位置/alpha。
 *
 * 四个跨 role rebase 场景当前会闪（外观不连续）：
 * 1. Move -> Insert：旧 Move 无 revealFraction，applyRebaseState 给新 Insert 写
 *    initialFraction = rebaseState.revealFraction ?: 0f = 0f，原本完整可见的字在新事务
 *    第一帧变成 reveal=0，先消失再重新吐出来。
 * 2. CrossfadeNew -> Insert：同样把 reveal 当 0；旧 CrossfadeNew 可能已经 alpha=0.4，
 *    但新 Insert renderer 不吃这个 alpha（有 revealSpec 时强制 alpha=255），而是从
 *    reveal=0 开始，当前可见状态丢失。
 * 3. Insert -> Move：旧 Insert 可能只 reveal 到 0.4，但 currentAlpha 一直是 1。新 Move
 *    只继承 currentAlpha=1，第一帧直接画完整字，半截字瞬间补全。
 * 4. Insert -> CrossfadeNew：同理，旧半截 Insert 的 currentAlpha=1 会让新 CrossfadeNew
 *    第一帧变成完整字。
 *
 * issue 第四部分（RebasePlanner.kt）描述的修复方案：
 * - 旧 state 有 revealFraction：无论新 role 是 Insert / Move / CrossfadeNew，都给新 slice
 *   重建一个 REVEAL continuation，initialFraction = old revealFraction。
 * - 旧 state 没有 revealFraction：新 slice 不应凭空启动 reveal，即使新 role 是 Insert 也要
 *   把 planner 原本的 revealSpec 清掉，继续 startAlpha = old currentAlpha -> endAlpha = 1。
 *
 * 本测试断言 issue 描述的期望（修复后）行为。在当前 buggy 代码上这些断言会失败，
 * 从而证明 bug 存在。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class RebasePlannerCrossRoleReproTest {
    private val planner = RebasePlanner()

    // ---- helpers ----

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
            revealSpec =
                TextRevealSpec(
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

    private fun makeMoveSlice(destinationRect: RectF): PreparedVisualTransaction.AnimatedSlice {
        return PreparedVisualTransaction.AnimatedSlice(
            role = SliceRole.Move,
            snapshot = null,
            sourceRect = Rect(0, 0, 10, 20),
            destinationRect = destinationRect,
            startAlpha = 1f,
            endAlpha = 1f,
            clusterByteStart = 0,
            clusterByteEndExclusive = 1,
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
     * 构造 SliceVisualState。currentRect == destinationRect（字在位置上），
     * 模拟旧 slice 已到位但外观状态（reveal/alpha）可能未走完。
     */
    private fun makeRebaseState(
        role: SliceRole,
        rect: RectF,
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
            currentLeft = rect.left,
            currentTop = rect.top,
            currentRight = rect.right,
            currentBottom = rect.bottom,
            currentAlpha = currentAlpha,
            destinationLeft = rect.left,
            destinationTop = rect.top,
            destinationRight = rect.right,
            destinationBottom = rect.bottom,
            revealFraction = revealFraction,
            remainingFraction = 1f,
        )
    }

    // ---- 场景1：Move -> Insert，旧 Move 完整可见字在新 Insert 第一帧消失 ----

    /**
     * 场景：旧 Move 字完整可见（revealFraction=null, currentAlpha=1, currentRect=destinationRect）。
     * rebase 成新 Insert（有 revealSpec）。
     *
     * 期望（修复后，issue 第四部分）：旧 state 没有 revealFraction，新 slice 不应凭空启动
     * reveal，即使新 role 是 Insert 也要把 planner 原本的 revealSpec 清掉，继续
     * startAlpha = old currentAlpha -> endAlpha = 1。即 rebased.revealSpec == null，
     * rebased.startAlpha == 1f。
     *
     * 当前 buggy：applyRebaseState Insert 分支写
     * initialFraction = rebaseState.revealFraction ?: 0f = null ?: 0f = 0f，
     * 新 Insert 从 reveal=0 开始，原本完整可见的字在新事务第一帧变成 reveal=0，
     * 先消失再重新吐出来 → 闪烁。
     */
    @Test
    fun repro1_moveToInsert_fullVisibleCharShouldNotDisappearAtFirstFrame() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val slice = makeInsertSlice(rect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Move,
                rect = rect,
                currentAlpha = 1f,
                // Move 无 revealFraction，字完整可见
                revealFraction = null,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // 旧 Move 无 revealFraction → 新 Insert 不应凭空启动 reveal
        assertNull(
            "Move -> Insert：旧 Move 无 revealFraction（字完整可见），新 Insert 不应凭空启动 reveal，" +
                "应清掉 revealSpec，实际 revealSpec=${rebased.revealSpec} → " +
                "initialFraction=${rebased.revealSpec?.initialFraction}，原本完整可见的字在新事务" +
                "第一帧会变成 reveal=0，先消失再重新吐出来",
            rebased.revealSpec,
        )
        // startAlpha 应继承旧 currentAlpha
        assertEquals(
            "Move -> Insert：新 Insert startAlpha 应继承旧 Move currentAlpha=1f，" +
                "实际 startAlpha=${rebased.startAlpha}",
            1f,
            rebased.startAlpha,
            0.001f,
        )
    }

    // ---- 场景2：CrossfadeNew -> Insert，旧 CrossfadeNew alpha=0.4 可见状态丢失 ----

    /**
     * 场景：旧 CrossfadeNew alpha=0.4（半透明完整字），revealFraction=null。
     * rebase 成新 Insert（有 revealSpec）。
     *
     * 期望（修复后，issue 第四部分）：旧 state 没有 revealFraction，新 slice 不应凭空启动
     * reveal，清掉 revealSpec，继续 startAlpha = old currentAlpha=0.4 -> endAlpha = 1。
     * 即 rebased.revealSpec == null，rebased.startAlpha == 0.4f。
     *
     * 当前 buggy：initialFraction = null ?: 0f = 0f，且 renderer 有 revealSpec 时强制
     * alpha=255（drawRevealSlice 中 slicePaint.alpha = 255），旧 alpha=0.4 丢失，
     * reveal 从 0 开始 → 当前可见状态完全丢失，字先消失再重新吐出。
     */
    @Test
    fun repro2_crossfadeNewToInsert_alphaAndVisibilityShouldBePreserved() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val slice = makeInsertSlice(rect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.CrossfadeNew,
                rect = rect,
                // 半透明完整字
                currentAlpha = 0.4f,
                revealFraction = null,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // 旧 CrossfadeNew 无 revealFraction → 新 Insert 不应凭空启动 reveal
        assertNull(
            "CrossfadeNew -> Insert：旧 CrossfadeNew 无 revealFraction，新 Insert 不应凭空启动 reveal，" +
                "应清掉 revealSpec，实际 revealSpec=${rebased.revealSpec} → " +
                "renderer 有 revealSpec 时强制 alpha=255，旧 alpha=0.4 丢失，且 reveal 从 0 开始，" +
                "当前可见状态完全丢失",
            rebased.revealSpec,
        )
        // 旧 alpha=0.4 应被保留到新 Insert startAlpha
        assertEquals(
            "CrossfadeNew -> Insert：旧 CrossfadeNew alpha=0.4 应被保留到新 Insert startAlpha，" +
                "实际 startAlpha=${rebased.startAlpha} → 半透明可见状态丢失",
            0.4f,
            rebased.startAlpha,
            0.001f,
        )
    }

    // ---- 场景3：Insert -> Move，旧半截 Insert 在新 Move 第一帧瞬间补全 ----

    /**
     * 场景：旧 Insert reveal=0.4（只 reveal 到 40%），currentAlpha=1（Insert 靠 reveal 不靠 alpha）。
     * rebase 成新 Move。
     *
     * 期望（修复后，issue 第四部分）：旧 state 有 revealFraction，无论新 role 是 Insert / Move /
     * CrossfadeNew，都给新 slice 重建一个 REVEAL continuation，initialFraction = old revealFraction=0.4。
     * 即 rebased 应携带 reveal 续播信息（revealSpec 或 fixedRevealClipRect），且 initialFraction=0.4。
     *
     * 当前 buggy：applyRebaseState Move 分支只 slice.copy(startAlpha = rebaseState.currentAlpha=1)，
     * 不携带任何 reveal 续播信息，新 Move 第一帧直接画完整字（alpha=1），半截字瞬间补全 → 闪烁。
     */
    @Test
    fun repro3_insertToMove_halfRevealedCharShouldNotInstantlyFillAtFirstFrame() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val slice = makeMoveSlice(rect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Insert,
                rect = rect,
                // Insert currentAlpha 一直是 1
                currentAlpha = 1f,
                // 只 reveal 到 40%
                revealFraction = 0.4f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        // 新 Move 应携带 reveal 续播信息继承旧 Insert 的半截可见
        val hasRevealContinuation = rebased.revealSpec != null || rebased.fixedRevealClipRect != null
        assertTrue(
            "Insert -> Move：旧 Insert reveal=0.4（半截字），新 Move 应携带 reveal/clip 续播信息" +
                "（revealSpec 或 fixedRevealClipRect）保持半截可见，实际两者都为 null，" +
                "且 startAlpha=${rebased.startAlpha} → 新 Move 第一帧直接画完整字，半截字瞬间补全",
            hasRevealContinuation,
        )
        // 若有 revealSpec，initialFraction 应为旧 revealFraction=0.4
        if (rebased.revealSpec != null) {
            assertEquals(
                "Insert -> Move：新 Move revealSpec.initialFraction 应为旧 Insert revealFraction=0.4，" +
                    "实际 initialFraction=${rebased.revealSpec!!.initialFraction}",
                0.4f,
                rebased.revealSpec!!.initialFraction,
                0.001f,
            )
        }
    }

    // ---- 场景4：Insert -> CrossfadeNew，旧半截 Insert 在新 CrossfadeNew 第一帧瞬间补全 ----

    /**
     * 场景：旧 Insert reveal=0.4，currentAlpha=1。rebase 成新 CrossfadeNew。
     *
     * 期望（修复后，issue 第四部分）：旧 state 有 revealFraction，新 CrossfadeNew 应重建
     * REVEAL continuation，initialFraction = old revealFraction=0.4。
     *
     * 当前 buggy：applyRebaseState CrossfadeNew 分支只 slice.copy(startAlpha = rebaseState.currentAlpha=1)，
     * 不携带任何 reveal 续播信息，新 CrossfadeNew 第一帧直接画完整字（alpha=1），半截字瞬间补全 → 闪烁。
     */
    @Test
    fun repro4_insertToCrossfadeNew_halfRevealedCharShouldNotInstantlyFillAtFirstFrame() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val slice = makeCrossfadeNewSlice(rect)
        val rebaseState =
            makeRebaseState(
                role = SliceRole.Insert,
                rect = rect,
                currentAlpha = 1f,
                revealFraction = 0.4f,
            )

        val rebased = planner.applyRebaseState(slice, rebaseState, emptyMap())

        val hasRevealContinuation = rebased.revealSpec != null || rebased.fixedRevealClipRect != null
        assertTrue(
            "Insert -> CrossfadeNew：旧 Insert reveal=0.4（半截字），新 CrossfadeNew 应携带 reveal/clip " +
                "续播信息保持半截可见，实际两者都为 null，且 startAlpha=${rebased.startAlpha} → " +
                "新 CrossfadeNew 第一帧直接画完整字，半截字瞬间补全",
            hasRevealContinuation,
        )
        if (rebased.revealSpec != null) {
            assertEquals(
                "Insert -> CrossfadeNew：新 CrossfadeNew revealSpec.initialFraction 应为旧 Insert " +
                    "revealFraction=0.4，实际 initialFraction=${rebased.revealSpec!!.initialFraction}",
                0.4f,
                rebased.revealSpec!!.initialFraction,
                0.001f,
            )
        }
    }

    // ---- 额外：RunAnimation synthetic run 未映射仍能从原 revealFraction 继续 ----

    /**
     * issue 第一部分提到 RunAnimation 的真实漏洞：groupClustersIntoRuns() 多字 run 会创建
     * synthetic LineClusterSnapshot，但这个合并对象并不在原始 AndroidLineSnapshot.clusters 里。
     * rebase 时按 clusterByteStart/clusterByteEndExclusive 在 snapshot.clusters 里匹配会找不到，
     * 导致未映射的半截 Insert continuation 无法重建 caret 几何。
     *
     * 本测试构造一个 synthetic run（byte range 0..3 跨 3 个 cluster），rebase 时
     * snapshotLookup 里的 snapshot.clusters 是原始 3 个单字 cluster（0..1, 1..2, 2..3），
     * 按 0..3 匹配不到 → shouldContinueInsertReveal 的 matchedCluster == null → Insert 被丢弃。
     *
     * 期望（修复后，issue 第一部分）：slice 保存 caret/reveal 几何（CaretRevealGeometry），
     * 不 rebase 时再从 snapshot 反查，synthetic run 也能继续。
     *
     * 当前 buggy：matchedCluster == null → Insert 被丢弃 → 半截字突然变成完整字。
     */
    @Test
    fun repro5_syntheticRunUnmappedInsertShouldContinueFromOriginalRevealFraction() {
        val (rebaseSnapshot, snapshotLookup) = makeSyntheticRunRebaseData()

        val result =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot,
                snapshotLookup = snapshotLookup,
            )

        assertTrue(
            "RunAnimation synthetic run（byte 0..3）未映射时，半截 Insert（reveal=0.4）应继续，" +
                "不应因 matchedCluster==null（synthetic cluster 不在 snapshot.clusters 里）被丢弃，" +
                "实际 result 为空 → 半截字突然变成完整字",
            result.isNotEmpty(),
        )
        if (result.isNotEmpty()) {
            val continued = result[0]
            assertNotNull(
                "synthetic run continuation 应携带 revealSpec 从原 revealFraction=0.4 继续",
                continued.revealSpec,
            )
            if (continued.revealSpec != null) {
                assertEquals(
                    "synthetic run continuation initialFraction 应为原 revealFraction=0.4",
                    0.4f,
                    continued.revealSpec!!.initialFraction,
                    0.001f,
                )
            }
            // #639 评论 5427183226 缺口1：synthetic run continuation 的 sourceRect 应是
            // 原 AnimatedSlice.sourceRect（合并后的几个字 Rect(0,0,30,20)），不是整行
            // snapshot.sourceRect = Rect(0,0,300,20)。
            val expectedSourceRect = Rect(0, 0, 30, 20)
            val wholeLineSourceRect = snapshotLookup.getValue(1L).sourceRect
            assertEquals(
                "synthetic run continuation 的 sourceRect 应是原 AnimatedSlice.sourceRect" +
                    "（合并后的几个字 $expectedSourceRect），实际 ${continued.sourceRect}。" +
                    "修复前会退到整行 snapshot.sourceRect = $wholeLineSourceRect，把整行 bitmap 压进 run 的 destinationRect。",
                expectedSourceRect,
                continued.sourceRect,
            )
            assertNotEquals(
                "synthetic run continuation 的 sourceRect 不应等于整行 snapshot.sourceRect",
                wholeLineSourceRect,
                continued.sourceRect,
            )
        }
    }

    // ---- #639 评论 5427183226：跨第二次 rebase appearance 轨续播 ----

    /**
     * #639 评论 5427183226 缺口2：跨第二次 rebase appearance 轨续播。
     *
     * 由 Insert -> Move 产生、仍有 revealFraction=0.5 的 Move，连续两次未映射 rebase
     * 后应仍然保留 revealSpec（reveal 轨不丢）。修复前：第一次 rebase 后 role=Move
     * 进 buildMoveContinuation 丢 revealSpec，第二次 rebase 时 revealFraction 已丢。
     * 修复后：按三条视觉轨续播，revealRemaining=true → 重建 revealSpec。
     */
    @Test
    fun repro6_crossSecondRebaseMoveWithRevealShouldKeepRevealAcrossTwoRebases() {
        val fromRect = RectF(0f, 0f, 100f, 20f)
        val destRect = RectF(50f, 0f, 150f, 20f)
        // 第一次 rebase 的 state：role=Move, revealFraction=0.5, revealMode=REVEAL
        val state1 =
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
                sourceRect = Rect(0, 0, 10, 20),
                targetAlpha = 1f,
                revealMode = TextRevealMode.REVEAL,
                revealFraction = 0.5f,
                remainingFraction = 1f,
                caretRevealGeometry =
                    PreparedVisualTransaction.CaretRevealGeometry(
                        visualRect = destRect,
                        caretStartX = destRect.left,
                        caretEndX = destRect.right,
                    ),
            )
        val rebaseSnapshot1 =
            com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot(
                progress = 0.5f,
                state = com.xiwei.sujian.feature.editor.visual.TransactionState.Rendering,
                sliceVisualStates = listOf(state1),
            )

        // 第一次未映射 rebase
        val result1 =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot1,
                snapshotLookup = emptyMap(),
            )
        assertTrue(
            "第一次 rebase：带 revealFraction=0.5 的 Move 未映射时应产生 continuation",
            result1.isNotEmpty(),
        )
        assertNotNull(
            "第一次 rebase：continuation 应携带 revealSpec（reveal 轨不丢）",
            result1[0].revealSpec,
        )

        // 模拟 computeSliceVisualStates 从 result1[0] 保存新的 SliceVisualState
        // （localProgress=0 → revealFraction = initialFraction = 0.5）
        val slice1 = result1[0]
        val state2 =
            SliceVisualState(
                snapshotId = slice1.snapshot?.snapshotId ?: -1L,
                role = slice1.role,
                lineIndex = 0,
                clusterByteStart = slice1.clusterByteStart,
                clusterByteEndExclusive = slice1.clusterByteEndExclusive,
                currentLeft = slice1.destinationRect.left,
                currentTop = slice1.destinationRect.top,
                currentRight = slice1.destinationRect.right,
                currentBottom = slice1.destinationRect.bottom,
                currentAlpha = slice1.startAlpha,
                destinationLeft = slice1.destinationRect.left,
                destinationTop = slice1.destinationRect.top,
                destinationRight = slice1.destinationRect.right,
                destinationBottom = slice1.destinationRect.bottom,
                sourceRect = Rect(slice1.sourceRect),
                targetAlpha = slice1.endAlpha,
                revealMode = slice1.revealSpec?.mode,
                revealFraction = slice1.revealSpec?.initialFraction,
                remainingFraction = 1f,
                fixedRevealClipRect = slice1.fixedRevealClipRect?.let { RectF(it) },
                caretRevealGeometry = slice1.caretRevealGeometry,
            )
        val rebaseSnapshot2 =
            com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot(
                progress = 0.5f,
                state = com.xiwei.sujian.feature.editor.visual.TransactionState.Rendering,
                sliceVisualStates = listOf(state2),
            )

        // 第二次未映射 rebase
        val result2 =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot2,
                snapshotLookup = emptyMap(),
            )
        assertTrue(
            "第二次 rebase：跨第二次 rebase 后 appearance 轨应仍产生 continuation",
            result2.isNotEmpty(),
        )
        assertNotNull(
            "第二次 rebase：跨第二次 rebase 后 revealSpec 应仍存在（reveal 轨不丢）",
            result2[0].revealSpec,
        )
    }

    /**
     * #639 评论 5427183226 缺口2：Delete 同时带 reveal 和 alpha 淡出，跨第二次 rebase
     * 后 startAlpha 应保持当前 alpha，不被抬回 1。
     */
    @Test
    fun repro7_crossSecondRebaseDeleteWithRevealAndFadingAlphaShouldKeepAlpha() {
        val rect = RectF(0f, 0f, 100f, 20f)
        val state1 =
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
                sourceRect = Rect(0, 0, 10, 20),
                targetAlpha = 0f,
                revealMode = com.xiwei.sujian.feature.editor.visual.TextRevealMode.SWALLOW,
                revealFraction = 0.5f,
                remainingFraction = 1f,
                caretRevealGeometry =
                    PreparedVisualTransaction.CaretRevealGeometry(
                        visualRect = rect,
                        caretStartX = rect.left,
                        caretEndX = rect.right,
                    ),
            )
        val rebaseSnapshot1 =
            com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot(
                progress = 0.5f,
                state = com.xiwei.sujian.feature.editor.visual.TransactionState.Rendering,
                sliceVisualStates = listOf(state1),
            )

        val result1 =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot1,
                snapshotLookup = emptyMap(),
            )
        assertTrue(result1.isNotEmpty())
        assertEquals(
            "第一次 rebase：Delete 带 reveal + alpha 淡出时 startAlpha 应保持 0.4",
            0.4f,
            result1[0].startAlpha,
            0.001f,
        )

        // 模拟第二次 rebase：从 result1[0] 保存新 state（currentAlpha 走到 0.3）
        val slice1 = result1[0]
        val state2 =
            SliceVisualState(
                snapshotId = slice1.snapshot?.snapshotId ?: -1L,
                role = slice1.role,
                lineIndex = 0,
                clusterByteStart = slice1.clusterByteStart,
                clusterByteEndExclusive = slice1.clusterByteEndExclusive,
                currentLeft = slice1.destinationRect.left,
                currentTop = slice1.destinationRect.top,
                currentRight = slice1.destinationRect.right,
                currentBottom = slice1.destinationRect.bottom,
                currentAlpha = 0.3f,
                destinationLeft = slice1.destinationRect.left,
                destinationTop = slice1.destinationRect.top,
                destinationRight = slice1.destinationRect.right,
                destinationBottom = slice1.destinationRect.bottom,
                sourceRect = Rect(slice1.sourceRect),
                targetAlpha = slice1.endAlpha,
                revealMode = slice1.revealSpec?.mode,
                revealFraction = 0.6f,
                remainingFraction = 1f,
                fixedRevealClipRect = slice1.fixedRevealClipRect?.let { RectF(it) },
                caretRevealGeometry = slice1.caretRevealGeometry,
            )
        val rebaseSnapshot2 =
            com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot(
                progress = 0.5f,
                state = com.xiwei.sujian.feature.editor.visual.TransactionState.Rendering,
                sliceVisualStates = listOf(state2),
            )

        val result2 =
            planner.applyRebaseToSlices(
                newSlices = emptyList(),
                rebaseSnapshot = rebaseSnapshot2,
                snapshotLookup = emptyMap(),
            )
        assertTrue(
            "第二次 rebase：Delete 带 reveal + alpha 淡出应仍产生 continuation",
            result2.isNotEmpty(),
        )
        assertEquals(
            "第二次 rebase：startAlpha 应保持当前 alpha=0.3，不被抬回 1",
            0.3f,
            result2[0].startAlpha,
            0.001f,
        )
    }

    /** repro5 的测试数据构造：synthetic run rebaseSnapshot + snapshotLookup。 */
    private fun makeSyntheticRunRebaseData(): Pair<
        com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot,
        Map<Long, com.xiwei.sujian.feature.editor.layout.AndroidLineSnapshot>,
        > {
        val rect = RectF(0f, 0f, 300f, 20f)
        val clusters = makeSyntheticRunClusters()
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
        val syntheticRunState = makeSyntheticRunState(rect)
        val rebaseSnapshot =
            com.xiwei.sujian.feature.editor.visual.VisualFrameSnapshot(
                progress = 0.4f,
                state = com.xiwei.sujian.feature.editor.visual.TransactionState.Rendering,
                sliceVisualStates = listOf(syntheticRunState),
            )
        return Pair(rebaseSnapshot, mapOf(1L to snapshot))
    }

    /** repro5 的三个单字 cluster（byte 0..1, 1..2, 2..3）。 */
    private fun makeSyntheticRunClusters(): List<com.xiwei.sujian.feature.editor.layout.LineClusterSnapshot> =
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

    /** repro5 的 synthetic run SliceVisualState（byte 0..3, reveal=0.4）。 */
    private fun makeSyntheticRunState(rect: RectF): SliceVisualState =
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
            // #639 评论 5427183226 缺口1：synthetic run 的 sourceRect 是合并后的几个字
            // Rect(0,0,30,20)，不是整行 snapshot.sourceRect = Rect(0,0,300,20)。
            sourceRect = Rect(0, 0, 30, 20),
            targetAlpha = 1f,
            revealMode = TextRevealMode.REVEAL,
            revealFraction = 0.4f,
            remainingFraction = 1f,
            caretRevealGeometry =
                PreparedVisualTransaction.CaretRevealGeometry(
                    visualRect = rect,
                    caretStartX = 0f,
                    caretEndX = 300f,
                ),
        )
}
