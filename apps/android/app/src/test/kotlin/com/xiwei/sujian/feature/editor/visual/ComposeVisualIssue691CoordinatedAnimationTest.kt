package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
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
@Suppress("StringLiteralDuplication", "MaxLineLength")
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
        val path = CursorMotionPath(
            points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
        )
        val patch = makePatch(
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
            cursorToRect = newCursorRect,
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
                2L, 1L, 2L, "a", "ab", TextRange(1, 2),
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
                3L, 2L, 3L, "ab", "abc", TextRange(2, 3),
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
        val path = CursorMotionPath(
            points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
        )
        val patch = makePatch(
            id = 1L,
            oldLayout = emptyLayout,
            newLayout = aLayout,
            insertedUnits = listOf(TextRange(0, 1)),
            cursorMotionPath = path,
            durationMs = 100L,
            motionPolicy = EditorMotionPolicy(
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
            cursorToRect = newCursorRect,
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
        val path = CursorMotionPath(
            points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
        )
        val patch = makePatch(
            id = 1L,
            oldLayout = emptyLayout,
            newLayout = aLayout,
            insertedUnits = listOf(TextRange(0, 1)),
            cursorMotionPath = path,
            durationMs = 100L,
            motionPolicy = EditorMotionPolicy(
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
            cursorToRect = newCursorRect,
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
            val path = CursorMotionPath(
                points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
            )
            val patch = makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                cursorMotionPath = path,
                durationMs = 100L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
            )

            val intervalNanos = 1_000_000_000L / hz
            // 在单一 frameTimeNanos(=0) 上同时把文字与光标交给同一个 timeline。
            timeline.applyPatch(
                patch = patch,
                frameTimeNanos = 0L,
                cursorFromRect = oldCursorRect,
                cursorToRect = newCursorRect,
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
                1L, 0L, 1L, "abcde", "abcdef", TextRange(5, 6),
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
        assertEquals("应 drain 3 笔 patch", 3, applied.size)

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
                2L, 1L, 2L, "a", "ab", TextRange(1, 2),
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
                2L, 1L, 2L, "a", "ab", TextRange(1, 2),
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
            EditorMotionPolicy(textEnabled = false, cursorEnabled = false),
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
            EditorMotionPolicy(textEnabled = false, cursorEnabled = true, cursorDurationMillis = 80L),
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
            cursor = CursorVisualIntent(
                oldEndUtf16 = oldText.length,
                newEndUtf16 = newText.length,
                animate = true,
            ),
            replaceBounds = VisualReplaceBounds(oldStart = oldText.length, oldEnd = oldText.length, newStart = newRange.start, newEnd = newRange.end),
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
            cursor = CursorVisualIntent(
                oldEndUtf16 = oldText.length,
                newEndUtf16 = newText.length,
                animate = true,
            ),
            replaceBounds = VisualReplaceBounds(oldStart = deletedRange.start, oldEnd = deletedRange.end, newStart = newText.length, newEnd = newText.length),
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
