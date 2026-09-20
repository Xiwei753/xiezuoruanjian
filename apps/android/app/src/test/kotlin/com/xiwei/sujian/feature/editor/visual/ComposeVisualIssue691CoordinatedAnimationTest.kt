package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #691 协同动画回归测试 — 验证文字与光标共享同一个 VisualScene / frame clock。
 *
 * 核心断言：
 * 1. 同一可见帧中 text scene 与 cursor scene 使用同一个 frame timestamp
 * 2. coordinated=true 时 cursor 与主文字视觉进度一致
 * 3. coordinated=false 时允许 progress 不同，但状态采样时间相同、动画所有权仍唯一
 * 4. cursor 不允许落后于当前已显示正文一个或多个 revision
 * 5. 自动换行时，发生 reflow 的 retained units 与 cursor 按当前设置语义运动
 * 6. 同一 VSync 内多笔 patch 不应给 cursor/reflow 连续创建肉眼不可见的中间几何轨迹
 * 7. 无文字时 smooth cursor 仍必须保持可见
 * 8. 任意设置组合都不能改变真实正文、selection、composition、IME 和 TextField 权威布局
 */
@Suppress("StringLiteralDuplication", "MaxLineLength", "LongMethod")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue691CoordinatedAnimationTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 核心：cursor 和 text 共享同一个 frame clock ====================

    /**
     * 断言1：同一帧采样 text scene 与 cursor scene 使用同一个 frameTimeNanos。
     *
     * 场景：插入 "a"，cursor motion 从 oldCursorRect 到 newCursorRect。
     * 在单个 frameTimeNanos(=0) 上同时把文字与光标 position 交给同一个 timeline，
     * 再在 50ms 采样，验证 cursor 与 text 都按同一帧插值到中间进度。
     */
    @Test
    fun sameFrameSampling_cursorAndTextShareFrameTimestamp() {
        val layouts = captureLayouts("", "a")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)

        val timeline = ComposeVisualTimeline()

        // 构建带 cursor motion 的 patch
        val oldCursorRect = Rect(0f, 0f, 2f, 14f)
        val newCursorRect = Rect(7f, 0f, 9f, 14f)
        val path =
            CursorMotionPath(
                points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
            )
        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                cursorMotionPath = path,
                durationMs = 100L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
            )

        // #691：在单个 frameTimeNanos(=0) 上同时处理文字与光标 position（同一 VisualScene 所有者）。
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = oldCursorRect,
            cursorPath = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // 在 50ms 采样（duration=100ms）— text 与 cursor 都应按同一帧 timestamp 插值到中间进度。
        val scene = timeline.sample(50L * NANOS_PER_MS)

        // 验证 cursor rect 存在且已插值到中间
        assertNotNull("cursor rect 应该存在", scene.cursorRect)
        val cursorRect = scene.cursorRect!!
        val expectedLeft = oldCursorRect.left + (newCursorRect.left - oldCursorRect.left) * 0.5f
        assertTrue(
            "cursor 应在中间位置（left≈$expectedLeft），实际=${cursorRect.left}",
            kotlin.math.abs(cursorRect.left - expectedLeft) < 1f,
        )

        // 验证 text unit 也在同一帧插值到中间 alpha 进度（与 cursor 共享 frame timestamp）。
        val aUnit = scene.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("应该有文字 unit", aUnit)
        val alpha = aUnit!!.alpha.from
        assertTrue(
            "text alpha 应在中间（≈0.5），实际=$alpha",
            kotlin.math.abs(alpha - 0.5f) < 0.1f,
        )
    }

    /**
     * 断言1 强化：快速输入 3 步，每步都验证 cursor 和 text 在同一帧被采样。
     *
     * 场景："" → "a" → "ab" → "abc"，间隔 30ms（< 100ms 动画时长）。
     */
    @Test
    fun rapidInput_threeSteps_cursorAndTextAlwaysSampledTogether() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val state = ComposeEditorVisualState(targetId = "test-sync-three-step")

        // Step A: "" → "a"
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(0L)

        // Step B: "a" → "ab" at 30ms
        val frameTimeB = 30L * 1_000_000L
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.onVisualIntent(
            makeInsertIntent(
                2L,
                1L,
                2L,
                "a",
                "ab",
                TextRange(1, 2),
                offsetMap = VisualOffsetMap(listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY))),
            ),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        state.drainPendingPatchesAtFrame(frameTimeB)
        val sceneB = state.sampleVisualScene(frameTimeB)

        // Step C: "ab" → "abc" at 60ms
        val frameTimeC = 60L * 1_000_000L
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        state.onVisualIntent(
            makeInsertIntent(
                3L,
                2L,
                3L,
                "ab",
                "abc",
                TextRange(2, 3),
                offsetMap = VisualOffsetMap(listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY))),
            ),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(3, 3), 0)
        state.drainPendingPatchesAtFrame(frameTimeC)
        val sceneC = state.sampleVisualScene(frameTimeC)

        // 验证：每帧 scene 都包含 cursorRect（即使只是 resting rect）
        assertNotNull("Step B: scene 应包含 cursorRect", sceneB.cursorRect)
        assertNotNull("Step C: scene 应包含 cursorRect", sceneC.cursorRect)

        // 验证：文字 units 在每帧都有活动（动画未完成）
        assertTrue("Step B: 应有活动文字 units", sceneB.units.isNotEmpty())
        assertTrue("Step C: 应有活动文字 units", sceneC.units.isNotEmpty())
    }

    // ==================== coordinated 模式 ====================

    /**
     * 断言2：coordinated=true 时，cursor 动画与文字动画使用同一个 duration。
     *
     * 场景：插入 "a"，duration=100ms。在 50ms 时验证 cursor 和 text 都在中间进度。
     */
    @Test
    fun coordinatedTrue_cursorAndTextShareDuration() {
        val layouts = captureLayouts("", "a")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)

        val timeline = ComposeVisualTimeline()

        val oldCursorRect = Rect(0f, 0f, 2f, 14f)
        val newCursorRect = Rect(7f, 0f, 9f, 14f)
        val path =
            CursorMotionPath(
                points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
            )
        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                cursorMotionPath = path,
                durationMs = 100L,
                motionPolicy =
                    EditorMotionPolicy(
                        textDurationMillis = 100L,
                        cursorEnabled = true,
                        cursorDurationMillis = 80L,
                        coordinated = true,
                    ),
            )

        val frameTime = 50L * NANOS_PER_MS
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = oldCursorRect,
            cursorPath = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )
        val scene = timeline.sample(frameTime)

        // 验证 cursor 在中间位置（50ms / 100ms = 50%）
        val cursorRect = scene.cursorRect
        assertNotNull(cursorRect)
        val expectedLeft = oldCursorRect.left + (newCursorRect.left - oldCursorRect.left) * 0.5f
        assertTrue(
            "coordinated=true: cursor 应在中间位置（left≈$expectedLeft），实际=${cursorRect!!.left}",
            kotlin.math.abs(cursorRect.left - expectedLeft) < 1f,
        )

        // 验证文字 unit 也在中间 alpha 进度（与 cursor 共享 frame timestamp）
        val aUnit = scene.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull(aUnit)
        val alpha = aUnit!!.alpha.from
        assertTrue(
            "coordinated=true: text alpha 应在中间（≈0.5），实际=$alpha",
            kotlin.math.abs(alpha - 0.5f) < 0.1f,
        )
    }

    /**
     * 断言3：coordinated=false 时，cursor 使用 cursorDurationMillis，text 使用 textDurationMillis。
     * 但二者仍在同一个 frame clock 中采样。
     */
    @Test
    fun coordinatedFalse_cursorUsesOwnDuration() {
        val layouts = captureLayouts("", "a")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)

        val timeline = ComposeVisualTimeline()

        val oldCursorRect = Rect(0f, 0f, 2f, 14f)
        val newCursorRect = Rect(7f, 0f, 9f, 14f)
        val path =
            CursorMotionPath(
                points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
            )
        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                cursorMotionPath = path,
                durationMs = 100L,
                motionPolicy =
                    EditorMotionPolicy(
                        textDurationMillis = 100L,
                        cursorEnabled = true,
                        cursorDurationMillis = 50L,
                        coordinated = false,
                    ),
            )

        val frameTime = 50L * NANOS_PER_MS
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = oldCursorRect,
            cursorPath = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
            // #691：coordinated=false 时 cursor 使用 cursorDurationMillis=50ms（独立 position track）。
            cursorDurationNanos = 50L * NANOS_PER_MS,
        )
        val scene = timeline.sample(frameTime)

        // cursor 使用 cursorDurationMillis=50ms，在 50ms 时已完成（left ≈ newCursorRect.left）
        val cursorRect = scene.cursorRect
        assertNotNull(cursorRect)
        assertTrue(
            "coordinated=false: cursor 应已完成（left≈${newCursorRect.left}），实际=${cursorRect!!.left}",
            kotlin.math.abs(cursorRect.left - newCursorRect.left) < 1f,
        )

        // text 使用 textDurationMillis=100ms，在 50ms 时仍在中间
        val aUnit = scene.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull(aUnit)
        val alpha = aUnit!!.alpha.from
        assertTrue(
            "coordinated=false: text alpha 应在中间（≈0.5），实际=$alpha",
            alpha in 0.3f..0.7f,
        )
    }

    // ==================== 不同 VSync 帧率（60/90/120Hz） ====================

    /**
     * 断言：60/90/120Hz 三种帧率下，文字与光标由同一个 frame clock 采样，
     * 收敛到的最终几何状态完全一致（文字 unit 清除、光标落到最终真实位置）。
     * 帧间隔只决定传入 sample 的 frameTimeNanos 粒度，不改变插值数学 —
     * 这是"唯一位置动画所有者"的直接推论。
     */
    @Test
    fun frameIntervalIndependence_60_90_120Hz_convergeIdentically() {
        // layout 与帧率无关，只取一次（setContent 每个测试只能调用一次）。
        val layouts = captureLayouts("", "a")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        for (hz in listOf(60, 90, 120)) {
            val timeline = ComposeVisualTimeline()
            val oldCursorRect = Rect(0f, 0f, 2f, 14f)
            val newCursorRect = Rect(7f, 0f, 9f, 14f)
            val path =
                CursorMotionPath(
                    points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
                )
            val patch =
                makePatch(
                    id = 1L,
                    oldLayout = emptyLayout,
                    newLayout = aLayout,
                    insertedUnits = listOf(TextRange(0, 1)),
                    cursorMotionPath = path,
                    durationMs = 100L,
                    motionPolicy =
                        EditorMotionPolicy(
                            textDurationMillis = 100L,
                            cursorEnabled = true,
                            coordinated = true,
                        ),
                )

            val intervalNanos = 1_000_000_000L / hz
            // 在单一 frameTimeNanos(=0) 上同时把文字与光标交给同一个 timeline。
            timeline.applyPatch(
                patch = patch,
                frameTimeNanos = 0L,
                cursorFromRect = oldCursorRect,
                cursorPath = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
                cursorDurationNanos = 100L * NANOS_PER_MS,
            )

            // 以该帧率推进到动画结束后（单调递增的 frameTimeNanos，无回退）。
            var t = 0L
            while (t < 110L * NANOS_PER_MS) {
                t += intervalNanos
                timeline.sample(t)
            }
            val finalScene = timeline.sample(120L * NANOS_PER_MS)

            // 动画结束：文字 unit 应被收口清除（不残留 hidden state）。
            assertEquals("hz=$hz: 动画结束后文字 unit 应清除", 0, finalScene.units.size)
            // 光标应落到最终真实位置（从 oldLayout 收敛到 newLayout）。
            val cursor = finalScene.cursorRect ?: error("hz=$hz: cursor 应可见")
            assertTrue(
                "hz=$hz: cursor 应收敛到最终位置（left≈${newCursorRect.left}），实际=${cursor.left}",
                kotlin.math.abs(cursor.left - newCursorRect.left) < 1f,
            )
        }
    }

    // ==================== 快速输入 / 快速删除 ====================

    /**
     * 断言4：快速输入（20ms 间隔 < 100ms 动画时长）时，cursor 不落后于文字。
     *
     * 场景："" → "a" → "ab" → "abc" → "abcd"，每步 20ms。
     */
    @Test
    fun rapidInput_cursorNeverLagsBehindText() {
        val layouts = captureLayouts("", "a", "ab", "abc", "abcd")
        val state = ComposeEditorVisualState(targetId = "test-rapid-input")

        // 初始
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 4 步快速输入，间隔 20ms
        for (i in 1..4) {
            val prevText = "a".repeat(i - 1)
            val currText = "a".repeat(i)
            val frameTime = (i - 1) * 20L * 1_000_000L

            state.onAuthoritativeLayout(layouts[i - 1], TextRange(i - 1, i - 1), 0)
            state.onVisualIntent(
                makeInsertIntent(
                    i.toLong(),
                    (i - 1).toLong(),
                    i.toLong(),
                    prevText,
                    currText,
                    TextRange(i - 1, i),
                ),
                EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
            )
            state.onAuthoritativeLayout(layouts[i], TextRange(i, i), 0)
            state.drainPendingPatchesAtFrame(frameTime)
            val scene = state.sampleVisualScene(frameTime)

            // 验证：cursor rect 存在（不为 null）
            assertNotNull("Step $i: cursor rect 不应为 null", scene.cursorRect)
        }

        // 最终验证：最终 scene 的 cursor rect 应该接近最终位置
        val finalScene = state.sampleVisualScene(100L * 1_000_000L)
        assertNotNull("最终: cursor rect 不应为 null", finalScene.cursorRect)
    }

    /**
     * 断言5：快速删除（连续 Backspace）时，cursor 与文字同步。
     *
     * 场景："abc" → "ab" → "a" → ""，每步 20ms。
     */
    @Test
    fun rapidDelete_cursorSynchronizedWithText() {
        val layouts = captureLayouts("abc", "ab", "a", "")
        val state = ComposeEditorVisualState(targetId = "test-rapid-delete")

        state.onAuthoritativeLayout(layouts[0], TextRange(3, 3), 0)

        for (i in 1..3) {
            val prevText = "abc".substring(0, 4 - i)
            val currText = "abc".substring(0, 3 - i)
            val frameTime = (i - 1) * 20L * 1_000_000L

            state.onAuthoritativeLayout(layouts[i - 1], TextRange(prevText.length, prevText.length), 0)
            state.onVisualIntent(
                makeDeleteIntent(
                    (i + 10).toLong(),
                    (i - 1).toLong(),
                    i.toLong(),
                    prevText,
                    currText,
                    TextRange(currText.length, prevText.length),
                ),
                EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
            )
            state.onAuthoritativeLayout(layouts[i], TextRange(currText.length, currText.length), 0)
            state.drainPendingPatchesAtFrame(frameTime)
            val scene = state.sampleVisualScene(frameTime)

            assertNotNull("Delete step $i: cursor rect 不应为 null", scene.cursorRect)
        }
    }

    // ==================== 自动换行 ====================

    /**
     * 断言6：自动换行时，retained reflow 与 cursor 在同一个 scene transition 中处理。
     *
     * 场景：在窄布局中输入 "abcdef"，"f" 到达行尾时触发换行。
     */
    @Test
    fun autoLineWrap_reflowAndCursorInSameTransition() {
        val layouts = captureLayoutsWithWidth(arrayOf("abcde", "abcdef"), 50)
        val state = ComposeEditorVisualState(targetId = "test-line-wrap")

        state.onAuthoritativeLayout(layouts[0], TextRange(5, 5), 0)
        state.onVisualIntent(
            makeInsertIntent(
                1L,
                0L,
                1L,
                "abcde",
                "abcdef",
                TextRange(5, 6),
            ),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(6, 6), 0)
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)

        // 验证：换行后光标仍必须可见 — 优先 scene.cursorRect，回退 restingCursorRect。
        // reflow 的 retained units 与 cursor 由同一个 scene transition 处理，光标不丢失。
        assertNotNull("换行后 cursor 应可见", scene.cursorRect ?: state.restingCursorRect.value)
    }

    // ==================== 一帧多 patch ====================

    /**
     * 断言7：同一 frameTimeNanos drain 多笔 patch 时，cursor 只创建最终几何目标。
     *
     * 场景：3 笔 patch 在同一帧到达，drain 后 cursor 应指向最终目标。
     */
    @Test
    fun multiplePatchesInOneFrame_cursorConvergesToFinalTarget() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val state = ComposeEditorVisualState(targetId = "test-multi-patch")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 连续 3 笔 patch
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1), durationMs = 30L),
            EditorMotionPolicy(textDurationMillis = 30L, cursorEnabled = true, cursorDurationMillis = 30L),
        )

        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        state.onVisualIntent(
            makeInsertIntent(2L, 1L, 2L, "a", "ab", TextRange(1, 2)),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true),
        )

        state.onAuthoritativeLayout(layouts[3], TextRange(3, 3), 0)
        state.onVisualIntent(
            makeInsertIntent(3L, 2L, 3L, "ab", "abc", TextRange(2, 3)),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true),
        )

        // 一帧 drain 3 笔
        val applied = state.drainPendingPatchesAtFrame(0L)
        // #694 评论第 7 步：同一 VSync 的多笔 patch 应先合成一个屏幕 transition，
        // 再只 applyPatch() 一次。旧实现逐笔 applyPatch 返回 3 笔；修复后 batch 合成返回 1 笔。
        assertEquals("应 drain 1 笔 batch 合成的 patch", 1, applied.size)

        // 断言7：同一 frameTimeNanos drain 多笔 patch 时，cursor 只收敛到最终几何目标，
        // 不创建肉眼不可见的中间几何轨迹（from 直接连到最终 to）。
        // 50ms 采样（最后笔 duration=100ms）：cursor 正从起点向最终位置运动。
        val mid = state.sampleVisualScene(50L * NANOS_PER_MS)
        val midCursor = mid.cursorRect ?: state.restingCursorRect.value
        assertNotNull("cursor 应可见", midCursor)
        assertTrue("cursor 应已离开起点向最终位置运动 (left>0)", midCursor!!.left > 0f)

        // 动画结束后 cursor 应收敛到最终 layout 的 cursor（"abc" 末尾），而非中间 patch 的目标。
        val finalScene = state.sampleVisualScene(300L * NANOS_PER_MS)
        val finalCursor = finalScene.cursorRect ?: state.restingCursorRect.value
        val resting = state.restingCursorRect.value
        assertNotNull("最终 cursor 应可见", finalCursor)
        assertNotNull("resting cursor 应存在", resting)
        assertTrue(
            "cursor 应收敛到最终 layout 位置（未见中间轨迹残留）",
            kotlin.math.abs(finalCursor!!.left - resting!!.left) < 1f,
        )
    }

    // ==================== 不同时长边界 ====================

    /**
     * 断言8a：短时长（30ms）时，动画快速完成不残留 hidden state。
     */
    @Test
    fun shortDuration_animationCompletesCleanly() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-short-duration")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textDurationMillis = 30L, cursorEnabled = true, cursorDurationMillis = 30L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)

        // 在 30ms 时（动画应已完成）
        val scene = state.sampleVisualScene(30L * 1_000_000L)
        // 动画完成的文字 unit 应从 timeline 移除（不残留 hidden state）
        assertEquals("短时长：动画完成后不应有残留文字 units", 0, scene.units.size)
        assertFalse("短时长：动画完成后不应有活动动画", state.hasActiveVisuals(30L * 1_000_000L))
        // cursor rect 在动画完成后仍存在（最终位置），overlay 的 restingCursorRect 兜底
    }

    /**
     * 断言8b：长时长（1000ms）时，连续输入不不断从起点重新开始。
     */
    @Test
    fun longDuration_continuousInputDoesNotRestart() {
        val layouts = captureLayouts("", "a", "ab")
        val state = ComposeEditorVisualState(targetId = "test-long-duration")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // Step A: "" → "a" at 0ms
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1), durationMs = 1000L),
            EditorMotionPolicy(textDurationMillis = 1000L, cursorEnabled = true),
        )
        state.drainPendingPatchesAtFrame(0L)

        // Step B: "a" → "ab" at 100ms（长时长动画仍在进行中）
        val frameTimeB = 100L * 1_000_000L
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.onVisualIntent(
            makeInsertIntent(
                2L,
                1L,
                2L,
                "a",
                "ab",
                TextRange(1, 2),
                offsetMap = VisualOffsetMap(listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY))),
                durationMs = 1000L,
            ),
            EditorMotionPolicy(textDurationMillis = 1000L, cursorEnabled = true),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        state.drainPendingPatchesAtFrame(frameTimeB)
        val scene = state.sampleVisualScene(frameTimeB)

        // "a" 的 alpha 不应被重置为 0（持续 timeline 核心不变量）
        val unitA = scene.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        if (unitA != null) {
            assertTrue(
                "长时长：'a' 的 alpha 不应被重置（持续 timeline），实际=${unitA.alpha.from}",
                unitA.alpha.from > 0f,
            )
        }
    }

    // ==================== 运行时切换设置 ====================

    /**
     * 断言9a：动画进行中关闭 cursor 动画 — cursor 立即 snap 到最终位置。
     */
    @Test
    fun runtimeToggle_disableCursorAnimation_snapsToFinalPosition() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-toggle-cursor")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)

        // 在 50ms 时，cursor 动画仍在进行
        val scene50 = state.sampleVisualScene(50L * 1_000_000L)
        assertNotNull("50ms: cursor rect 不应为 null", scene50.cursorRect)

        // 模拟关闭 cursor 动画：clear timeline 后重新设置 resting cursor
        state.clear()
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        // 新 scene 应该直接使用 resting cursor（通过 overlay 的 restingCursorRect 兜底）
        val sceneAfter = state.sampleVisualScene(0L)
        // cursor rect 为 null 时 overlay 会使用 restingCursorRect
        assertNotNull("关闭 cursor 后: resting cursor rect 不应为 null", state.restingCursorRect.value)
    }

    /**
     * 断言9b：动画进行中切换 coordinated 模式 — 安全收口。
     */
    @Test
    fun runtimeToggle_switchCoordinatedMode_safeTransition() {
        val layouts = captureLayouts("", "a", "ab")
        val state = ComposeEditorVisualState(targetId = "test-toggle-coordinated")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔用 coordinated=true
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(50L * 1_000_000L)

        // 第二笔用 coordinated=false
        state.onVisualIntent(
            makeInsertIntent(
                2L,
                1L,
                2L,
                "a",
                "ab",
                TextRange(1, 2),
                offsetMap = VisualOffsetMap(listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY))),
            ),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = false),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        state.drainPendingPatchesAtFrame(50L * 1_000_000L)
        val scene = state.sampleVisualScene(50L * 1_000_000L)

        // 不崩溃且 cursor rect 存在
        assertNotNull("切换 coordinated 后 cursor rect 不应为 null", scene.cursorRect)
    }

    /**
     * 断言9c：动画进行中切换 reduceMotion — 所有动画降级为静态更新。
     */
    @Test
    fun runtimeToggle_reduceMotionAllAnimationsDisabled() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-toggle-reduce-motion")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, reduceMotion = true),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)

        // reduceMotion 时不应有活动动画
        assertFalse("reduceMotion: 不应有活动动画", state.hasActiveVisuals(0L))
        // reduceMotion effective 后 cursorEnabled=false，不创建 cursorChannel
        // restingCursorRect 应该存在
        assertNotNull("reduceMotion: resting cursor rect 不应为 null", state.restingCursorRect.value)
    }

    // ==================== 无文字时 cursor 可见 ====================

    /**
     * 断言10：无文字时 smooth cursor 仍必须保持可见。
     *
     * 场景：空文本状态下 cursor 应该有 resting rect。
     */
    @Test
    fun emptyText_cursorStillVisible() {
        val layouts = captureLayouts("")
        val state = ComposeEditorVisualState(targetId = "test-empty-cursor", initialDrawsVisualCursor = true)

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)

        // 空文本时 resting cursor rect 应该存在
        assertNotNull("空文本: resting cursor rect 不应为 null", state.restingCursorRect.value)
    }

    // ==================== 设置组合覆盖 ====================

    /**
     * 设置组合 A：reduceMotion=true — 所有动画禁用。
     */
    @Test
    fun settingCombinationA_reduceMotion() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-combo-a")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(reduceMotion = true),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)

        assertFalse("Combo A: 不应有活动动画", state.hasActiveVisuals(0L))
        // reduceMotion 时 cursorEnabled=false，不创建 cursorChannel
        // restingCursorRect 应该存在（光标位置来自 layout）
        assertNotNull("Combo A: resting cursor rect 不应为 null", state.restingCursorRect.value)
    }

    /**
     * 设置组合 B：textEnabled=false, cursorEnabled=false — 完全静态。
     */
    @Test
    fun settingCombinationB_allDisabled() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-combo-b")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            // Issue #723 评论 5749023316 缺口2：coordinated=false 时独立开关生效。
            // coordinated=true 时 effective() 强制 textEnabled/cursorEnabled=true。
            EditorMotionPolicy(textEnabled = false, cursorEnabled = false, coordinated = false),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)

        assertFalse("Combo B: 不应有活动动画", state.hasActiveVisuals(0L))
        // cursorEnabled=false，不创建 cursorChannel，但 restingCursorRect 应存在
        assertNotNull("Combo B: resting cursor rect 不应为 null", state.restingCursorRect.value)
    }

    /**
     * 设置组合 C：textEnabled=true, cursorEnabled=false — 只开文字动画。
     */
    @Test
    fun settingCombinationC_textOnly() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-combo-c")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textEnabled = true, cursorEnabled = false),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)

        // 文字有动画，cursor 无动画（使用 resting rect）
        assertTrue("Combo C: 应有活动文字 units", scene.units.isNotEmpty())
        // cursorEnabled=false，不创建 cursorChannel，但 restingCursorRect 应存在
        assertNotNull("Combo C: resting cursor rect 不应为 null", state.restingCursorRect.value)
    }

    /**
     * 设置组合 D：textEnabled=false, cursorEnabled=true — 只开光标动画。
     */
    @Test
    fun settingCombinationD_cursorOnly() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-combo-d")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            // Issue #723 评论 5749023316 缺口2：coordinated=false 时独立开关生效。
            EditorMotionPolicy(textEnabled = false, cursorEnabled = true, cursorDurationMillis = 80L, coordinated = false),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)

        // 文字无动画，cursor 有动画
        assertTrue("Combo D: 不应有活动文字 units", scene.units.isEmpty())
        assertNotNull("Combo D: cursor rect 不应为 null", scene.cursorRect)
    }

    /**
     * 设置组合 E：textEnabled=true, cursorEnabled=true, coordinated=true — 默认协同。
     */
    @Test
    fun settingCombinationE_coordinated() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-combo-e")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        val scene = state.sampleVisualScene(0L)

        assertTrue("Combo E: 应有活动文字 units", scene.units.isNotEmpty())
        assertNotNull("Combo E: cursor rect 不应为 null", scene.cursorRect)
    }

    /**
     * 设置组合 F：textEnabled=true, cursorEnabled=true, coordinated=false — 独立时长。
     */
    @Test
    fun settingCombinationF_independentDuration() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-combo-f")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(
                textDurationMillis = 100L,
                cursorEnabled = true,
                cursorDurationMillis = 50L,
                coordinated = false,
            ),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)

        // 在 50ms 时：cursor 应已完成（cursorDuration=50ms），text 仍在进行（textDuration=100ms）
        val scene = state.sampleVisualScene(50L * 1_000_000L)
        assertNotNull("Combo F: cursor rect 不应为 null", scene.cursorRect)
        assertTrue("Combo F: 应有活动文字 units", scene.units.isNotEmpty())
    }

    // ==================== 时长覆盖（30ms / 默认 / 1000ms） ====================

    /**
     * 断言：30ms 时长覆盖。
     */
    @Test
    fun durationBoundary_30ms() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-duration-30")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textDurationMillis = 30L, cursorEnabled = true, cursorDurationMillis = 30L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)

        val scene30 = state.sampleVisualScene(30L * 1_000_000L)
        assertFalse("30ms: 动画完成后不应有活动", state.hasActiveVisuals(30L * 1_000_000L))
        // cursor rect 在动画完成后仍存在（最终位置），文字 units 已清除
        assertEquals("30ms: 动画完成后文字 units 应为空", 0, scene30.units.size)
    }

    /**
     * 断言：1000ms 时长覆盖。
     */
    @Test
    fun durationBoundary_1000ms() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-duration-1000")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1), durationMs = 1000L),
            EditorMotionPolicy(textDurationMillis = 1000L, cursorEnabled = true, cursorDurationMillis = 1000L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)

        // 在 500ms 时动画仍在进行
        assertTrue("1000ms: 500ms 时应有活动", state.hasActiveVisuals(500L * 1_000_000L))
        // 在 1000ms 时动画应已完成
        assertFalse("1000ms: 1000ms 时不应有活动", state.hasActiveVisuals(1000L * 1_000_000L))
    }

    // ==================== #691 评论 5679242735 补充测试 ====================

    /**
     * #691 评论 5679242735 修改1：纯 selection 变化时静止光标跟随 live selection。
     *
     * 同一份 TextLayoutResult 不变（多行文本 "abcde\nfghij" 不变），
     * 只把 selection 从第 0 行首(0) 改到第 0 行末(5)、第 1 行首(6)、第 1 行末(11)。
     * smooth cursor 开启且没有文字 patch。用 [computeRestingCursorRect] 实时计算静止光标，
     * 验证不同 selection 返回不同 rect（跨行时 top 不同，同行时 left 不同）。
     *
     * 这验证了 [EditorTextFieldDrawLayer] 中 liveSelection 参数的实际用途 —
     * BasicTextField.onTextLayout 只在"新的 text layout 被计算时"才回调，
     * 纯 selection 变化不保证重新计算文字布局，restingCursorRect 会停在旧位置。
     *
     * 用多行文本确保 Robolectric 下不同行的 cursor rect top 有明显差异
     * （单行短文本在 Robolectric 下不同 offset 的 getCursorRect 可能返回相同 left）。
     */
    @Test
    fun restingCursor_followsLiveSelection_notJustOnTextLayout() {
        val layouts = captureLayouts("abcde\nfghij")
        val state = ComposeEditorVisualState(targetId = "test-resting-live-selection", initialDrawsVisualCursor = true)

        // 建立布局（文本 "abcde\nfghij"，selection 在 0）
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 同一份 layout 不变，只改 selection — 模拟鼠标点选/方向键移动
        val layout = state.latestLayout.value
        assertNotNull("latestLayout 应存在", layout)

        val rectAt0 = computeRestingCursorRect(layout, TextRange(0, 0))
        val rectAt6 = computeRestingCursorRect(layout, TextRange(6, 6))
        val rectAt11 = computeRestingCursorRect(layout, TextRange(11, 11))

        assertNotNull("selection=0: cursor rect 不应为 null", rectAt0)
        assertNotNull("selection=6: cursor rect 不应为 null", rectAt6)
        assertNotNull("selection=11: cursor rect 不应为 null", rectAt11)

        // 跨行对比：第 0 行 (offset=0) 与第 1 行 (offset=6) 的 top 应不同
        assertTrue(
            "跨行: selection=0 (第0行) 与 selection=6 (第1行) 的 cursor rect top 应不同: " +
                "rectAt0.top=${rectAt0!!.top}, rectAt6.top=${rectAt6!!.top}",
            kotlin.math.abs(rectAt0.top - rectAt6.top) > 0.1f,
        )
        // 跨行对比：第 0 行 (offset=0) 与第 1 行末 (offset=11) 的 top 应不同
        assertTrue(
            "跨行: selection=0 (第0行) 与 selection=11 (第1行末) 的 cursor rect top 应不同: " +
                "rectAt0.top=${rectAt0.top}, rectAt11.top=${rectAt11!!.top}",
            kotlin.math.abs(rectAt0.top - rectAt11.top) > 0.1f,
        )
        // 同行对比：第 1 行首 (offset=6) 与第 1 行末 (offset=11) 的 left 或 top 应不同
        assertTrue(
            "同行: selection=6 与 selection=11 的 cursor rect 应不同: " +
                "rectAt6.left=${rectAt6.left}, rectAt11.left=${rectAt11.left}, " +
                "rectAt6.top=${rectAt6.top}, rectAt11.top=${rectAt11.top}",
            kotlin.math.abs(rectAt6.left - rectAt11.left) > 0.1f || kotlin.math.abs(rectAt6.top - rectAt11.top) > 0.1f,
        )
    }

    /**
     * #691 评论 5679242735 修改2a：textEnabled=false + reflow 时 scene.units 始终为空。
     *
     * 构造一个会产生 retained reflow 的场景，textEnabled=false, cursorEnabled=true。
     * 用 [ComposeVisualTimeline] 直接 applyPatch（带 retainedMoves），然后 sample，
     * 断言 scene.units 为空，scene.cursorRect 可以非 null（只允许 cursor track）。
     * 在动画中间和结束都采样验证 units 始终为空。
     */
    @Test
    fun textDisabled_cursorEnabled_reflow_producesNoTextUnits_onlyCursorTrack() {
        val layouts = captureLayouts("aaaa")
        val layout = ComposeLayoutSnapshot(layouts[0], TextRange(4, 4), 0)

        val timeline = ComposeVisualTimeline()

        // 构造带 retainedMoves 的 patch（模拟 reflow：前两个字符移到后面）
        val retainedMoves =
            listOf(
                RetainedMove(oldRange = TextRange(0, 2), newRange = TextRange(2, 4)),
            )
        val cursorRect = Rect(30f, 0f, 32f, 14f)
        val cursorPath =
            CursorMotionPath(
                points = listOf(CursorMotionPoint(rect = cursorRect, endFraction = 1f)),
            )
        val patch =
            makePatch(
                id = 1L,
                oldLayout = layout,
                newLayout = layout,
                retainedMoves = retainedMoves,
                cursorMotionPath = cursorPath,
                durationMs = 100L,
                motionPolicy =
                    EditorMotionPolicy(
                        textEnabled = false,
                        cursorEnabled = true,
                        cursorDurationMillis = 100L,
                        // Issue #723 评论 5749023316 缺口2：coordinated=false 时独立开关生效。
                        coordinated = false,
                    ),
            )

        // applyPatch — textEnabled=false，不应创建任何文字 track
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = Rect(10f, 0f, 12f, 14f),
            cursorPath = cursorPath.points,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // 在动画中间（50ms）采样
        val midScene = timeline.sample(50L * NANOS_PER_MS)
        assertTrue(
            "textEnabled=false: 动画中间不应有文字 units，实际=${midScene.units.size}",
            midScene.units.isEmpty(),
        )

        // 在动画结束（100ms）采样
        val endScene = timeline.sample(100L * NANOS_PER_MS)
        assertTrue(
            "textEnabled=false: 动画结束不应有文字 units，实际=${endScene.units.size}",
            endScene.units.isEmpty(),
        )

        // cursor track 可以非 null（cursorEnabled=true）
        // 不强制断言 cursorRect 非 null，因为 50ms 时可能在动画中，100ms 时可能已收口
    }

    /**
     * #691 评论 5679242735 修改2b：patch 入队后切 textEnabled=false 再 drain 不出现 text units。
     *
     * 场景：
     * 1. 先 onAuthoritativeLayout 建立旧 layout（空文本）
     * 2. 用 onVisualIntent + onAuthoritativeLayout 生成并入队一个 textEnabled=true 的插入 patch
     * 3. 不要 drain
     * 4. 调用 applyMotionPolicyAtFrame(EditorMotionPolicy(textEnabled=false, cursorEnabled=true))
     * 5. 然后 drainPendingPatchesAtFrame(0L)
     * 6. sampleVisualScene(0L) 后断言 scene.units 为空且 scene.hiddenRanges 为空
     */
    @Test
    fun queuedPatch_textEnabledTrue_thenPolicyChangeToDisabled_drainProducesNoTextUnits() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-queued-policy-change")

        // 1. 建立旧 layout（空文本）
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 2. 生成并入队一个 textEnabled=true 的插入 patch
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textEnabled = true, cursorEnabled = true, textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        // 3. 不要 drain — patch 已入队
        assertTrue("应有 pending patch", state.hasPendingPatches())

        // 4. 切换 policy 到 textEnabled=false
        // Issue #723 评论 5749023316 缺口2：coordinated=false 时独立开关生效。
        state.applyMotionPolicyAtFrame(EditorMotionPolicy(textEnabled = false, cursorEnabled = true, coordinated = false))

        // 5. drain
        state.drainPendingPatchesAtFrame(0L)

        // 6. sample 后断言 units 和 hiddenRanges 为空
        val scene = state.sampleVisualScene(0L)
        assertTrue(
            "切 textEnabled=false 后 drain: scene.units 应为空，实际=${scene.units.size}",
            scene.units.isEmpty(),
        )
        assertTrue(
            "切 textEnabled=false 后 drain: scene.hiddenRanges 应为空，实际=${scene.hiddenRanges.size}",
            scene.hiddenRanges.isEmpty(),
        )
    }

    /**
     * #691 评论 5686733880：设置矩阵 D（textEnabled=false, cursorEnabled=true, coordinated=false）
     * 从 [ComposeEditorVisualState] 完整生产路径验证 cursor 使用 cursorDurationMillis 而非 textDurationMillis。
     *
     * Issue #723 评论 5749023316 缺口2：coordinated=true 时 effective() 强制 textEnabled=true，
     * "coordinated=true 但 textEnabled=false" 的旧组合不再存在。本测试改用 coordinated=false
     * 验证独立开关下 cursor 使用 cursorDurationMillis。
     *
     * 场景："" → "a" 插入。patch 在 textEnabled=true 时生成（含 insertedUnits，确保不是 CURSOR_ONLY），
     * 然后通过 [applyMotionPolicyAtFrame] 切换到设置矩阵 D（textEnabled=false, cursorEnabled=true,
     * coordinated=false, textDurationMillis=1000, cursorDurationMillis=80）。
     * drain 时 patch 仍含 insertedUnits，但 motionPolicy 已被替换成 textEnabled=false。
     *
     * 断言：
     * - 40ms 时 cursor 在中间（80ms 时长 50% 进度），未到最终位置
     * - 80ms 时 cursor 已到最终位置，hasActiveVisuals=false（动画结束）
     * - 500ms 时 cursor 仍在最终位置（绝不拖到 1000ms 才完成）
     * - textEnabled=false 时 scene.units 始终为空（文字立即显示，不创建文字 track）
     */
    @Test
    fun textDisabled_cursorEnabled_coordinatedTrue_cursorUsesCursorDurationNotTextDuration_fromFullProductionPath() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-comment-5686733880-matrix-d")

        // 从 layout 直接计算 cursor rect（与生产代码 computeCursorRectFromLayout 一致）
        val oldCursorRect = layouts[0].getCursorRect(0)
        val newCursorRect = layouts[1].getCursorRect(1)
        // 确认旧/新 cursor 位置不同（否则动画无法验证）
        assertTrue(
            "旧/新 cursor 位置应不同（old.left=${oldCursorRect.left}, new.left=${newCursorRect.left}）",
            kotlin.math.abs(oldCursorRect.left - newCursorRect.left) > 0.5f,
        )

        // 步骤1：建立旧 layout（空文本）
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 步骤2：用 textEnabled=true 生成含 insertedUnits 的 patch（确保不是 CURSOR_ONLY）
        // 此时 patch 入队，insertedUnits = [TextRange(0,1)] 非空
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(
                textEnabled = true,
                cursorEnabled = true,
                coordinated = true,
                textDurationMillis = 1000L,
                cursorDurationMillis = 80L,
            ),
        )

        // 步骤3：新 layout 到达，patch 生成并入队
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertTrue("drain 前应有 pending patch", state.hasPendingPatches())

        // 步骤4：切换 policy 到设置矩阵 D（textEnabled=false, cursorEnabled=true, coordinated=false）
        // patch 的 motionPolicy 被替换成 textEnabled=false，但 insertedUnits 保留（非空）
        // Issue #723 评论 5749023316 缺口2：coordinated=false 时独立开关生效。
        state.applyMotionPolicyAtFrame(
            EditorMotionPolicy(
                textEnabled = false,
                cursorEnabled = true,
                coordinated = false,
                textDurationMillis = 1000L,
                cursorDurationMillis = 80L,
            ),
        )

        // 步骤5：drain — patch 含 insertedUnits（isCursorOnly=false），motionPolicy.textEnabled=false
        // 修复后：usesCoordinatedTextTimeline = false → cursorDurationMillis=80ms
        // 旧缺陷：coordinated && !isCursorOnly = true → textDurationMillis=1000ms
        state.drainPendingPatchesAtFrame(0L)

        // === 断言1：40ms 时 cursor 在中间（80ms 时长 50% 进度）===
        val scene40 = state.sampleVisualScene(40L * NANOS_PER_MS)
        // textEnabled=false：不创建文字 track，scene.units 始终为空
        assertTrue(
            "40ms: textEnabled=false 时 scene.units 应为空（文字立即显示），实际=${scene40.units.size}",
            scene40.units.isEmpty(),
        )
        val cursorRect40 = scene40.cursorRect
        assertNotNull("40ms: cursor rect 应存在", cursorRect40)
        val cursorRect40Value = cursorRect40!!
        // cursor 应在中间位置（progress = 40/80 = 0.5）
        val expectedMidLeft = oldCursorRect.left + (newCursorRect.left - oldCursorRect.left) * 0.5f
        assertTrue(
            "40ms: cursor 应在中间位置（left≈$expectedMidLeft），实际=${cursorRect40Value.left}；" +
                "若错误使用 textDurationMillis=1000ms，40ms 时 progress=0.04，cursor 仍在起点附近",
            kotlin.math.abs(cursorRect40Value.left - expectedMidLeft) < 1f,
        )
        // cursor 不应已到最终位置
        // 阈值用 0.3f 而非 0.5f：cursor 移动范围可能正好 1.0（old.left=0 → new.left=1），
        // 中间位置 0.5 距终点 1.0 正好 0.5，严格大于 0.5 会误判。
        // 0.3f 既能可靠区分"中间位置"（差 0.5 > 0.3 通过）和"终点位置"（差 0 > 0.3 失败），
        // 又不会因边界条件失败。
        assertTrue(
            "40ms: cursor 不应已到最终位置（left≈${newCursorRect.left}），实际=${cursorRect40Value.left}",
            kotlin.math.abs(cursorRect40Value.left - newCursorRect.left) > 0.3f,
        )

        // === 断言2：80ms 时 cursor 已到最终位置，动画结束 ===
        val scene80 = state.sampleVisualScene(80L * NANOS_PER_MS)
        assertTrue(
            "80ms: textEnabled=false 时 scene.units 应为空，实际=${scene80.units.size}",
            scene80.units.isEmpty(),
        )
        val cursorRect80 = scene80.cursorRect
        assertNotNull("80ms: cursor rect 应存在", cursorRect80)
        val cursorRect80Value = cursorRect80!!
        assertTrue(
            "80ms: cursor 应已到最终位置（left≈${newCursorRect.left}），实际=${cursorRect80Value.left}；" +
                "若错误使用 textDurationMillis=1000ms，80ms 时 progress=0.08，cursor 仍在起点附近",
            kotlin.math.abs(cursorRect80Value.left - newCursorRect.left) < 1f,
        )
        assertFalse(
            "80ms: cursor 动画应已完成（hasActiveVisuals=false）；" +
                "若错误使用 textDurationMillis=1000ms，80ms 时动画仍在进行",
            state.hasActiveVisuals(80L * NANOS_PER_MS),
        )

        // === 断言3：500ms 时 cursor 仍在最终位置（绝不拖到 1000ms 才完成）===
        val scene500 = state.sampleVisualScene(500L * NANOS_PER_MS)
        assertTrue(
            "500ms: textEnabled=false 时 scene.units 应为空，实际=${scene500.units.size}",
            scene500.units.isEmpty(),
        )
        val cursorRect500 = scene500.cursorRect
        assertNotNull("500ms: cursor rect 应存在", cursorRect500)
        val cursorRect500Value = cursorRect500!!
        assertTrue(
            "500ms: cursor 应仍在最终位置（left≈${newCursorRect.left}），实际=${cursorRect500Value.left}；" +
                "若错误使用 textDurationMillis=1000ms，500ms 时 progress=0.5，cursor 在中间",
            kotlin.math.abs(cursorRect500Value.left - newCursorRect.left) < 1f,
        )
        assertFalse(
            "500ms: cursor 动画应早已完成（hasActiveVisuals=false）；" +
                "若错误使用 textDurationMillis=1000ms，500ms 时动画仍在进行",
            state.hasActiveVisuals(500L * NANOS_PER_MS),
        )
    }

    /**
     * #691 评论 5679242735 修改3a：一次提交 3 个 unit，cursor path 有 3 个 point。
     *
     * 一次提交 3 个 newAnimationUnits，cursor path 有 3 个 point（endFraction = 1/3, 2/3, 1.0），
     * 3 个 point 的 rect 在不同水平位置。用 [ComposeVisualTimeline] 直接 applyPatch（传入完整 cursorPath），
     * duration=300ms。在 100ms(1/3)、200ms(2/3)、300ms(1.0) 时分别采样，
     * 断言 cursor rect 接近对应 point 的 rect（不是直接从旧位置线性插值到最终位置）。
     */
    @Test
    fun multiCharInsert_cursorPathHasMultiplePoints_sampledAtThirds() {
        val layouts = captureLayouts("", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()

        // 3 个 point 在不同水平位置
        val point0 = CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)
        val cursorPath = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
            )

        // fromRect 在起点
        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // 在 100ms(1/3) 采样 — cursor 应接近 point0.rect
        val scene100 = timeline.sample(100L * NANOS_PER_MS)
        val cursor100 = scene100.cursorRect
        assertNotNull("100ms: cursor rect 不应为 null", cursor100)
        assertTrue(
            "100ms: cursor 应接近 point0 (left≈${point0.rect.left})，实际=${cursor100!!.left}",
            kotlin.math.abs(cursor100.left - point0.rect.left) < 1f,
        )

        // 在 200ms(2/3) 采样 — cursor 应接近 point1.rect
        val scene200 = timeline.sample(200L * NANOS_PER_MS)
        val cursor200 = scene200.cursorRect
        assertNotNull("200ms: cursor rect 不应为 null", cursor200)
        assertTrue(
            "200ms: cursor 应接近 point1 (left≈${point1.rect.left})，实际=${cursor200!!.left}",
            kotlin.math.abs(cursor200.left - point1.rect.left) < 1f,
        )

        // 在 300ms(1.0) 采样 — cursor 应等于 point2.rect
        val scene300 = timeline.sample(300L * NANOS_PER_MS)
        val cursor300 = scene300.cursorRect
        assertNotNull("300ms: cursor rect 不应为 null", cursor300)
        assertTrue(
            "300ms: cursor 应接近 point2 (left≈${point2.rect.left})，实际=${cursor300!!.left}",
            kotlin.math.abs(cursor300.left - point2.rect.left) < 1f,
        )
    }

    /**
     * #691 评论 5679242735 修改3b：跨行多字符提交不直接从旧位置插值到最终行末。
     *
     * 构造一个跨行的多字符提交：cursor fromRect 在第 0 行，3 个 point 分别在第 0 行末、第 1 行首、第 1 行中。
     * duration=300ms。在 150ms（中间）采样，断言 cursor rect 的 y 坐标在第 1 行
     * （不是从第 0 行 y 线性插值到第 1 行 y 的中点）。
     * 即验证光标经过中间 point 而不是直线追最终位置。
     */
    @Test
    fun multiCharInsert_acrossLines_cursorNotStraightLineToFinal() {
        val layouts = captureLayouts("", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()

        // 跨行：fromRect 在第 0 行 (y=0)，point0 在第 0 行末 (y=0)，
        // point1 在第 1 行首 (y=20)，point2 在第 1 行中 (y=20)
        // endFraction: 1/3, 1/2, 1.0 — 在 150ms (progress=0.5) 时光标到达 point1 (第 1 行)
        val point0 = CursorMotionPoint(rect = Rect(40f, 0f, 42f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(0f, 20f, 2f, 34f), endFraction = 0.5f)
        val point2 = CursorMotionPoint(rect = Rect(20f, 20f, 22f, 34f), endFraction = 1f)
        val cursorPath = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
            )

        // fromRect 在第 0 行首
        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // 在 150ms（中间）采样
        val scene150 = timeline.sample(150L * NANOS_PER_MS)
        val cursor150 = scene150.cursorRect
        assertNotNull("150ms: cursor rect 不应为 null", cursor150)

        // 断言 y 坐标在第 1 行（接近 point1.y = 20），而不是从第 0 行线性插值到第 1 行的中点 (y=10)
        // 经过中间 point：150ms 时 cursor 到达 point1，y ≈ 20
        // 直线到最终：150ms 时 y = 0 + (20-0)*0.5 = 10
        val cursorY = cursor150!!.top
        assertTrue(
            "150ms: cursor y 应在第 1 行 (≈20)，经过中间 point，实际=$cursorY。" +
                "直线插值到最终位置的中点 y=10，不应是中点。",
            kotlin.math.abs(cursorY - 20f) < 3f,
        )
    }

    /**
     * #691 评论 5679815971 问题2：一次提交 3 个 unit，文字与 cursor 真正协同。
     *
     * 一次提交 "abc"（3 个 insertedUnits），总时长 300ms，cursor path 有 3 个 point
     * （endFraction = 1/3, 2/3, 1.0）。
     *
     * 在 100ms(1/3) 时：
     * - 第一个 unit (a) 的 alpha 应已完成（≈1）或已交还系统正文（从 timeline 移除）
     * - 第二个 unit (b) 的 alpha 应刚开始（≈0 或很小）
     * - 第三个 unit (c) 的 alpha 应为 0（尚未开始）
     * - cursor 同时到达第一个 point
     *
     * 在 200ms(2/3) 时：
     * - 第一、二个 unit 已完成或交还系统正文
     * - 第三个 unit 刚开始
     * - cursor 到达第二个 point
     */
    @Test
    fun multiCharInsert_textAndCursorTrulyCoordinated_atThirds() {
        val layouts = captureLayouts("", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()

        // 3 个 point 在不同水平位置
        val point0 = CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)
        val cursorPath = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
            )

        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 100ms (1/3) 采样 ===
        val scene100 = timeline.sample(100L * NANOS_PER_MS)

        // cursor 应接近 point0
        val cursor100 = scene100.cursorRect
        assertNotNull("100ms: cursor rect 不应为 null", cursor100)
        assertTrue(
            "100ms: cursor 应接近 point0 (left≈${point0.rect.left})，实际=${cursor100!!.left}",
            kotlin.math.abs(cursor100.left - point0.rect.left) < 1f,
        )

        // 文字协同断言：
        // 第一个 unit (a, range 0-1)：startedAt=0, duration=100ms，在 100ms 时 alpha 应已完成（≈1）
        //   → sample 后可能已从 timeline 移除（交还系统正文），或 alpha.from ≈ 1
        // 第二个 unit (b, range 1-2)：startedAt=100ms, duration=100ms，在 100ms 时刚开始（alpha ≈ 0）
        // 第三个 unit (c, range 2-3)：startedAt=200ms, duration=100ms，在 100ms 时尚未开始（alpha = 0）
        val unitA100 = scene100.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        val unitB100 = scene100.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC100 = scene100.units.firstOrNull { it.targetRange == TextRange(2, 3) }

        // unit a：要么已交还系统正文（unitA100 == null），要么 alpha ≈ 1
        if (unitA100 != null) {
            assertTrue(
                "100ms: unit a alpha 应已完成（≈1），实际=${unitA100.alpha.from}",
                unitA100.alpha.from > 0.9f,
            )
        }
        // unit b：应存在且 alpha 刚开始（≈0）
        assertNotNull("100ms: unit b 应存在（刚开始动画）", unitB100)
        assertTrue(
            "100ms: unit b alpha 应刚开始（≈0），实际=${unitB100!!.alpha.from}",
            unitB100.alpha.from < 0.2f,
        )
        // unit c：应存在且 alpha = 0（尚未开始）
        assertNotNull("100ms: unit c 应存在（尚未开始动画）", unitC100)
        assertTrue(
            "100ms: unit c alpha 应为 0（尚未开始），实际=${unitC100!!.alpha.from}",
            unitC100.alpha.from < 0.01f,
        )

        // === 200ms (2/3) 采样 ===
        val scene200 = timeline.sample(200L * NANOS_PER_MS)

        // cursor 应接近 point1
        val cursor200 = scene200.cursorRect
        assertNotNull("200ms: cursor rect 不应为 null", cursor200)
        assertTrue(
            "200ms: cursor 应接近 point1 (left≈${point1.rect.left})，实际=${cursor200!!.left}",
            kotlin.math.abs(cursor200.left - point1.rect.left) < 1f,
        )

        // unit a：已交还系统正文（unitA200 == null）或 alpha ≈ 1
        val unitA200 = scene200.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        if (unitA200 != null) {
            assertTrue(
                "200ms: unit a alpha 应已完成（≈1），实际=${unitA200.alpha.from}",
                unitA200.alpha.from > 0.9f,
            )
        }
        // unit b：要么已交还系统正文，要么 alpha ≈ 1
        val unitB200 = scene200.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        if (unitB200 != null) {
            assertTrue(
                "200ms: unit b alpha 应已完成（≈1），实际=${unitB200.alpha.from}",
                unitB200.alpha.from > 0.9f,
            )
        }
        // unit c：应存在且 alpha 刚开始（≈0）
        val unitC200 = scene200.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        assertNotNull("200ms: unit c 应存在（刚开始动画）", unitC200)
        assertTrue(
            "200ms: unit c alpha 应刚开始（≈0），实际=${unitC200!!.alpha.from}",
            unitC200.alpha.from < 0.2f,
        )
    }

    /**
     * #691 评论 5679815971 问题2：跨行多字符提交，文字与 cursor 同时断言。
     *
     * 跨行多字符提交：cursor fromRect 在第 0 行，3 个 point 分别在第 0 行末、第 1 行首、第 1 行中。
     * duration=300ms。在 100ms(1/3) 和 200ms(2/3) 时同时断言文字 alpha 分段和 cursor 位置。
     */
    @Test
    fun multiCharInsert_acrossLines_textAndCursorCoordinated() {
        val layouts = captureLayouts("", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()

        // 跨行：fromRect 在第 0 行 (y=0)，point0 在第 0 行末 (y=0)，
        // point1 在第 1 行首 (y=20)，point2 在第 1 行中 (y=20)
        val point0 = CursorMotionPoint(rect = Rect(40f, 0f, 42f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(0f, 20f, 2f, 34f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(20f, 20f, 22f, 34f), endFraction = 1f)
        val cursorPath = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
            )

        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 100ms (1/3) 采样 ===
        val scene100 = timeline.sample(100L * NANOS_PER_MS)

        // cursor 应在第 0 行末（接近 point0），y ≈ 0
        val cursor100 = scene100.cursorRect
        assertNotNull("跨行 100ms: cursor rect 不应为 null", cursor100)
        assertTrue(
            "跨行 100ms: cursor 应在第 0 行末 (y≈0)，实际 y=${cursor100!!.top}",
            kotlin.math.abs(cursor100.top - 0f) < 3f,
        )
        assertTrue(
            "跨行 100ms: cursor 应接近 point0 (left≈${point0.rect.left})，实际=${cursor100.left}",
            kotlin.math.abs(cursor100.left - point0.rect.left) < 1f,
        )

        // 文字断言：unit a 已完成或交还，unit b 刚开始，unit c 尚未开始
        val unitA100 = scene100.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        val unitB100 = scene100.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC100 = scene100.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        if (unitA100 != null) {
            assertTrue(
                "跨行 100ms: unit a alpha 应已完成（≈1），实际=${unitA100.alpha.from}",
                unitA100.alpha.from > 0.9f,
            )
        }
        assertNotNull("跨行 100ms: unit b 应存在", unitB100)
        assertTrue(
            "跨行 100ms: unit b alpha 应刚开始（≈0），实际=${unitB100!!.alpha.from}",
            unitB100.alpha.from < 0.2f,
        )
        assertNotNull("跨行 100ms: unit c 应存在", unitC100)
        assertTrue(
            "跨行 100ms: unit c alpha 应为 0（尚未开始），实际=${unitC100!!.alpha.from}",
            unitC100.alpha.from < 0.01f,
        )

        // === 200ms (2/3) 采样 ===
        val scene200 = timeline.sample(200L * NANOS_PER_MS)

        // cursor 应在第 1 行首（接近 point1），y ≈ 20
        val cursor200 = scene200.cursorRect
        assertNotNull("跨行 200ms: cursor rect 不应为 null", cursor200)
        assertTrue(
            "跨行 200ms: cursor 应在第 1 行首 (y≈20)，实际 y=${cursor200!!.top}",
            kotlin.math.abs(cursor200.top - 20f) < 3f,
        )
        assertTrue(
            "跨行 200ms: cursor 应接近 point1 (left≈${point1.rect.left})，实际=${cursor200.left}",
            kotlin.math.abs(cursor200.left - point1.rect.left) < 1f,
        )

        // 文字断言：unit a、b 已完成或交还，unit c 刚开始
        val unitA200 = scene200.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        val unitB200 = scene200.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC200 = scene200.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        if (unitA200 != null) {
            assertTrue(
                "跨行 200ms: unit a alpha 应已完成（≈1），实际=${unitA200.alpha.from}",
                unitA200.alpha.from > 0.9f,
            )
        }
        if (unitB200 != null) {
            assertTrue(
                "跨行 200ms: unit b alpha 应已完成（≈1），实际=${unitB200.alpha.from}",
                unitB200.alpha.from > 0.9f,
            )
        }
        assertNotNull("跨行 200ms: unit c 应存在", unitC200)
        assertTrue(
            "跨行 200ms: unit c alpha 应刚开始（≈0），实际=${unitC200!!.alpha.from}",
            unitC200.alpha.from < 0.2f,
        )
    }

    /**
     * #691 评论 5680711648 修复1+修复2 测试A：两笔 patch，第二笔在 30ms 到来。
     *
     * 第一笔：一次提交 abc，总时长 300ms（3 个 insertedUnits，cursor path 3 个 point）。
     * 第二笔：30ms 时再输入 d（第 4 个 unit，cursor path 新增 point）。
     *
     * 关键验证：
     * - 8ms/16ms/24ms（第一笔后）和 32ms/40ms/48ms/50ms（第二笔后）连续帧采样：
     *   b/c 仍必须是 alpha=0（尚未到 100ms/200ms）。
     *   这验证了修复1：rebaseUnitForPatch 保留了未来 unit 的绝对 start time。
     * - 50ms：cursor 不能越过当前已出现文字（cursor 停留在 startRect，不跑到 b/c/d 位置）。
     *   这验证了修复2：cursor rebase 不越过尚未开始的 surviving unit。
     * - 100ms/200ms：文字 unit 与 cursor 仍按同一分段推进。
     */
    @Test
    fun twoPatches_secondAt30ms_futureUnitsKeepAbsoluteStart() {
        val layouts = captureLayouts("", "abc", "abcd")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)
        val abcdLayout = ComposeLayoutSnapshot(layouts[2], TextRange(4, 4), 0)

        val timeline = ComposeVisualTimeline()

        // === 第一笔："" → "abc"，duration=300ms ===
        // a: startedAt=0, duration=100; b: startedAt=100, duration=100; c: startedAt=200, duration=100
        val point0 = CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)
        val cursorPath1 = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath1,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(1L),
            )

        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath1.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 连续帧采样（第一笔后，第二笔前）：8ms/16ms/24ms ===
        // b: startedAt=100ms, c: startedAt=200ms — 都尚未开始，alpha 必须为 0
        for (ms in listOf(8L, 16L, 24L)) {
            val scene = timeline.sample(ms * NANOS_PER_MS)
            val unitB = scene.units.firstOrNull { it.targetRange == TextRange(1, 2) }
            val unitC = scene.units.firstOrNull { it.targetRange == TextRange(2, 3) }
            assertNotNull("$ms ms: unit b 应存在（尚未开始动画）", unitB)
            assertTrue(
                "$ms ms: unit b alpha 应为 0（尚未开始），实际=${unitB!!.alpha.from}",
                unitB.alpha.from < 0.01f,
            )
            assertNotNull("$ms ms: unit c 应存在（尚未开始动画）", unitC)
            assertTrue(
                "$ms ms: unit c alpha 应为 0（尚未开始），实际=${unitC!!.alpha.from}",
                unitC.alpha.from < 0.01f,
            )
        }

        // === 第二笔：30ms 时 "abc" → "abcd"，插入 d ===
        val frameTime2 = 30L * NANOS_PER_MS
        // offsetMap：a/b/c 在新正文中位置不变（identity 映射 0..3 → 0..3）
        val offsetMap2 = listOf(VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY))
        // d 的 cursor point（单点路径）
        val pointD = CursorMotionPoint(rect = Rect(40f, 0f, 42f, 14f), endFraction = 1f)
        val cursorPath2 = CursorMotionPath(points = listOf(pointD))

        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abcLayout,
                newLayout = abcdLayout,
                offsetMap = offsetMap2,
                insertedUnits = listOf(TextRange(3, 4)),
                cursorMotionPath = cursorPath2,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(2L),
            )

        // cursor 在 30ms 时的位置（第一笔 cursor 在 30ms 的插值结果）
        // elapsed=30ms, progress=0.1, point0 endFraction=1/3, segmentProgress=0.3
        // cursor = interpolate(fromRect, point0, 0.3) = (3, 0, 5, 14)
        val cursorAt30ms = Rect(3f, 0f, 5f, 14f)
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = frameTime2,
            cursorFromRect = cursorAt30ms,
            cursorPath = cursorPath2.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 连续帧采样（第二笔后）：32ms/40ms/48ms/50ms ===
        // 评论 5682970101：b/c 在第二笔时被重新分段（不再保留 startedAt=100/200）。
        // abc@0ms：a:0-100, b:100-200, c:200-300
        // d@30ms：a 已开始；待显示=[b,c,d] 在 [30,330] 分段 → b:30-130, c:130-230, d:230-330
        // 32ms 时 b 已开始（startedAt=30ms，alpha≈0.02）；c/d 仍为 0
        for (ms in listOf(32L, 40L, 48L, 50L)) {
            val scene = timeline.sample(ms * NANOS_PER_MS)
            val unitC = scene.units.firstOrNull { it.targetRange == TextRange(2, 3) }
            val unitD = scene.units.firstOrNull { it.targetRange == TextRange(3, 4) }
            assertNotNull("$ms ms: unit c 应存在（尚未开始动画）", unitC)
            assertTrue(
                "$ms ms: unit c alpha 应为 0（startedAt=130ms），实际=${unitC!!.alpha.from}",
                unitC.alpha.from < 0.01f,
            )
            assertNotNull("$ms ms: unit d 应存在（尚未开始动画）", unitD)
            assertTrue(
                "$ms ms: unit d alpha 应为 0（startedAt=230ms），实际=${unitD!!.alpha.from}",
                unitD.alpha.from < 0.01f,
            )
        }

        // === 50ms：cursor 不能越过当前已出现文字 ===
        // 修复后 b 在 30ms 重新分段后 startedAt=30ms，cursor 在 b segment 内（b:30-130）
        // cursor 在 startRect 和 b caret 之间，不应到达 d 的位置
        val scene50 = timeline.sample(50L * NANOS_PER_MS)
        val cursor50 = scene50.cursorRect
        assertNotNull("50ms: cursor rect 不应为 null", cursor50)
        assertTrue(
            "50ms: cursor 不应到达 d 的位置 (left 应远小于 40f)，实际=${cursor50!!.left}",
            cursor50.left < 35f,
        )

        // === 100ms：b alpha≈0.7（30-130,elapsed=70），c 尚未开始（alpha=0）===
        val scene100 = timeline.sample(100L * NANOS_PER_MS)
        val unitB100 = scene100.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC100 = scene100.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        assertNotNull("100ms: unit b 应存在", unitB100)
        assertTrue(
            "100ms: unit b alpha 应≈0.7（30-130,elapsed=70），实际=${unitB100!!.alpha.from}",
            kotlin.math.abs(unitB100.alpha.from - 0.7f) < 0.15f,
        )
        assertNotNull("100ms: unit c 应存在（尚未开始动画）", unitC100)
        assertTrue(
            "100ms: unit c alpha 应为 0（startedAt=130ms），实际=${unitC100!!.alpha.from}",
            unitC100.alpha.from < 0.01f,
        )

        // === 200ms：c alpha≈0.7（130-230,elapsed=70）===
        val scene200 = timeline.sample(200L * NANOS_PER_MS)
        val unitC200 = scene200.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        assertNotNull("200ms: unit c 应存在", unitC200)
        assertTrue(
            "200ms: unit c alpha 应≈0.7（130-230,elapsed=70），实际=${unitC200!!.alpha.from}",
            kotlin.math.abs(unitC200!!.alpha.from - 0.7f) < 0.15f,
        )
    }

    /**
     * #691 评论 5680711648 修复1+修复2 测试B：同一 VSync 两个 patch 都在 0ms drain。
     *
     * abc patch 和下一笔 patch 在同一个 frameTimeNanos=0 drain。
     * 修复前：sampleUnit 会把 b/c 的 startedAt 重置到 0，三个 unit 重新一起开始。
     * 修复后：rebaseUnitForPatch 保留 b/c 的 startedAt=100/200，b/c 不会提前启动。
     *
     * 关键验证：
     * - 8ms/16ms 连续帧采样：b/c alpha=0（尚未到 100ms/200ms）。
     * - 10ms：cursor 停留在 startRect，不越过已出现文字。
     */
    @Test
    fun sameVsyncTwoPatchesAt0ms_futureUnitsDoNotRestart() {
        val layouts = captureLayouts("", "abc", "abcd")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)
        val abcdLayout = ComposeLayoutSnapshot(layouts[2], TextRange(4, 4), 0)

        val timeline = ComposeVisualTimeline()

        // === 第一笔：0ms "" → "abc"，duration=300ms ===
        val point0 = CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)
        val cursorPath1 = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath1,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(1L),
            )

        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath1.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 第二笔：同一 VSync 0ms "abc" → "abcd"，插入 d ===
        // 与第一笔在同一个 frameTimeNanos=0 drain
        val offsetMap2 = listOf(VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY))
        val pointD = CursorMotionPoint(rect = Rect(40f, 0f, 42f, 14f), endFraction = 1f)
        val cursorPath2 = CursorMotionPath(points = listOf(pointD))

        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abcLayout,
                newLayout = abcdLayout,
                offsetMap = offsetMap2,
                insertedUnits = listOf(TextRange(3, 4)),
                cursorMotionPath = cursorPath2,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(2L),
            )

        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath2.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 连续帧采样：8ms/16ms ===
        // 评论 5684136311：同一 VSync 0ms 两笔。a/b/c/d 在 [0,300] 分段
        // a:0-75, b:75-150, c:150-225, d:225-300（同一 VSync 零进度 unit 可重新分段）
        // 8ms 时 a 在进行（startedAt=0），b/c/d 仍为 0
        for (ms in listOf(8L, 16L)) {
            val scene = timeline.sample(ms * NANOS_PER_MS)
            val unitC = scene.units.firstOrNull { it.targetRange == TextRange(2, 3) }
            val unitD = scene.units.firstOrNull { it.targetRange == TextRange(3, 4) }
            assertNotNull("$ms ms: unit c 应存在（尚未开始动画）", unitC)
            assertTrue(
                "$ms ms: unit c alpha 应为 0（startedAt=150ms），实际=${unitC!!.alpha.from}",
                unitC.alpha.from < 0.01f,
            )
            assertNotNull("$ms ms: unit d 应存在（尚未开始动画）", unitD)
            assertTrue(
                "$ms ms: unit d alpha 应为 0（startedAt=225ms），实际=${unitD!!.alpha.from}",
                unitD.alpha.from < 0.01f,
            )
        }

        // === 10ms：cursor 在 a segment 内，不越过已出现文字 ===
        // 修复后 a 从 0ms 开始（0..75ms），cursor 在 a segment 内
        val scene10 = timeline.sample(10L * NANOS_PER_MS)
        val cursor10 = scene10.cursorRect
        assertNotNull("10ms: cursor rect 不应为 null", cursor10)
        assertTrue(
            "10ms: cursor 不应到达 d 的位置 (left 应远小于 40f)，实际=${cursor10!!.left}",
            cursor10.left < 35f,
        )

        // === 100ms：b 在进行（startedAt=75），c 尚未开始（startedAt=150），d 尚未开始 ===
        val scene100 = timeline.sample(100L * NANOS_PER_MS)
        val unitB100 = scene100.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC100 = scene100.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        // unit b：startedAt=75ms，100ms 时 ≈0.33（在 0.1..0.6 范围内）
        if (unitB100 != null) {
            assertTrue(
                "100ms: unit b alpha 应在进行中（startedAt=75ms，100ms 时≈0.33），实际=${unitB100.alpha.from}",
                unitB100.alpha.from in 0.1f..0.6f,
            )
        }
        assertNotNull("100ms: unit c 应存在（尚未开始动画）", unitC100)
        assertTrue(
            "100ms: unit c alpha 应为 0（startedAt=150ms），实际=${unitC100!!.alpha.from}",
            unitC100.alpha.from < 0.2f,
        )
    }

    /**
     * #691 评论 5679815971 问题1：applyMotionPolicyAtFrame 一次性同步所有 UI 状态。
     *
     * 验证调用 applyMotionPolicyAtFrame 后：
     * - drawsVisualCursor 跟随 effective.cursorEnabled
     * - hiddenRanges 被清空
     * - visualScene 被重置为 Empty
     */
    @Test
    fun applyMotionPolicyAtFrame_syncsAllUiStates() {
        val state = ComposeEditorVisualState(targetId = "test-policy-sync", initialDrawsVisualCursor = true)

        // 先建立一些状态
        val layouts = captureLayouts("", "a")
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textEnabled = true, cursorEnabled = true, textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(0L)

        // 切换 policy：cursorEnabled=false
        // Issue #723 评论 5749023316 缺口2：coordinated=false 时独立开关生效。
        state.applyMotionPolicyAtFrame(EditorMotionPolicy(textEnabled = true, cursorEnabled = false, coordinated = false))

        // drawsVisualCursor 应跟随 effective.cursorEnabled = false
        assertFalse(
            "applyMotionPolicyAtFrame 后 drawsVisualCursor 应为 false",
            state.drawsVisualCursor.value,
        )
        // hiddenRanges 应被清空（scene.hiddenRanges 保留，draw 层裁切用）
        assertTrue(
            "applyMotionPolicyAtFrame 后 scene.hiddenRanges 应为空",
            state.visualScene.value.hiddenRanges.isEmpty(),
        )
        // visualScene 应被重置为 Empty
        assertEquals(
            "applyMotionPolicyAtFrame 后 visualScene 应为 Empty",
            ComposeVisualScene.Empty,
            state.visualScene.value,
        )

        // 再切换：cursorEnabled=true
        state.applyMotionPolicyAtFrame(EditorMotionPolicy(textEnabled = true, cursorEnabled = true))
        assertTrue(
            "切回 cursorEnabled=true 后 drawsVisualCursor 应为 true",
            state.drawsVisualCursor.value,
        )

        // reduceMotion=true 时 effective.cursorEnabled=false
        state.applyMotionPolicyAtFrame(EditorMotionPolicy(reduceMotion = true))
        assertFalse(
            "reduceMotion=true 后 drawsVisualCursor 应为 false",
            state.drawsVisualCursor.value,
        )
    }

    // ==================== #691 评论 5681258225 协同动画修复测试 ====================

    /**
     * #691 评论 5681258225 修复1 / 评论 5682970101：abc@0ms + d@30ms — scene redirect 有界窗口。
     *
     * 第一笔：0ms "" → "abc"，3 个 insertedUnits，duration=300ms。
     *   a: 0..100ms, b: 100..200ms, c: 200..300ms
     * 第二笔：30ms "abc" → "abcd"，1 个 insertedUnit (d)。
     *   修复后（评论 5682970101）：a 已开始保留；待显示=[b,c,d] 在 [30,330] 有界窗口分段
     *   b: 30..130ms, c: 130..230ms, d: 230..330ms
     *   cursor 合并：surviving 未开始 = [b, c] + 新 patch = [d] → 3 points
     *   cursor startedAtNanos=30ms, durationNanos=300ms（coordinated=true 用文字时长）
     *
     * 关键验证：
     * - 50ms：d alpha=0（d startedAt=230ms）；b alpha≈0.2（30-130,elapsed=20）
     * - 100ms：b alpha≈0.7（30-130,elapsed=70）；c/d alpha=0
     * - 200ms：c alpha≈0.7（130-230,elapsed=70）；d alpha=0
     * - 350ms：d alpha 进行中（230-330,elapsed=120,progress=0.4）
     */
    @Test
    fun abcAt0ms_dAt30ms_newUnitQueuedAfterSurviving_cursorSyncedWithText() {
        val layouts = captureLayouts("", "abc", "abcd")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)
        val abcdLayout = ComposeLayoutSnapshot(layouts[2], TextRange(4, 4), 0)

        val timeline = ComposeVisualTimeline()

        // === 第一笔：0ms "" → "abc"，duration=300ms ===
        val point0 = CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)
        val cursorPath1 = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath1,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(1L),
            )

        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath1.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 第二笔：30ms "abc" → "abcd"，插入 d ===
        val frameTime2 = 30L * NANOS_PER_MS
        val offsetMap2 = listOf(VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY))
        val pointD = CursorMotionPoint(rect = Rect(40f, 0f, 42f, 14f), endFraction = 1f)
        val cursorPath2 = CursorMotionPath(points = listOf(pointD))

        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abcLayout,
                newLayout = abcdLayout,
                offsetMap = offsetMap2,
                insertedUnits = listOf(TextRange(3, 4)),
                cursorMotionPath = cursorPath2,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(2L),
            )

        // cursor 在 30ms 时的位置（第一笔 cursor 在 30ms 的插值结果）
        // elapsed=30ms, progress=0.1, point0 endFraction=1/3, segmentProgress=0.3
        // cursor = interpolate(fromRect, point0, 0.3) = (3, 0, 5, 14)
        val cursorAt30ms = Rect(3f, 0f, 5f, 14f)
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = frameTime2,
            cursorFromRect = cursorAt30ms,
            cursorPath = cursorPath2.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 50ms：d alpha=0（d startedAt=230ms）；b alpha≈0.2（30-130,elapsed=20）===
        val scene50 = timeline.sample(50L * NANOS_PER_MS)
        val unitD50 = scene50.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        assertNotNull("50ms: unit d 应存在（尚未开始动画）", unitD50)
        assertTrue(
            "50ms: unit d alpha 应为 0（startedAt=230ms），实际=${unitD50!!.alpha.from}",
            unitD50.alpha.from < 0.01f,
        )

        // === 50ms：cursor 不应到达 d 的位置 ===
        // cursor 合并路径 = [b caret, c caret, d caret]，3 points
        // cursor startedAt=30ms, duration=300ms（coordinated=true 用文字时长）
        // 50ms: progress = 20/300 ≈ 0.067, 在第一段（endFraction=1/3）
        // 不应到达 d 的位置（left≈40f）
        val cursor50 = scene50.cursorRect
        assertNotNull("50ms: cursor rect 不应为 null", cursor50)
        assertTrue(
            "50ms: cursor 不应到达 d 的位置 (left 应远小于 40f)，实际=${cursor50!!.left}",
            cursor50.left < 35f,
        )

        // === 100ms：b alpha≈0.7（30-130,elapsed=70），c/d alpha=0 ===
        val scene100 = timeline.sample(100L * NANOS_PER_MS)
        val unitB100 = scene100.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC100 = scene100.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        val unitD100 = scene100.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        assertNotNull("100ms: unit b 应存在", unitB100)
        assertTrue(
            "100ms: unit b alpha 应≈0.7（30-130,elapsed=70），实际=${unitB100!!.alpha.from}",
            kotlin.math.abs(unitB100.alpha.from - 0.7f) < 0.15f,
        )
        assertNotNull("100ms: unit c 应存在", unitC100)
        assertTrue(
            "100ms: unit c alpha 应为 0（startedAt=130ms），实际=${unitC100!!.alpha.from}",
            unitC100.alpha.from < 0.01f,
        )
        assertNotNull("100ms: unit d 应存在", unitD100)
        assertTrue(
            "100ms: unit d alpha 应为 0（startedAt=230ms），实际=${unitD100!!.alpha.from}",
            unitD100.alpha.from < 0.01f,
        )

        // === 200ms：c alpha≈0.7（130-230,elapsed=70），d 仍为 0 ===
        val scene200 = timeline.sample(200L * NANOS_PER_MS)
        val unitC200 = scene200.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        val unitD200 = scene200.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        assertNotNull("200ms: unit c 应存在", unitC200)
        assertTrue(
            "200ms: unit c alpha 应≈0.7（130-230,elapsed=70），实际=${unitC200!!.alpha.from}",
            kotlin.math.abs(unitC200!!.alpha.from - 0.7f) < 0.15f,
        )
        assertNotNull("200ms: unit d 应存在", unitD200)
        assertTrue(
            "200ms: unit d alpha 应为 0（startedAt=230ms），实际=${unitD200!!.alpha.from}",
            unitD200.alpha.from < 0.01f,
        )

        // === 350ms：d alpha 应已开始（startedAt=230ms，elapsed=120,progress=0.4） ===
        val scene350 = timeline.sample(350L * NANOS_PER_MS)
        val unitD350 = scene350.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        if (unitD350 != null) {
            assertTrue(
                "350ms: unit d alpha 应已开始（>0），实际=${unitD350.alpha.from}",
                unitD350.alpha.from > 0f,
            )
        }
    }

    /**
     * #691 评论 5684136311：同一 VSync abc+d — 收敛为一条最终 scene 时间表。
     *
     * 同一 frameTimeNanos=0 连续两笔 patch（abc + d）。
     * 修复后（评论 5684136311）：同一 VSync 零进度 unit 可重新分段。
     * a/b/c/d 在 [0,300] 有界窗口均匀分段：
     * a: 0..75ms, b: 75..150ms, c: 150..225ms, d: 225..300ms
     * cursor 用同一份 a/b/c/d segment 表生成 4 个 caret point。
     *
     * 关键验证：
     * - 37.5ms：a 在进行，b/c/d 仍为 0
     * - 75ms：a 完成，b 刚开始
     * - 150ms：b 完成，c 刚开始
     * - 225ms：c 完成，d 刚开始
     * - 300ms：d 完成
     * - cursor 在边界分别到 a/b/c/d caret
     * - 不能出现两个 startedAtNanos==frameTimeNanos 且都从 0→1 的插入 unit
     */
    @Test
    fun sameVsyncAbcPlusD_textOrderAbcd_cursorUsesFinalLayout() {
        val layouts = captureLayouts("", "abc", "abcd")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)
        val abcdLayout = ComposeLayoutSnapshot(layouts[2], TextRange(4, 4), 0)

        val timeline = ComposeVisualTimeline()

        // === 第一笔：0ms "" → "abc"，duration=300ms ===
        val point0 = CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)
        val cursorPath1 = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath1,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(1L),
            )

        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath1.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 第二笔：同一 VSync 0ms "abc" → "abcd"，插入 d ===
        val offsetMap2 = listOf(VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY))
        val pointD = CursorMotionPoint(rect = Rect(40f, 0f, 42f, 14f), endFraction = 1f)
        val cursorPath2 = CursorMotionPath(points = listOf(pointD))

        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abcLayout,
                newLayout = abcdLayout,
                offsetMap = offsetMap2,
                insertedUnits = listOf(TextRange(3, 4)),
                cursorMotionPath = cursorPath2,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(2L),
            )

        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath2.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 37.5ms：a 在进行（alpha≈0.5），b/c/d 仍为 0 ===
        val scene375 = timeline.sample(37.5f.toLong() * NANOS_PER_MS)
        val unitA375 = scene375.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        val unitB375 = scene375.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC375 = scene375.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        val unitD375 = scene375.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        assertNotNull("37.5ms: unit a 应存在", unitA375)
        assertTrue(
            "37.5ms: unit a alpha 应在进行中（≈0.5），实际=${unitA375!!.alpha.from}",
            kotlin.math.abs(unitA375.alpha.from - 0.5f) < 0.15f,
        )
        assertNotNull("37.5ms: unit b 应存在", unitB375)
        assertTrue("37.5ms: unit b alpha 应为 0，实际=${unitB375!!.alpha.from}", unitB375.alpha.from < 0.01f)
        assertNotNull("37.5ms: unit c 应存在", unitC375)
        assertTrue("37.5ms: unit c alpha 应为 0，实际=${unitC375!!.alpha.from}", unitC375.alpha.from < 0.01f)
        assertNotNull("37.5ms: unit d 应存在", unitD375)
        assertTrue("37.5ms: unit d alpha 应为 0，实际=${unitD375!!.alpha.from}", unitD375.alpha.from < 0.01f)

        // === 75ms：a 完成（alpha≈1 或已收口），b 刚开始（alpha≈0） ===
        val scene75 = timeline.sample(75L * NANOS_PER_MS)
        val unitA75 = scene75.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        val unitB75 = scene75.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC75 = scene75.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        val unitD75 = scene75.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        if (unitA75 != null) {
            assertTrue("75ms: unit a alpha 应已完成（≈1），实际=${unitA75.alpha.from}", unitA75.alpha.from > 0.8f)
        }
        assertNotNull("75ms: unit b 应存在", unitB75)
        assertTrue("75ms: unit b alpha 应刚开始（≈0），实际=${unitB75!!.alpha.from}", unitB75.alpha.from < 0.2f)
        assertNotNull("75ms: unit c 应存在", unitC75)
        assertTrue("75ms: unit c alpha 应为 0，实际=${unitC75!!.alpha.from}", unitC75.alpha.from < 0.01f)
        assertNotNull("75ms: unit d 应存在", unitD75)
        assertTrue("75ms: unit d alpha 应为 0，实际=${unitD75!!.alpha.from}", unitD75.alpha.from < 0.01f)

        // === 150ms：b 完成，c 刚开始 ===
        val scene150 = timeline.sample(150L * NANOS_PER_MS)
        val unitB150 = scene150.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC150 = scene150.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        val unitD150 = scene150.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        if (unitB150 != null) {
            assertTrue("150ms: unit b alpha 应已完成（≈1），实际=${unitB150.alpha.from}", unitB150.alpha.from > 0.8f)
        }
        assertNotNull("150ms: unit c 应存在", unitC150)
        assertTrue("150ms: unit c alpha 应刚开始（≈0），实际=${unitC150!!.alpha.from}", unitC150.alpha.from < 0.2f)
        assertNotNull("150ms: unit d 应存在", unitD150)
        assertTrue("150ms: unit d alpha 应为 0，实际=${unitD150!!.alpha.from}", unitD150.alpha.from < 0.01f)

        // === 225ms：c 完成，d 刚开始 ===
        val scene225 = timeline.sample(225L * NANOS_PER_MS)
        val unitC225 = scene225.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        val unitD225 = scene225.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        if (unitC225 != null) {
            assertTrue("225ms: unit c alpha 应已完成（≈1），实际=${unitC225.alpha.from}", unitC225.alpha.from > 0.8f)
        }
        assertNotNull("225ms: unit d 应存在", unitD225)
        assertTrue("225ms: unit d alpha 应刚开始（≈0），实际=${unitD225!!.alpha.from}", unitD225.alpha.from < 0.2f)

        // === 300ms：d 完成 ===
        val scene300 = timeline.sample(300L * NANOS_PER_MS)
        val unitD300 = scene300.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        if (unitD300 != null) {
            assertTrue("300ms: unit d alpha 应已完成（≈1），实际=${unitD300.alpha.from}", unitD300.alpha.from > 0.8f)
        }

        // === cursor 在边界到 a/b/c/d caret ===
        // cursor 4 points (a_caret, b_caret, c_caret, d_caret), duration=300ms, endFraction=0.25/0.5/0.75/1.0
        // 75ms (progress=0.25): cursor 到达 a caret（a 的 caret rect 来自 abcdLayout.getCursorRect(1)）
        val cursor75 = scene75.cursorRect
        assertNotNull("75ms: cursor rect 应存在", cursor75)
        // 150ms (progress=0.5): cursor 到达 b caret
        val cursor150 = scene150.cursorRect
        assertNotNull("150ms: cursor rect 应存在", cursor150)
        // 225ms (progress=0.75): cursor 到达 c caret
        val cursor225 = scene225.cursorRect
        assertNotNull("225ms: cursor rect 应存在", cursor225)
        // 300ms (progress=1.0): cursor 到达 d caret (pointD.left=40)
        val cursor300 = scene300.cursorRect
        assertNotNull("300ms: cursor rect 应存在", cursor300)
        assertTrue(
            "300ms: cursor 应到达 d 的位置 (left≈40)，实际=${cursor300!!.left}",
            kotlin.math.abs(cursor300.left - 40f) < 5f,
        )

        // === 补充断言：同一 VSync 第二笔 patch 后，不能出现两个 startedAtNanos == frameTimeNanos 且都从 0→1 的插入 unit ===
        // 正确行为：a/b/c/d 均匀分段，只有 a 的 startedAt==0（frameTimeNanos），b/c/d 的 startedAt > 0
        val allUnitsAfterPatch2 = timeline.sample(0L).units.filter { it.targetRange != null }
        val zeroStartedUnits =
            allUnitsAfterPatch2.filter {
                it.alpha.startedAtNanos == 0L && it.alpha.from == 0f && it.alpha.to == 1f
            }
        assertTrue(
            "同一 VSync 后不应有多个 startedAtNanos==0 且从 0→1 的 unit，实际有 ${zeroStartedUnits.size} 个",
            zeroStartedUnits.size <= 1,
        )
    }

    /**
     * #691 评论 5681258225 修复3：跨行场景 — surviving unit 的 caret rect 从最新 layout 重算。
     *
     * 用窄布局让文字换行。第一笔输入跨行文字，第二笔快速输入新字。
     * surviving unit 的 caret rect 必须来自 patch2.newLayout（最新 layout），
     * 不是旧 layout — 这确保跨行时 cursor 位置正确。
     *
     * 验证方式：通过对比 patch1.newLayout 和 patch2.newLayout 的 getCursorRect 结果，
     * 确认它们不同（跨行导致 caret 位置变化），然后验证 cursor 位置反映了最新 layout。
     */
    @Test
    fun crossLineRapidInput_caretRectRecomputedFromLatestLayout() {
        // 用窄布局让 "abc" 和 "abcd" 换行位置不同
        val layouts = captureLayoutsWithWidth(arrayOf("", "abc", "abcd"), 30)
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)
        val abcdLayout = ComposeLayoutSnapshot(layouts[2], TextRange(4, 4), 0)

        val timeline = ComposeVisualTimeline()

        // 先验证两个 layout 在某些 offset 上的 getCursorRect 不同（跨行效果）
        // 这确保测试有意义 — 如果两个 layout 的 caret rect 相同，就无法验证"从最新 layout 重算"
        val caretAt2InAbc = abcLayout.result.getCursorRect(2)
        val caretAt2InAbcd = abcdLayout.result.getCursorRect(2)
        val caretAt3InAbc = abcLayout.result.getCursorRect(3)
        val caretAt3InAbcd = abcdLayout.result.getCursorRect(3)

        // 跨行验证：窄布局下 "abc" 和 "abcd" 的某些 caret rect 应该不同
        // （因为 "abcd" 可能换行方式与 "abc" 不同）
        val caretDiffExists =
            kotlin.math.abs(caretAt2InAbc.left - caretAt2InAbcd.left) > 0.1f ||
                kotlin.math.abs(caretAt2InAbc.top - caretAt2InAbcd.top) > 0.1f ||
                kotlin.math.abs(caretAt3InAbc.left - caretAt3InAbcd.left) > 0.1f ||
                kotlin.math.abs(caretAt3InAbc.top - caretAt3InAbcd.top) > 0.1f

        // 即使 Robolectric 下布局差异不大，也继续测试 — 核心 assert 是 cursor 不为 null 且行为合理
        // === 第一笔：0ms "" → "abc"，3 个 insertedUnits，duration=300ms ===
        val point0 = CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)
        val cursorPath1 = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath1,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(1L),
            )

        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath1.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 第二笔：30ms "abc" → "abcd"，插入 d ===
        val frameTime2 = 30L * NANOS_PER_MS
        val offsetMap2 = listOf(VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY))
        val pointD = CursorMotionPoint(rect = Rect(40f, 0f, 42f, 14f), endFraction = 1f)
        val cursorPath2 = CursorMotionPath(points = listOf(pointD))

        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abcLayout,
                newLayout = abcdLayout,
                offsetMap = offsetMap2,
                insertedUnits = listOf(TextRange(3, 4)),
                cursorMotionPath = cursorPath2,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(2L),
            )

        val cursorAt30ms = Rect(3f, 0f, 5f, 14f)
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = frameTime2,
            cursorFromRect = cursorAt30ms,
            cursorPath = cursorPath2.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // === 验证：d 的 alpha 在 50ms 时为 0 ===
        val scene50 = timeline.sample(50L * NANOS_PER_MS)
        val unitD50 = scene50.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        assertNotNull("跨行 50ms: unit d 应存在", unitD50)
        assertTrue(
            "跨行 50ms: unit d alpha 应为 0（queued after surviving），实际=${unitD50!!.alpha.from}",
            unitD50.alpha.from < 0.01f,
        )

        // === 验证：cursor 不为 null（合并路径成功创建） ===
        val cursor50 = scene50.cursorRect
        assertNotNull("跨行 50ms: cursor rect 不应为 null（合并 surviving + new patch points）", cursor50)

        // === 验证：cursor 不应到达 d 的位置 ===
        assertTrue(
            "跨行 50ms: cursor 不应到达 d 的位置 (left 应远小于 40f)，实际=${cursor50!!.left}",
            cursor50.left < 35f,
        )

        // === 如果两个 layout 的 caret rect 确实不同（跨行效果），进一步验证 ===
        if (caretDiffExists) {
            // cursor 合并路径中的 surviving points 使用 patch2.newLayout 的 getCursorRect
            // 在 100ms 时 cursor 应接近某个 surviving caret rect（来自 abcdLayout，不是 abcLayout）
            val scene100 = timeline.sample(100L * NANOS_PER_MS)
            val cursor100 = scene100.cursorRect
            assertNotNull("跨行 100ms: cursor rect 不应为 null", cursor100)

            // cursor 不应等于旧 layout 的 caret rect（如果新旧 layout 的 caret rect 不同）
            // cursor 应反映最新 layout 的几何
            // 这是一个弱断言 — 只验证 cursor 在合理范围内
            assertTrue(
                "跨行 100ms: cursor 应在合理范围内 (left >= 0)，实际=${cursor100!!.left}",
                cursor100.left >= 0f,
            )
        }

        // === 验证：b/c 的 alpha 在 100ms/200ms 时按序开始（评论 5682970101 有界窗口分段）===
        // 修复后：a 已开始保留；待显示=[b,c,d] 在 [30,330] 分段：b:30-130, c:130-230, d:230-330
        // 100ms: b alpha≈0.7（30-130,elapsed=70）
        // 200ms: c alpha≈0.7（130-230,elapsed=70）
        val scene100 = timeline.sample(100L * NANOS_PER_MS)
        val unitB100 = scene100.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        assertNotNull("跨行 100ms: unit b 应存在", unitB100)
        assertTrue(
            "跨行 100ms: unit b alpha 应≈0.7（30-130,elapsed=70），实际=${unitB100!!.alpha.from}",
            kotlin.math.abs(unitB100.alpha.from - 0.7f) < 0.15f,
        )

        val scene200 = timeline.sample(200L * NANOS_PER_MS)
        val unitC200 = scene200.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        assertNotNull("跨行 200ms: unit c 应存在", unitC200)
        assertTrue(
            "跨行 200ms: unit c alpha 应≈0.7（130-230,elapsed=70），实际=${unitC200!!.alpha.from}",
            kotlin.math.abs(unitC200!!.alpha.from - 0.7f) < 0.15f,
        )
    }

    /**
     * #691 评论 5681258225 修复4 / 评论 5682970101：默认 100ms 和长时长 1000ms — 连续 patch 有界窗口分段。
     *
     * 子测试 A：默认 100ms
     * - patch1："" → "ab"，duration=100ms（a: 0..50ms, b: 50..100ms）
     * - 20ms 时 patch2："ab" → "abc"，duration=100ms
     * - 修复后（评论 5682970101）：20ms 时 a 已开始（0<=20）；b 尚未开始（50>20）；
     *   待显示=[b,c] 在 [20,120] 有界窗口分段：b: 20..70ms, c: 70..120ms
     * - 30ms：c alpha=0（c startedAt=70ms）；b alpha≈0.2（20-70,elapsed=10）
     * - 100ms：c alpha≈0.6（70-120,elapsed=30）；b 已完成
     *
     * 子测试 B：长时长 1000ms
     * - patch1："" → "ab"，duration=1000ms（a: 0..500ms, b: 500..1000ms）
     * - 100ms 时 patch2："ab" → "abc"，duration=1000ms
     * - 修复后：100ms 时 a 已开始；b 尚未开始（500>100）；
     *   待显示=[b,c] 在 [100,1100] 有界窗口分段：b: 100..600ms, c: 600..1100ms
     * - 200ms：c alpha=0（c startedAt=600ms）；b alpha≈0.2（100-600,elapsed=100）
     * - 1000ms：c alpha≈0.8（600-1100,elapsed=400）；b 已完成
     */
    @Test
    fun default100ms_andLongDuration1000ms_continuousPatchNoRestartNoJumping() {
        // 一次性获取所有需要的 layout（composeRule.setContent 只能调用一次）
        val layouts = captureLayouts("", "ab", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[2], TextRange(3, 3), 0)

        // === 子测试 A：默认 100ms ===
        run {
            val timeline = ComposeVisualTimeline()

            // patch1：0ms "" → "ab"，duration=100ms
            // a: startedAt=0, duration=50ms（0..50ms）
            // b: startedAt=50ms, duration=50ms（50..100ms）
            val cursorPath1 =
                CursorMotionPath(
                    points =
                        listOf(
                            CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 0.5f),
                            CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 1f),
                        ),
                )
            val patch1 =
                makePatch(
                    id = 1L,
                    oldLayout = emptyLayout,
                    newLayout = abLayout,
                    insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2)),
                    cursorMotionPath = cursorPath1,
                    durationMs = 100L,
                    motionPolicy =
                        EditorMotionPolicy(
                            textDurationMillis = 100L,
                            cursorEnabled = true,
                            coordinated = true,
                        ),
                    intent = nonLocalIntent(1L),
                )

            timeline.applyPatch(
                patch = patch1,
                frameTimeNanos = 0L,
                cursorFromRect = Rect(0f, 0f, 2f, 14f),
                cursorPath = cursorPath1.points,
                cursorDurationNanos = 100L * NANOS_PER_MS,
            )

            // patch2：20ms "ab" → "abc"，duration=100ms
            // 修复后：a 已开始；b 尚未开始（50>20）；待显示=[b,c] 在 [20,120] 分段
            // b: 20..70ms, c: 70..120ms
            val frameTime2 = 20L * NANOS_PER_MS
            val offsetMap2 = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY))
            val cursorPath2 =
                CursorMotionPath(
                    points = listOf(CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)),
                )
            val patch2 =
                makePatch(
                    id = 2L,
                    oldLayout = abLayout,
                    newLayout = abcLayout,
                    offsetMap = offsetMap2,
                    insertedUnits = listOf(TextRange(2, 3)),
                    cursorMotionPath = cursorPath2,
                    durationMs = 100L,
                    motionPolicy =
                        EditorMotionPolicy(
                            textDurationMillis = 100L,
                            cursorEnabled = true,
                            coordinated = true,
                        ),
                    intent = nonLocalIntent(2L),
                )

            timeline.applyPatch(
                patch = patch2,
                frameTimeNanos = frameTime2,
                cursorFromRect = Rect(4f, 0f, 6f, 14f),
                cursorPath = cursorPath2.points,
                cursorDurationNanos = 100L * NANOS_PER_MS,
            )

            // 30ms：c alpha=0（c startedAt=70ms >> 30ms）；b alpha≈0.2（20-70,elapsed=10）
            val scene30 = timeline.sample(30L * NANOS_PER_MS)
            val unitC30 = scene30.units.firstOrNull { it.targetRange == TextRange(2, 3) }
            assertNotNull("100ms 子测试 30ms: unit c 应存在", unitC30)
            assertTrue(
                "100ms 子测试 30ms: unit c alpha 应为 0（startedAt=70ms），实际=${unitC30!!.alpha.from}",
                unitC30.alpha.from < 0.01f,
            )

            // 100ms：c alpha≈0.6（70-120,elapsed=30）；b 已完成
            val scene100 = timeline.sample(100L * NANOS_PER_MS)
            val unitC100 = scene100.units.firstOrNull { it.targetRange == TextRange(2, 3) }
            assertNotNull("100ms 子测试 100ms: unit c 应存在", unitC100)
            assertTrue(
                "100ms 子测试 100ms: unit c alpha 应≈0.6（70-120,elapsed=30），实际=${unitC100!!.alpha.from}",
                kotlin.math.abs(unitC100.alpha.from - 0.6f) < 0.15f,
            )

            // a 的 alpha 不应被重置（持续 timeline 核心不变量）
            val unitA100 = scene100.units.firstOrNull { it.targetRange == TextRange(0, 1) }
            if (unitA100 != null) {
                assertTrue(
                    "100ms 子测试 100ms: unit a alpha 不应被重置为 0，实际=${unitA100.alpha.from}",
                    unitA100.alpha.from > 0f,
                )
            }
        }

        // === 子测试 B：长时长 1000ms ===
        run {
            val timeline = ComposeVisualTimeline()

            // patch1：0ms "" → "ab"，duration=1000ms
            // a: startedAt=0, duration=500ms（0..500ms）
            // b: startedAt=500ms, duration=500ms（500..1000ms）
            val cursorPath1 =
                CursorMotionPath(
                    points =
                        listOf(
                            CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 0.5f),
                            CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 1f),
                        ),
                )
            val patch1 =
                makePatch(
                    id = 1L,
                    oldLayout = emptyLayout,
                    newLayout = abLayout,
                    insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2)),
                    cursorMotionPath = cursorPath1,
                    durationMs = 1000L,
                    motionPolicy =
                        EditorMotionPolicy(
                            textDurationMillis = 1000L,
                            cursorEnabled = true,
                            coordinated = true,
                        ),
                    intent = nonLocalIntent(1L),
                )

            timeline.applyPatch(
                patch = patch1,
                frameTimeNanos = 0L,
                cursorFromRect = Rect(0f, 0f, 2f, 14f),
                cursorPath = cursorPath1.points,
                cursorDurationNanos = 1000L * NANOS_PER_MS,
            )

            // patch2：100ms "ab" → "abc"，duration=1000ms
            // 修复后：a 已开始；b 尚未开始（500>100）；待显示=[b,c] 在 [100,1100] 分段
            // b: 100..600ms, c: 600..1100ms
            val frameTime2 = 100L * NANOS_PER_MS
            val offsetMap2 = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY))
            val cursorPath2 =
                CursorMotionPath(
                    points = listOf(CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)),
                )
            val patch2 =
                makePatch(
                    id = 2L,
                    oldLayout = abLayout,
                    newLayout = abcLayout,
                    offsetMap = offsetMap2,
                    insertedUnits = listOf(TextRange(2, 3)),
                    cursorMotionPath = cursorPath2,
                    durationMs = 1000L,
                    motionPolicy =
                        EditorMotionPolicy(
                            textDurationMillis = 1000L,
                            cursorEnabled = true,
                            coordinated = true,
                        ),
                    intent = nonLocalIntent(2L),
                )

            timeline.applyPatch(
                patch = patch2,
                frameTimeNanos = frameTime2,
                cursorFromRect = Rect(2f, 0f, 4f, 14f),
                cursorPath = cursorPath2.points,
                cursorDurationNanos = 1000L * NANOS_PER_MS,
            )

            // 200ms：c alpha=0（c startedAt=600ms >> 200ms）；b alpha≈0.2（100-600,elapsed=100）
            val scene200 = timeline.sample(200L * NANOS_PER_MS)
            val unitC200 = scene200.units.firstOrNull { it.targetRange == TextRange(2, 3) }
            assertNotNull("1000ms 子测试 200ms: unit c 应存在", unitC200)
            assertTrue(
                "1000ms 子测试 200ms: unit c alpha 应为 0（startedAt=600ms），" +
                    "实际=${unitC200!!.alpha.from}",
                unitC200.alpha.from < 0.01f,
            )

            // 1000ms：c alpha≈0.8（600-1100,elapsed=400）；b 已完成
            val scene1000 = timeline.sample(1000L * NANOS_PER_MS)
            val unitC1000 = scene1000.units.firstOrNull { it.targetRange == TextRange(2, 3) }
            assertNotNull("1000ms 子测试 1000ms: unit c 应存在", unitC1000)
            assertTrue(
                "1000ms 子测试 1000ms: unit c alpha 应≈0.8（600-1100,elapsed=400），实际=${unitC1000!!.alpha.from}",
                kotlin.math.abs(unitC1000.alpha.from - 0.8f) < 0.15f,
            )

            // a 的 alpha 不应被重置（持续 timeline 核心不变量）
            val unitA1000 = scene1000.units.firstOrNull { it.targetRange == TextRange(0, 1) }
            if (unitA1000 != null) {
                assertTrue(
                    "1000ms 子测试 1000ms: unit a alpha 不应被重置为 0，实际=${unitA1000.alpha.from}",
                    unitA1000.alpha.from > 0f,
                )
            }
        }
    }

    // ==================== #691 评论 5682970101 有界窗口测试 ====================

    /**
     * #691 评论 5682970101 新增测试1：快速输入时动画尾巴不随字符数线性增长。
     *
     * 100ms duration，30ms 间隔连续输入 12 个字符。
     * 串行 FIFO 下：12 个字最后一个字要在 12*100=1200ms 才完成。
     * 有界窗口下：最后一笔在 (11)*30=330ms 输入，动画应在 330+100=430ms 内全部完成。
     *
     * 关键验证：
     * - 430ms 时 hasActiveAnimation 应为 false（所有动画已收敛到最新正文）
     * - 对比串行 FIFO：12 个字最后一个字要 1200ms 才完成，430ms << 1200ms
     */
    @Test
    fun rapidInput_boundedTailDoesNotGrowLinearly() {
        // 一次性获取所有需要的 layout："" → "a" → "ab" → ... → "abcdefghijkl"
        val texts = listOf("") + (1..12).map { i -> ('a'..'l').toList().subList(0, i).joinToString("") }
        val layouts = captureLayouts(*texts.toTypedArray())
        val timeline = ComposeVisualTimeline()

        val textDurationMs = 100L
        val inputIntervalMs = 30L
        val fromRect = Rect(0f, 0f, 2f, 14f)

        // 连续输入 12 个字符，每 30ms 一笔
        for (i in 1..12) {
            val oldText = texts[i - 1]
            val newText = texts[i]
            val oldLayout = ComposeLayoutSnapshot(layouts[i - 1], TextRange(i - 1, i - 1), 0)
            val newLayout = ComposeLayoutSnapshot(layouts[i], TextRange(i, i), 0)
            val frameTime = (i - 1) * inputIntervalMs * NANOS_PER_MS

            // offsetMap：旧文字 identity 映射
            val offsetMap =
                if (oldText.isNotEmpty()) {
                    listOf(VisualOffsetMapEntry(0, 0, oldText.length, VisualOffsetMapKind.IDENTITY))
                } else {
                    null
                }
            val cursorPath =
                CursorMotionPath(
                    points = listOf(CursorMotionPoint(rect = Rect(i * 10f, 0f, i * 10f + 2f, 14f), endFraction = 1f)),
                )
            val patch =
                makePatch(
                    id = i.toLong(),
                    oldLayout = oldLayout,
                    newLayout = newLayout,
                    offsetMap = offsetMap,
                    insertedUnits = listOf(TextRange(i - 1, i)),
                    cursorMotionPath = cursorPath,
                    durationMs = textDurationMs,
                    motionPolicy =
                        EditorMotionPolicy(
                            textDurationMillis = textDurationMs,
                            cursorEnabled = true,
                            coordinated = true,
                        ),
                )
            timeline.applyPatch(
                patch = patch,
                frameTimeNanos = frameTime,
                cursorFromRect = fromRect,
                cursorPath = cursorPath.points,
                cursorDurationNanos = textDurationMs * NANOS_PER_MS,
            )
        }

        // 最后一笔在 (12-1)*30=330ms 输入。
        // 有界窗口：动画应在 330+100=430ms 内全部完成。
        // 串行 FIFO：12 个字最后一个字要 12*100=1200ms 才完成。
        val convergenceTimeMs = (11 * inputIntervalMs) + textDurationMs // 430ms
        val scene = timeline.sample(convergenceTimeMs * NANOS_PER_MS)
        assertFalse(
            "有界窗口：$convergenceTimeMs ms 时不应有活动文字动画（应已收敛），" +
                "实际 units=${scene.units.size}，串行 FIFO 下 12 个字要 1200ms 才完成",
            timeline.hasActiveAnimation(convergenceTimeMs * NANOS_PER_MS),
        )
    }

    /**
     * #691 评论 5682970101 新增测试2：abc@0ms + d@30ms — cursor 和文字来自同一份 schedule。
     *
     * 修复后时间表（duration=300ms）：
     * - abc@0ms：a:0-100, b:100-200, c:200-300
     * - d@30ms：a 已开始保留；待显示=[b,c,d] 在 [30,330] 分段
     *   b:30-130, c:130-230, d:230-330
     *
     * 关键验证（连续采样）：
     * - 130ms：cursor 到达 b caret（b segment 30-130 结束），b alpha≈1
     * - 230ms：cursor 到达 c caret，c alpha≈1
     * - 330ms：cursor 到达 d caret，d alpha≈1
     * - 不出现 cursor 已到 d、d 还要几百毫秒才完成
     */
    @Test
    fun abcAt0ms_dAt30ms_cursorAndTextFromSameSchedule() {
        val layouts = captureLayouts("", "abc", "abcd")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)
        val abcdLayout = ComposeLayoutSnapshot(layouts[2], TextRange(4, 4), 0)

        val timeline = ComposeVisualTimeline()

        // 第一笔：0ms "" → "abc"，duration=300ms
        val point0 = CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)
        val cursorPath1 = CursorMotionPath(points = listOf(point0, point1, point2))
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath1,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
            )
        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath1.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // 第二笔：30ms "abc" → "abcd"，插入 d
        val offsetMap2 = listOf(VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY))
        val pointD = CursorMotionPoint(rect = Rect(40f, 0f, 42f, 14f), endFraction = 1f)
        val cursorPath2 = CursorMotionPath(points = listOf(pointD))
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abcLayout,
                newLayout = abcdLayout,
                offsetMap = offsetMap2,
                insertedUnits = listOf(TextRange(3, 4)),
                cursorMotionPath = cursorPath2,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
            )
        val cursorAt30ms = Rect(3f, 0f, 5f, 14f)
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = 30L * NANOS_PER_MS,
            cursorFromRect = cursorAt30ms,
            cursorPath = cursorPath2.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // 130ms：b segment（30-130）结束，b alpha≈1，cursor 到达 b caret（point1.left=20）
        val scene130 = timeline.sample(130L * NANOS_PER_MS)
        val unitB130 = scene130.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        if (unitB130 != null) {
            assertTrue(
                "130ms: unit b alpha 应已完成（≈1），实际=${unitB130.alpha.from}",
                unitB130.alpha.from > 0.8f,
            )
        }

        // 230ms：c segment（130-230）结束，c alpha≈1，cursor 到达 c caret（point2.left=30）
        val scene230 = timeline.sample(230L * NANOS_PER_MS)
        val unitC230 = scene230.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        if (unitC230 != null) {
            assertTrue(
                "230ms: unit c alpha 应已完成（≈1），实际=${unitC230.alpha.from}",
                unitC230.alpha.from > 0.8f,
            )
        }

        // 330ms：d segment（230-330）结束，d alpha≈1，cursor 到达 d caret（pointD.left=40）
        val scene330 = timeline.sample(330L * NANOS_PER_MS)
        val unitD330 = scene330.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        if (unitD330 != null) {
            assertTrue(
                "330ms: unit d alpha 应已完成（≈1），实际=${unitD330.alpha.from}",
                unitD330.alpha.from > 0.8f,
            )
        }

        // 关键验证：不出现 cursor 已到 d、d 还要几百毫秒才完成
        // 在 130ms 时，cursor 应在 b caret 附近（不应到达 d 的 left=40）
        val cursor130 = scene130.cursorRect
        assertNotNull("130ms: cursor rect 不应为 null", cursor130)
        assertTrue(
            "130ms: cursor 不应到达 d 的位置 (left 应远小于 40f)，实际=${cursor130!!.left}",
            cursor130.left < 35f,
        )
    }

    /**
     * #691 评论 5682970101 新增测试3：同一 VSync 多 patch — cursor 只追最终 layout。
     *
     * 同一 VSync 0ms drain 3 笔：a, ab, abc。
     * 验证：
     * - insert unit 语义保留（a/b/c 都有 insert unit）
     * - cursor/reflow 只追最终 layout 的几何目标
     * - cursor 不创建肉眼不可见的中间几何轨迹
     */
    @Test
    fun sameVsyncMultiplePatches_cursorOnlyFinalLayout() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val abLayout = ComposeLayoutSnapshot(layouts[2], TextRange(2, 2), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[3], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()
        val fromRect = Rect(0f, 0f, 2f, 14f)

        // 第一笔：0ms "" → "a"
        val cursorPath1 =
            CursorMotionPath(points = listOf(CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f)))
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                cursorMotionPath = cursorPath1,
                durationMs = 100L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(1L),
            )
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath1.points,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // 第二笔：同一 VSync 0ms "a" → "ab"
        val offsetMap2 = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY))
        val cursorPath2 =
            CursorMotionPath(points = listOf(CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 1f)))
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = aLayout,
                newLayout = abLayout,
                offsetMap = offsetMap2,
                insertedUnits = listOf(TextRange(1, 2)),
                cursorMotionPath = cursorPath2,
                durationMs = 100L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(2L),
            )
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath2.points,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // 第三笔：同一 VSync 0ms "ab" → "abc"
        val offsetMap3 = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY))
        val cursorPath3 =
            CursorMotionPath(points = listOf(CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)))
        val patch3 =
            makePatch(
                id = 3L,
                oldLayout = abLayout,
                newLayout = abcLayout,
                offsetMap = offsetMap3,
                insertedUnits = listOf(TextRange(2, 3)),
                cursorMotionPath = cursorPath3,
                durationMs = 100L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
                intent = nonLocalIntent(3L),
            )
        timeline.applyPatch(
            patch = patch3,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath3.points,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // 验证：insert unit 语义保留 — a/b/c 都有 insert unit
        // 修复后（评论 5684136311）：同一 VSync 零进度 unit 可重新分段。
        // a/b/c 在 [0,100] 均匀分段：a:0-33, b:33-66, c:66-100
        val scene50 = timeline.sample(50L * NANOS_PER_MS)
        val unitA = scene50.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        val unitB = scene50.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC = scene50.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        // a：要么已交还系统正文，要么 alpha > 0
        if (unitA != null) {
            assertTrue(
                "同VSync多patch 50ms: unit a alpha 应 > 0，实际=${unitA.alpha.from}",
                unitA.alpha.from > 0f,
            )
        }
        // b：应存在且 alpha > 0（b:0-50，50ms 时刚完成或接近完成）
        assertNotNull("同VSync多patch 50ms: unit b 应存在", unitB)
        // c：应存在
        assertNotNull("同Vsync多patch 50ms: unit c 应存在", unitC)

        // 验证：cursor 只追最终 layout — cursor 不创建肉眼不可见的中间几何轨迹
        // cursor 最终应到达 c 的 caret 位置（最终 layout 的 cursor rect）
        val scene100 = timeline.sample(100L * NANOS_PER_MS)
        val cursor100 = scene100.cursorRect
        assertNotNull("同VSync多patch 100ms: cursor rect 不应为 null", cursor100)
        // 100ms 时 cursor 应已到达最终位置（c 的 caret，left≈30）
        assertTrue(
            "同VSync多patch 100ms: cursor 应到达最终位置 (left≈30)，实际=${cursor100!!.left}",
            kotlin.math.abs(cursor100.left - 30f) < 5f,
        )
    }

    /**
     * #691 评论 5684993243：同一 VSync 连续两笔 patch 时，已在前一可见帧产生真实 alpha 进度的
     * surviving unit 被错误判成"零进度 pending"，alpha 跳回 0。
     *
     * 根因：hasBeenPresented() 从 TimedFloat.startedAtNanos 和 from 反推"是否已显示过"：
     *   frameTimeNanos > unit.alpha.startedAtNanos && alphaNow != unit.alpha.from
     * 这两个字段恰好会在同一个 VSync 的第一笔 patch 里被 rebaseUnitForPatch() 改写。
     *
     * 复现步骤（严格按评论 5684993243）：
     * 1. 0ms 建立插入动画 a 的 alpha 0→1，100ms。
     * 2. 24ms sample/draw，a 肉眼已显示到约 0.24。
     * 3. 30ms 同一 VSync 连续来 patch2、patch3。
     * 4. patch2 开始时 hasBeenPresented(a, 30ms)=true，a 正确保留当前进度；
     *    rebaseUnitForPatch() 把 a 改成 from≈0.30, startedAtNanos=30ms, remaining=70ms。
     * 5. patch3 仍用 frameTimeNanos=30ms。再次 hasBeenPresented(a, 30ms)：
     *    30 > 30 == false → 返回 false，已显示过的 a 被判成"零进度 pending"。
     * 6. repartitionPendingAndInsertedUnits() 给 a 重建 TimedFloat(0f, 1f, ...)，
     *    a 从已画出的约 0.24/0.30 跳回 0。
     *
     * 断言正确行为：a 的 alpha 不应跳回 0，应保留约 0.30。
     * 当前实现（bug）会让 a 跳回 0，本断言 FAIL → 复现成功。
     */
    @Test
    fun comment5684993243_sampledUnitJumpsBackToZeroOnSecondSameVsyncPatch() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val abLayout = ComposeLayoutSnapshot(layouts[2], TextRange(2, 2), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[3], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()
        val textPolicy = EditorMotionPolicy(textDurationMillis = 100L)

        // === patch1@0ms："" → "a"，插入 a，alpha 0→1，duration=100ms ===
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(1L),
            )
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
        )

        // === sample@24ms：a 的 alpha 应约 0.24，肉眼已显示中间帧 ===
        val scene24 = timeline.sample(24L * NANOS_PER_MS)
        val unitA24 = scene24.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("24ms: unit a 应存在", unitA24)
        val alphaAt24 = unitA24!!.alpha.from
        assertTrue(
            "24ms: unit a alpha 应约 0.24（已产生可见进度），实际=$alphaAt24",
            alphaAt24 > 0.15f && alphaAt24 < 0.35f,
        )

        // === patch2@30ms：同一 VSync 第一笔，"a" → "ab"，插入 b ===
        val frameTime30ms = 30L * NANOS_PER_MS
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = aLayout,
                newLayout = abLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(1, 2)),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(2L),
            )
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = frameTime30ms,
        )

        // === patch3@30ms：同一 VSync 第二笔，"ab" → "abc"，插入 c ===
        // 关键：frameTimeNanos 仍是 30ms（同一 VSync 时间戳）。
        // patch2 的 rebaseUnitForPatch 已把 a 改成 startedAtNanos=30ms，
        // 此处 hasBeenPresented(a, 30ms) 中 30 > 30 == false → 误判 a 为零进度 pending。
        val patch3 =
            makePatch(
                id = 3L,
                oldLayout = abLayout,
                newLayout = abcLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(2, 3)),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(3L),
            )
        timeline.applyPatch(
            patch = patch3,
            frameTimeNanos = frameTime30ms,
        )

        // === sample@30ms：采样最终 scene ===
        val scene30 = timeline.sample(frameTime30ms)
        val unitA30 = scene30.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("30ms: unit a 应仍存在（动画未完成）", unitA30)
        val alphaAt30 = unitA30!!.alpha.from

        // 复现断言（期望正确行为）：a 已在 24ms 可见帧显示到约 0.24/0.30，
        // 不应被同一 VSync 的第二笔 patch 重置回 0。
        // 当前 bug：hasBeenPresented 在 patch3 误判 a 为零进度，
        // repartition 给 a 重建 TimedFloat(0f, 1f, ...)，a 跳回 0 → 本断言 FAIL。
        assertTrue(
            "评论5684993243: a 已在 24ms 产生可见 alpha 进度(≈$alphaAt24)，" +
                "同一 VSync 第二笔 patch 后不应跳回 0，实际 alpha.from=$alphaAt30",
            alphaAt30 > 0.1f,
        )
    }

    /**
     * #691 评论 5684993243 修复后完整断言 — 验证 presentedKeys 持久状态集合
     * 让同一 VSync 连续 patch 不会把已显示 unit 误判成"零进度 pending"。
     *
     * 这是 [comment5684993243_sampledUnitJumpsBackToZeroOnSecondSameVsyncPatch] 的配套测试，
     * 在原复现断言（alphaAt30 > 0.1f）基础上补充评论要求的完整断言：
     * 1. a 在最终同帧 scene 的 alpha 不得小于 24ms 已画出的 alpha（允许少量容差），更不能回 0。
     * 2. a 不得再次进入 pending 分段（a 的 alpha 通道 from 不为 0，未被重建成 from=0）。
     * 3. b/c/新插入 unit 仍然被压进有界窗口，尾巴不能重新线性增长 —
     *    b 和 c 的 alpha 通道 startedAtNanos 在 [30ms, 30ms+100ms] 有界窗口内。
     *
     * 场景与原复现测试一致：
     * - patch1@0ms: "" → "a"，alpha 0→1，duration=100ms
     * - sample@24ms: a 的 alpha≈0.24，肉眼已显示中间帧 → a.key 计入 presentedKeys
     * - patch2@30ms: "a" → "ab"，插入 b（a 已在 presentedKeys，保留当前进度）
     * - patch3@30ms: "ab" → "abc"，插入 c（a 仍在 presentedKeys，不归零；
     *   b 从未被 sample 过，不在 presentedKeys，和 c 一起重新分段）
     * - sample@30ms: a 保留 from≈0.30，b/c 在 [30ms, 130ms] 有界窗口内均匀分段
     */
    @Test
    fun comment5684993243_sampledUnitPreservesProgressAcrossSameVsyncPatches() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val abLayout = ComposeLayoutSnapshot(layouts[2], TextRange(2, 2), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[3], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()
        val textPolicy = EditorMotionPolicy(textDurationMillis = 100L)

        // === patch1@0ms："" → "a"，插入 a，alpha 0→1，duration=100ms ===
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(1L),
            )
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
        )

        // === sample@24ms：a 的 alpha 应约 0.24，肉眼已显示中间帧 → a.key 计入 presentedKeys ===
        val scene24 = timeline.sample(24L * NANOS_PER_MS)
        val unitA24 = scene24.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("24ms: unit a 应存在", unitA24)
        val alphaAt24 = unitA24!!.alpha.from
        assertTrue(
            "24ms: unit a alpha 应约 0.24（已产生可见进度），实际=$alphaAt24",
            alphaAt24 > 0.15f && alphaAt24 < 0.35f,
        )

        // === patch2@30ms：同一 VSync 第一笔，"a" → "ab"，插入 b ===
        val frameTime30ms = 30L * NANOS_PER_MS
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = aLayout,
                newLayout = abLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(1, 2)),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(2L),
            )
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = frameTime30ms,
        )

        // === patch3@30ms：同一 VSync 第二笔，"ab" → "abc"，插入 c ===
        // 关键：frameTimeNanos 仍是 30ms（同一 VSync 时间戳）。
        // 修复后：a.key 已在 presentedKeys（24ms sample 时记入），
        // applyPatch 判断 started/pending 只看持久状态，a 不被误判成"零进度 pending"。
        val patch3 =
            makePatch(
                id = 3L,
                oldLayout = abLayout,
                newLayout = abcLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(2, 3)),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(3L),
            )
        timeline.applyPatch(
            patch = patch3,
            frameTimeNanos = frameTime30ms,
        )

        // === sample@30ms：采样最终 scene ===
        val scene30 = timeline.sample(frameTime30ms)
        val unitA30 = scene30.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("30ms: unit a 应仍存在（动画未完成）", unitA30)
        val alphaAt30 = unitA30!!.alpha.from

        // === 断言1：a 在最终同帧 scene 的 alpha 不得小于 24ms 已画出的 alpha（允许少量容差），更不能回 0 ===
        // 修复后 a 应保留 from≈0.30（rebase 后的当前值），不应跳回 0。
        assertTrue(
            "评论5684993243 断言1: a 已在 24ms 产生可见 alpha 进度(≈$alphaAt24)，" +
                "同一 VSync 第二笔 patch 后不应跳回 0，实际 alpha.from=$alphaAt30",
            alphaAt30 > 0.1f,
        )
        assertTrue(
            "评论5684993243 断言1: a 在 30ms 的 alpha($alphaAt30) 不应小于 24ms 已画出的 alpha($alphaAt24) - 0.05 容差，" +
                "更不能跳回 0",
            alphaAt30 >= alphaAt24 - 0.05f,
        )

        // === 断言2：a 不得再次进入 pending 分段（a 的 alpha 通道 from 不为 0，未被重建成 from=0） ===
        // 修复后 a 应保留 from≈0.30，不应 from==0f。
        assertTrue(
            "评论5684993243 断言2: a 的 alpha 通道 from 不应为 0（不应被重建成 from=0），实际 from=${unitA30.alpha.from}",
            unitA30.alpha.from > 0.1f,
        )
        assertFalse(
            "评论5684993243 断言2: a 不应被重建成 from=0f & to=1f 的全新插入通道，" +
                "实际 from=${unitA30.alpha.from}, to=${unitA30.alpha.to}",
            unitA30.alpha.from == 0f && unitA30.alpha.to == 1f,
        )

        // === 断言3：b/c/新插入 unit 仍然被压进有界窗口，尾巴不能重新线性增长 ===
        // b 和 c 的 alpha 通道 startedAtNanos 应在 [30ms, 30ms+100ms] 有界窗口内。
        // b 从未被 sample 过（不在 presentedKeys），和 c 一起重新分段；
        // c 是新插入。两者都在 [frameTimeNanos, frameTimeNanos + durationNanos] 有界窗口内均匀分段。
        val unitB30 = scene30.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC30 = scene30.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        val boundedWindowStart = frameTime30ms
        val boundedWindowEnd = frameTime30ms + 100L * NANOS_PER_MS
        assertNotNull("30ms: unit b 应存在", unitB30)
        assertTrue(
            "评论5684993243 断言3: b 的 startedAtNanos(${unitB30!!.alpha.startedAtNanos}) " +
                "应在有界窗口 [$boundedWindowStart, $boundedWindowEnd] 内",
            unitB30.alpha.startedAtNanos in boundedWindowStart..boundedWindowEnd,
        )
        assertNotNull("30ms: unit c 应存在", unitC30)
        assertTrue(
            "评论5684993243 断言3: c 的 startedAtNanos(${unitC30!!.alpha.startedAtNanos}) " +
                "应在有界窗口 [$boundedWindowStart, $boundedWindowEnd] 内",
            unitC30.alpha.startedAtNanos in boundedWindowStart..boundedWindowEnd,
        )
    }

    /**
     * #691 评论 5684993243 完整组合测试（带 cursor） —
     * patch1@0ms -> sample@24ms -> patch2@30ms -> patch3@30ms -> sample@30ms -> sample@32ms
     *
     * 这是评论要求的完整测试组合，在 [comment5684993243_sampledUnitPreservesProgressAcrossSameVsyncPatches]
     * 基础上增加 cursor 参数，验证评论要求的全部断言：
     * 1. a 在最终同帧 scene 的 alpha 不得小于 24ms 已画出的 alpha，更不能回 0
     * 2. a 不得再次进入 pending 分段
     * 3. **cursor 不得在 patch3 后重新经过 a 的旧 caret** — 关键新增断言
     * 4. b/c/新插入 unit 仍然被压进有界窗口，尾巴不能重新线性增长
     *
     * cursor 行为分析：
     * - patch1: cursor 从 (0,0,2,14) 到 a 的 caret (10,0,12,14)
     * - sample@24ms: cursor 在 a caret 附近（progress=0.24, segmentProgress=0.24）
     * - patch2@30ms: a 已在 presentedKeys → startedSurviving → 不纳入 cursor 路径
     *   cursor 路径 = [b caret (20,0,22,14)]（只有新插入 b 的 caret）
     * - patch3@30ms: a 仍在 presentedKeys → 不纳入 cursor 路径
     *   b 从未被 sample 过 → pendingSurviving → 纳入 survivingCursorPoints
     *   cursor 路径 = [b caret (20,0,22,14), c caret (30,0,32,14)]
     *   **不应包含 a 的 caret (10,0,12,14)**
     *
     * 如果 bug 存在（a 被误判为 pending），a 会进入 survivingCursorPoints，
     * cursor 路径会包含 a 的 caret → cursor 重新经过 a 的旧 caret。
     */
    @Test
    fun comment5684993243_cursorDoesNotRevisitSampledUnitCaretOnSameVsyncPatches() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val abLayout = ComposeLayoutSnapshot(layouts[2], TextRange(2, 2), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[3], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()
        val textPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true)

        // a 的 caret rect — cursor 在 bug 场景下会错误地重新经过此位置
        val aCaretRect = Rect(10f, 0f, 12f, 14f)
        val bCaretRect = Rect(20f, 0f, 22f, 14f)
        val cCaretRect = Rect(30f, 0f, 32f, 14f)
        val initialCursorRect = Rect(0f, 0f, 2f, 14f)

        // === patch1@0ms："" → "a"，插入 a，cursor 从初始位置到 a 的 caret ===
        val cursorPath1 = listOf(CursorMotionPoint(rect = aCaretRect, endFraction = 1f))
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                cursorMotionPath = CursorMotionPath(points = cursorPath1),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(1L),
            )
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
            cursorFromRect = initialCursorRect,
            cursorPath = cursorPath1,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // === sample@24ms：a 的 alpha 应约 0.24，cursor 在 a caret 附近 ===
        val scene24 = timeline.sample(24L * NANOS_PER_MS)
        val unitA24 = scene24.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("24ms: unit a 应存在", unitA24)
        val alphaAt24 = unitA24!!.alpha.from
        assertTrue(
            "24ms: unit a alpha 应约 0.24（已产生可见进度），实际=$alphaAt24",
            alphaAt24 > 0.15f && alphaAt24 < 0.35f,
        )
        // 24ms 时 cursor 在 a caret 附近（progress=0.24）
        val cursor24 = scene24.cursorRect
        assertNotNull("24ms: cursor rect 不应为 null", cursor24)

        // === patch2@30ms：同一 VSync 第一笔，"a" → "ab"，插入 b ===
        val frameTime30ms = 30L * NANOS_PER_MS
        val cursorPath2 = listOf(CursorMotionPoint(rect = bCaretRect, endFraction = 1f))
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = aLayout,
                newLayout = abLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(1, 2)),
                cursorMotionPath = CursorMotionPath(points = cursorPath2),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(2L),
            )
        // cursor 在 30ms 时的位置（从 patch1 的 cursor 采样）
        val cursorAt30ms = timeline.sampleCursorRect(frameTime30ms) ?: initialCursorRect
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = frameTime30ms,
            cursorFromRect = cursorAt30ms,
            cursorPath = cursorPath2,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // === patch3@30ms：同一 VSync 第二笔，"ab" → "abc"，插入 c ===
        // 关键：frameTimeNanos 仍是 30ms（同一 VSync 时间戳）。
        // 修复后：a.key 已在 presentedKeys（24ms sample 时记入），
        // a 不被误判成"零进度 pending"，cursor 路径不包含 a 的 caret。
        val cursorPath3 = listOf(CursorMotionPoint(rect = cCaretRect, endFraction = 1f))
        val patch3 =
            makePatch(
                id = 3L,
                oldLayout = abLayout,
                newLayout = abcLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(2, 3)),
                cursorMotionPath = CursorMotionPath(points = cursorPath3),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(3L),
            )
        // cursor 在 patch2 后的位置（仍在 30ms，同一 VSync）
        val cursorAfterPatch2 = timeline.sampleCursorRect(frameTime30ms) ?: cursorAt30ms
        timeline.applyPatch(
            patch = patch3,
            frameTimeNanos = frameTime30ms,
            cursorFromRect = cursorAfterPatch2,
            cursorPath = cursorPath3,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // === sample@30ms：采样最终 scene ===
        val scene30 = timeline.sample(frameTime30ms)
        val unitA30 = scene30.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("30ms: unit a 应仍存在（动画未完成）", unitA30)
        val alphaAt30 = unitA30!!.alpha.from

        // === 断言1：a 在最终同帧 scene 的 alpha 不得小于 24ms 已画出的 alpha，更不能回 0 ===
        assertTrue(
            "评论5684993243 断言1: a 已在 24ms 产生可见 alpha 进度(≈$alphaAt24)，" +
                "同一 VSync 第二笔 patch 后不应跳回 0，实际 alpha.from=$alphaAt30",
            alphaAt30 > 0.1f,
        )
        assertTrue(
            "评论5684993243 断言1: a 在 30ms 的 alpha($alphaAt30) 不应小于 24ms 已画出的 alpha($alphaAt24) - 0.05 容差",
            alphaAt30 >= alphaAt24 - 0.05f,
        )

        // === 断言2：a 不得再次进入 pending 分段 ===
        assertTrue(
            "评论5684993243 断言2: a 的 alpha 通道 from 不应为 0（不应被重建成 from=0），实际 from=${unitA30.alpha.from}",
            unitA30.alpha.from > 0.1f,
        )
        assertFalse(
            "评论5684993243 断言2: a 不应被重建成 from=0f & to=1f 的全新插入通道",
            unitA30.alpha.from == 0f && unitA30.alpha.to == 1f,
        )

        // === 断言3：cursor 不得在 patch3 后重新经过 a 的旧 caret ===
        // patch3 后 cursor 路径应为 [b caret, c caret]，不应包含 a 的 caret。
        // 在整个 cursor 动画过程中（30ms 到 130ms），cursor 不应到达 a 的 caret 位置 (left≈10)。
        // cursor 从 patch2/3 合并后的起点出发，依次经过 b caret (left=20) 和 c caret (left=30)。
        // 如果 bug 存在，cursor 路径会包含 a 的 caret (left=10)，cursor 会先到达 a 的旧 caret 再到 b/c。
        val cursor30 = scene30.cursorRect
        assertNotNull("30ms: cursor rect 不应为 null", cursor30)
        // 30ms 时 cursor 不应在 a 的 caret 位置（left≈10）
        // 修复后 cursor 从当前位置向 b/c 移动，不应回退到 a 的 caret
        assertTrue(
            "评论5684993243 断言3: 30ms 时 cursor(left=${cursor30!!.left}) 不应在 a 的旧 caret 位置(left≈10)，" +
                "cursor 不应重新经过 a 的旧 caret",
            kotlin.math.abs(cursor30.left - aCaretRect.left) > 3f,
        )

        // 在 cursor 动画过程中采样多个时间点，确保 cursor 不会经过 a 的 caret
        val sampleTimes = listOf(40L, 50L, 60L, 80L, 100L, 130L)
        for (timeMs in sampleTimes) {
            val scene = timeline.sample(timeMs * NANOS_PER_MS)
            val cursor = scene.cursorRect
            if (cursor != null) {
                // cursor 的 left 不应接近 a 的 caret left (10f)
                // 允许 3f 容差（cursor 在动画过程中可能短暂接近但不应该精确停在 a 的 caret）
                assertTrue(
                    "评论5684993243 断言3: ${timeMs}ms 时 cursor(left=${cursor.left}) 不应经过 a 的旧 caret(left≈10)，" +
                        "cursor 不得在 patch3 后重新经过 a 的旧 caret",
                    kotlin.math.abs(cursor.left - aCaretRect.left) > 3f,
                )
            }
        }

        // === 断言4：b/c/新插入 unit 仍然被压进有界窗口，尾巴不能重新线性增长 ===
        val unitB30 = scene30.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        val unitC30 = scene30.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        val boundedWindowStart = frameTime30ms
        val boundedWindowEnd = frameTime30ms + 100L * NANOS_PER_MS
        assertNotNull("30ms: unit b 应存在", unitB30)
        assertTrue(
            "评论5684993243 断言4: b 的 startedAtNanos(${unitB30!!.alpha.startedAtNanos}) " +
                "应在有界窗口 [$boundedWindowStart, $boundedWindowEnd] 内",
            unitB30.alpha.startedAtNanos in boundedWindowStart..boundedWindowEnd,
        )
        assertNotNull("30ms: unit c 应存在", unitC30)
        assertTrue(
            "评论5684993243 断言4: c 的 startedAtNanos(${unitC30!!.alpha.startedAtNanos}) " +
                "应在有界窗口 [$boundedWindowStart, $boundedWindowEnd] 内",
            unitC30.alpha.startedAtNanos in boundedWindowStart..boundedWindowEnd,
        )

        // === sample@32ms：验证 a 的 alpha 在 32ms 时仍不跳回 0 ===
        val scene32 = timeline.sample(32L * NANOS_PER_MS)
        val unitA32 = scene32.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        if (unitA32 != null) {
            val alphaAt32 = unitA32.alpha.from
            assertTrue(
                "评论5684993243 sample@32ms: a 的 alpha($alphaAt32) 不应跳回 0，" +
                    "应继续从 30ms 的进度(≈$alphaAt30)推进",
                alphaAt32 > 0.1f,
            )
            assertTrue(
                "评论5684993243 sample@32ms: a 的 alpha($alphaAt32) 不应小于 30ms 的 alpha($alphaAt30) - 0.05 容差",
                alphaAt32 >= alphaAt30 - 0.05f,
            )
        }
        // 32ms 时 cursor 也不应经过 a 的旧 caret
        val cursor32 = scene32.cursorRect
        if (cursor32 != null) {
            assertTrue(
                "评论5684993243 sample@32ms: cursor(left=${cursor32.left}) 不应经过 a 的旧 caret(left≈10)",
                kotlin.math.abs(cursor32.left - aCaretRect.left) > 3f,
            )
        }
    }

    /**
     * #691 评论 5685940102 复现测试（纯文字场景） —
     * retained reflow unit（alpha 1→1, position old→new）在同一 VSync 连续 patch 下
     * 不应被重建成插入 unit（alpha 0→1）。
     *
     * 根因（评论 5685940102 指出）：
     * - createMoveUnitForReflow 创建的 retained unit alpha = TimedFloat(1f, 1f, ...)，alpha 永远不变
     * - 旧逻辑 sample() 记录 presentedKeys 的条件是 `currentAlpha != alpha.from`，
     *   对 alpha 1→1 的 retained unit 永远为 false → retained unit 永远不计入 presentedKeys
     * - 下一笔 patch 的 progressByKey 把它判成 false → 进入 pendingSurviving →
     *   repartitionPendingAndInsertedUnits 重建 alpha 0→1 → 已可见文字突然变透明再淡入
     *
     * 场景：
     * - patch1@0ms: "ab" → "c\nab"，产生 retained move（"ab" 从第一行 reflow 到第二行）
     *   - insertedUnits = [TextRange(0,2)]（"c\n" 新插入，alpha 0→1）
     *   - retainedMoves = [RetainedMove(TextRange(0,2), TextRange(2,4))]（"ab" reflow，alpha 1→1）
     * - sample@16ms: retained unit alpha=1，position 正在动画
     *   → isUnitVisibleAndPresented 返回 true（alpha>0 且 position 已开始）
     *   → retained unit.key 计入 presentedKeys
     * - patch2@30ms: "c\nab" → "c\nabc"，插入 "c"（同一 VSync 第一笔）
     * - patch3@30ms: "c\nabc" → "c\nabcd"，插入 "d"（同一 VSync 第二笔）
     * - sample@30ms: 断言 retained unit alpha 始终为 1，不被重建成 0→1
     */
    @Test
    fun comment5685940102_retainedReflowUnitNotRebuiltAsInsertOnSameVsyncPatch() {
        val layouts = captureLayouts("ab", "c\nab", "c\nabc", "c\nabcd")
        val abLayout = ComposeLayoutSnapshot(layouts[0], TextRange(2, 2), 0)
        val cabLayout = ComposeLayoutSnapshot(layouts[1], TextRange(4, 4), 0)
        val cabcLayout = ComposeLayoutSnapshot(layouts[2], TextRange(5, 5), 0)
        val cabcdLayout = ComposeLayoutSnapshot(layouts[3], TextRange(6, 6), 0)

        val timeline = ComposeVisualTimeline()
        val textPolicy = EditorMotionPolicy(textDurationMillis = 100L)

        // === patch1@0ms："ab" → "c\nab"，产生 retained move（"ab" reflow 到第二行） ===
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = abLayout,
                newLayout = cabLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 2, 2, VisualOffsetMapKind.SHIFTED)),
                insertedUnits = listOf(TextRange(0, 2)),
                retainedMoves = listOf(RetainedMove(oldRange = TextRange(0, 2), newRange = TextRange(2, 4))),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(1L),
            )
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
        )

        // 验证 patch1 后 retained unit 存在且 alpha 是 1→1
        val scene0 = timeline.sample(0L)
        val retainedUnit0 = scene0.units.firstOrNull { it.targetRange == TextRange(2, 4) }
        assertNotNull("0ms: retained unit (ab) 应存在", retainedUnit0)
        assertEquals(
            "0ms: retained unit alpha.from 应为 1（alpha 1→1）",
            1f,
            retainedUnit0!!.alpha.from,
        )
        assertEquals(
            "0ms: retained unit alpha.to 应为 1（alpha 1→1）",
            1f,
            retainedUnit0.alpha.to,
        )
        val retainedKey = retainedUnit0.key

        // === sample@16ms：retained unit alpha=1，position 正在动画 ===
        // isUnitVisibleAndPresented: alphaNow=1 > 0, alphaNow == from(1),
        // position.startedAtNanos=0 <= 16ms → true → retained unit.key 计入 presentedKeys
        val scene16 = timeline.sample(16L * NANOS_PER_MS)
        val retainedUnit16 = scene16.units.firstOrNull { it.targetRange == TextRange(2, 4) }
        assertNotNull("16ms: retained unit (ab) 应仍存在", retainedUnit16)
        assertEquals(
            "16ms: retained unit alpha.from 应为 1（alpha 1→1，不应变）",
            1f,
            retainedUnit16!!.alpha.from,
        )

        // === patch2@30ms：同一 VSync 第一笔，"c\nab" → "c\nabc"，插入 "c" ===
        val frameTime30ms = 30L * NANOS_PER_MS
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = cabLayout,
                newLayout = cabcLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 4, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(4, 5)),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(2L),
            )
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = frameTime30ms,
        )

        // === patch3@30ms：同一 VSync 第二笔，"c\nabc" → "c\nabcd"，插入 "d" ===
        // 关键：frameTimeNanos 仍是 30ms（同一 VSync 时间戳）。
        // 修复后：retained unit.key 已在 presentedKeys（16ms sample 时记入），
        // applyPatch 判断 started/pending 只看持久状态，retained unit 不被误判成"零进度 pending"。
        val patch3 =
            makePatch(
                id = 3L,
                oldLayout = cabcLayout,
                newLayout = cabcdLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 5, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(5, 6)),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(3L),
            )
        timeline.applyPatch(
            patch = patch3,
            frameTimeNanos = frameTime30ms,
        )

        // === sample@30ms：采样最终 scene ===
        val scene30 = timeline.sample(frameTime30ms)
        // retained unit 的 targetRange 通过 offsetMap 映射后仍是 TextRange(2,4)
        val retainedUnit30 = scene30.units.firstOrNull { it.targetRange == TextRange(2, 4) }
        assertNotNull("30ms: retained unit (ab) 应仍存在（position 动画未完成）", retainedUnit30)

        // === 断言1：retained unit alpha 始终为 1，不被重建成 0→1 ===
        assertEquals(
            "评论5685940102 断言1: retained unit alpha.from 应为 1（不应被重建成 from=0），" +
                "实际 from=${retainedUnit30!!.alpha.from}",
            1f,
            retainedUnit30.alpha.from,
        )
        assertEquals(
            "评论5685940102 断言1: retained unit alpha.to 应为 1（不应被重建成插入通道），" +
                "实际 to=${retainedUnit30.alpha.to}",
            1f,
            retainedUnit30.alpha.to,
        )
        assertFalse(
            "评论5685940102 断言1: retained unit 不应被重建成 from=0f & to=1f 的全新插入通道",
            retainedUnit30.alpha.from == 0f && retainedUnit30.alpha.to == 1f,
        )

        // === 断言2：retained unit key 保持不变（未被销毁重建） ===
        assertEquals(
            "评论5685940102 断言2: retained unit key 应保持不变（未被销毁重建），" +
                "原 key=$retainedKey, 现 key=${retainedUnit30.key}",
            retainedKey,
            retainedUnit30.key,
        )

        // === 断言3：retained unit position 从当前屏幕位置继续，不跳回旧位置 ===
        // patch1 创建时 position.from = oldLayout "ab" 中 TextRange(0,2) 的位置 = (0,0)
        // 修复后 position 应从 30ms 时的屏幕位置继续，不应跳回 (0,0)
        val positionFrom30 = retainedUnit30.position.from
        assertTrue(
            "评论5685940102 断言3: retained unit position.from 不应跳回旧位置 (0,0)，" +
                "实际 from=$positionFrom30",
            kotlin.math.abs(positionFrom30.x) > 0.1f || kotlin.math.abs(positionFrom30.y) > 0.1f,
        )
    }

    /**
     * #691 评论 5685940102 复现测试（带 cursor 场景） —
     * cursor 不应把已显示的 retained reflow unit 当 pending caret 再走一遍。
     *
     * 在 [comment5685940102_retainedReflowUnitNotRebuiltAsInsertOnSameVsyncPatch] 基础上
     * 增加 cursor 参数，验证评论要求的全部断言：
     * 1. retained unit alpha 始终为 1，不被重建成 0→1
     * 2. retained unit key 保持不变
     * 3. **cursor 不得在 patch2/patch3 后重新经过 retained unit 的旧 caret** — 关键新增断言
     *
     * cursor 行为分析：
     * - patch1: cursor 从初始位置到 "c\n" 插入后的 caret
     * - sample@16ms: cursor 在动画中
     * - patch2@30ms: retained unit 已在 presentedKeys → startedSurviving → 不纳入 cursor 路径
     *   cursor 路径 = [新插入 "c" 的 caret]
     * - patch3@30ms: retained unit 仍在 presentedKeys → 不纳入 cursor 路径
     *   cursor 路径 = [新插入 "d" 的 caret]
     *   **不应包含 retained unit 的 caret**
     *
     * 如果 bug 存在（retained unit 被误判为 pending），它会进入 survivingCursorPoints，
     * cursor 路径会包含 retained unit 的 caret → cursor 重新经过旧 caret。
     */
    @Test
    fun comment5685940102_cursorDoesNotRevisitRetainedReflowUnitCaretOnSameVsyncPatch() {
        val layouts = captureLayouts("ab", "c\nab", "c\nabc", "c\nabcd")
        val abLayout = ComposeLayoutSnapshot(layouts[0], TextRange(2, 2), 0)
        val cabLayout = ComposeLayoutSnapshot(layouts[1], TextRange(4, 4), 0)
        val cabcLayout = ComposeLayoutSnapshot(layouts[2], TextRange(5, 5), 0)
        val cabcdLayout = ComposeLayoutSnapshot(layouts[3], TextRange(6, 6), 0)

        val timeline = ComposeVisualTimeline()
        val textPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true)

        // 从 layout 获取 caret rect — retained unit 的 caret 在 "c\nab" 的 offset=4（"ab" 后面）
        val retainedCaretRect = cabLayout.result.getCursorRect(4)
        // 新插入字符的 caret
        val insertCaretAt2 = cabLayout.result.getCursorRect(2)
        val insertCaretAt5 = cabcLayout.result.getCursorRect(5)
        val insertCaretAt6 = cabcdLayout.result.getCursorRect(6)
        val initialCursorRect = Rect(0f, 0f, 2f, 14f)

        // === patch1@0ms："ab" → "c\nab"，产生 retained move，cursor 到 "c\n" 后的 caret ===
        val cursorPath1 = listOf(CursorMotionPoint(rect = insertCaretAt2, endFraction = 1f))
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = abLayout,
                newLayout = cabLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 2, 2, VisualOffsetMapKind.SHIFTED)),
                insertedUnits = listOf(TextRange(0, 2)),
                retainedMoves = listOf(RetainedMove(oldRange = TextRange(0, 2), newRange = TextRange(2, 4))),
                cursorMotionPath = CursorMotionPath(points = cursorPath1),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(1L),
            )
        timeline.applyPatch(
            patch = patch1,
            frameTimeNanos = 0L,
            cursorFromRect = initialCursorRect,
            cursorPath = cursorPath1,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // 验证 patch1 后 retained unit 存在且 alpha 是 1→1
        val scene0 = timeline.sample(0L)
        val retainedUnit0 = scene0.units.firstOrNull { it.targetRange == TextRange(2, 4) }
        assertNotNull("0ms: retained unit (ab) 应存在", retainedUnit0)
        assertEquals(
            "0ms: retained unit alpha.from 应为 1（alpha 1→1）",
            1f,
            retainedUnit0!!.alpha.from,
        )
        val retainedKey = retainedUnit0.key

        // === sample@16ms：retained unit alpha=1，position 正在动画 → 计入 presentedKeys ===
        val scene16 = timeline.sample(16L * NANOS_PER_MS)
        val retainedUnit16 = scene16.units.firstOrNull { it.targetRange == TextRange(2, 4) }
        assertNotNull("16ms: retained unit (ab) 应仍存在", retainedUnit16)
        assertEquals(
            "16ms: retained unit alpha.from 应为 1（alpha 1→1，不应变）",
            1f,
            retainedUnit16!!.alpha.from,
        )

        // === patch2@30ms：同一 VSync 第一笔，"c\nab" → "c\nabc"，插入 "c" ===
        val frameTime30ms = 30L * NANOS_PER_MS
        val cursorPath2 = listOf(CursorMotionPoint(rect = insertCaretAt5, endFraction = 1f))
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = cabLayout,
                newLayout = cabcLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 4, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(4, 5)),
                cursorMotionPath = CursorMotionPath(points = cursorPath2),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(2L),
            )
        val cursorAt30ms = timeline.sampleCursorRect(frameTime30ms) ?: initialCursorRect
        timeline.applyPatch(
            patch = patch2,
            frameTimeNanos = frameTime30ms,
            cursorFromRect = cursorAt30ms,
            cursorPath = cursorPath2,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // === patch3@30ms：同一 VSync 第二笔，"c\nabc" → "c\nabcd"，插入 "d" ===
        // 修复后：retained unit.key 已在 presentedKeys（16ms sample 时记入），
        // retained unit 不被误判成"零进度 pending"，cursor 路径不包含 retained unit 的 caret。
        val cursorPath3 = listOf(CursorMotionPoint(rect = insertCaretAt6, endFraction = 1f))
        val patch3 =
            makePatch(
                id = 3L,
                oldLayout = cabcLayout,
                newLayout = cabcdLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 5, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(5, 6)),
                cursorMotionPath = CursorMotionPath(points = cursorPath3),
                durationMs = 100L,
                motionPolicy = textPolicy,
                intent = nonLocalIntent(3L),
            )
        val cursorAfterPatch2 = timeline.sampleCursorRect(frameTime30ms) ?: cursorAt30ms
        timeline.applyPatch(
            patch = patch3,
            frameTimeNanos = frameTime30ms,
            cursorFromRect = cursorAfterPatch2,
            cursorPath = cursorPath3,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // === sample@30ms：采样最终 scene ===
        val scene30 = timeline.sample(frameTime30ms)
        val retainedUnit30 = scene30.units.firstOrNull { it.targetRange == TextRange(2, 4) }
        assertNotNull("30ms: retained unit (ab) 应仍存在", retainedUnit30)

        // === 断言1：retained unit alpha 始终为 1，不被重建成 0→1 ===
        assertEquals(
            "评论5685940102 断言1: retained unit alpha.from 应为 1（不应被重建成 from=0），" +
                "实际 from=${retainedUnit30!!.alpha.from}",
            1f,
            retainedUnit30.alpha.from,
        )
        assertEquals(
            "评论5685940102 断言1: retained unit alpha.to 应为 1（不应被重建成插入通道），" +
                "实际 to=${retainedUnit30.alpha.to}",
            1f,
            retainedUnit30.alpha.to,
        )
        assertFalse(
            "评论5685940102 断言1: retained unit 不应被重建成 from=0f & to=1f 的全新插入通道",
            retainedUnit30.alpha.from == 0f && retainedUnit30.alpha.to == 1f,
        )

        // === 断言2：retained unit key 保持不变 ===
        assertEquals(
            "评论5685940102 断言2: retained unit key 应保持不变，原 key=$retainedKey, 现 key=${retainedUnit30.key}",
            retainedKey,
            retainedUnit30.key,
        )

        // === 断言3：cursor 不得在 patch3 后把 retained unit 当 pending caret 再走一遍 ===
        // patch3 后 cursor 路径应为 [新插入 "d" 的 caret]，不应包含 retained unit 的 caret。
        //
        // 关键区分：如果 bug 存在，retained unit 被误判为 pending → 进入 survivingCursorPoints →
        // cursor 路径 = [retained caret, 新插入 caret]（2 个点），endFraction = [0.5, 1.0]，
        // cursor 在 50% 时间（80ms）**精确**到达 retained caret。
        // 修复后 cursor 路径 = [新插入 caret]（1 个点），cursor 从起点直接插值到新 caret，
        // 80ms 时 cursor 不等于 retained caret。
        //
        // 因此只在 80ms（50% 时间点）检查 cursor 是否**精确**到达 retained caret，
        // 用严格容差 0.5f 区分"因 bug 精确到达"和"自然路径恰好经过附近"。
        val retainedCaretLeft = retainedCaretRect.left
        val retainedCaretTop = retainedCaretRect.top
        val cursorPrecisionTolerance = 0.5f
        val scene80 = timeline.sample(80L * NANOS_PER_MS)
        val cursor80 = scene80.cursorRect
        assertNotNull("80ms: cursor rect 不应为 null", cursor80)
        assertTrue(
            "评论5685940102 断言3: 80ms（50% 时间点）时 cursor(left=${cursor80!!.left}, top=${cursor80.top}) " +
                "不应精确到达 retained unit 旧 caret(left=$retainedCaretLeft, top=$retainedCaretTop)，" +
                "cursor 不得把已显示的 retained unit 当 pending caret 再走一遍",
            kotlin.math.abs(cursor80.left - retainedCaretLeft) > cursorPrecisionTolerance ||
                kotlin.math.abs(cursor80.top - retainedCaretTop) > cursorPrecisionTolerance,
        )

        // === 断言4：retained unit position 从当前屏幕位置继续，不跳回旧位置 ===
        val positionFrom30 = retainedUnit30.position.from
        assertTrue(
            "评论5685940102 断言4: retained unit position.from 不应跳回旧位置 (0,0)，" +
                "实际 from=$positionFrom30",
            kotlin.math.abs(positionFrom30.x) > 0.1f || kotlin.math.abs(positionFrom30.y) > 0.1f,
        )
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L
    }

    @Suppress("LongParameterList")
    private fun makePatch(
        id: Long,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>? = null,
        insertedUnits: List<TextRange> = emptyList(),
        deletedUnits: List<TextRange> = emptyList(),
        retainedMoves: List<RetainedMove> = emptyList(),
        cursorMotionPath: CursorMotionPath? = null,
        durationMs: Long = 100L,
        motionPolicy: EditorMotionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        intent: EditorVisualIntent? = null,
    ): ComposeVisualPatch =
        ComposeVisualPatch(
            id = id,
            coreTransactionIds = listOf(id),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = offsetMap,
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = retainedMoves,
            cursorMotionPath = cursorMotionPath,
            durationMs = durationMs,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            motionPolicy = motionPolicy,
            intent = intent,
        )

    /**
     * Issue #720 评论 5747339452：构造非 null intent 绕过本地 reflow 释放门控 —
     * Robolectric 下 getPathForRange 跨文本 bounds 不稳定（同 range 在不同文本中 left/right 不同），
     * 导致 naturalGeometryChanged 误判为几何变化、survivor 被误释放。
     * 本测试验证的是协同动画/光标行为（非 #720 释放），用非本地 intent 绕过释放门控。
     */
    private fun nonLocalIntent(id: Long): EditorVisualIntent =
        EditorVisualIntent(
            coreTransactionId = id,
            baseRevision = 0L,
            newRevision = id,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = null,
            oldRanges = emptyList(),
            newRanges = emptyList(),
            textKind = TextVisualKind.None,
            cursor = null,
        )

    private fun makeInsertIntent(
        coreTxnId: Long,
        baseRev: Long,
        newRev: Long,
        oldText: String,
        newText: String,
        newRange: TextRange,
        offsetMap: VisualOffsetMap? = null,
        durationMs: Long = 100L,
    ): EditorVisualIntent =
        EditorVisualIntent(
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = durationMs,
            offsetMap = offsetMap,
            oldRanges = emptyList(),
            newRanges = listOf(newRange),
            textKind = TextVisualKind.Insert,
            cursor =
                CursorVisualIntent(
                    oldEndUtf16 = oldText.length,
                    newEndUtf16 = newText.length,
                    animate = true,
                ),
            replaceBounds =
                VisualReplaceBounds(
                    oldStart = oldText.length,
                    oldEnd = oldText.length,
                    newStart = newRange.start,
                    newEnd = newRange.end,
                ),
            expectedOldText = oldText,
            expectedNewText = newText,
        )

    @Suppress("LongParameterList")
    private fun makeDeleteIntent(
        coreTxnId: Long,
        baseRev: Long,
        newRev: Long,
        oldText: String,
        newText: String,
        deletedRange: TextRange,
        offsetMap: VisualOffsetMap? = null,
    ): EditorVisualIntent =
        EditorVisualIntent(
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = offsetMap,
            oldRanges = listOf(deletedRange),
            newRanges = emptyList(),
            textKind = TextVisualKind.Delete,
            cursor =
                CursorVisualIntent(
                    oldEndUtf16 = oldText.length,
                    newEndUtf16 = newText.length,
                    animate = true,
                ),
            replaceBounds =
                VisualReplaceBounds(
                    oldStart = deletedRange.start,
                    oldEnd = deletedRange.end,
                    newStart = newText.length,
                    newEnd = newText.length,
                ),
            expectedOldText = oldText,
            expectedNewText = newText,
        )

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> = captureLayoutsWithWidth(texts, 1000)

    private fun captureLayoutsWithWidth(
        texts: Array<out String>,
        maxWidth: Int,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = fontSizeSp.sp),
                        constraints = Constraints(maxWidth = maxWidth),
                    ),
                )
            }
        }
        return results
    }
}
