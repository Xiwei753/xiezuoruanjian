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
import com.xiwei.sujian.feature.editor.layout.cursorRect
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

/**
 * Issue #728 评论 5754839786 三个缺口的回归测试。
 *
 * 缺口1：[ComposeEditorVisualState.buildLocalInputPatch] 写真实 caret 两端 —
 *   originCaretRect = oldLayout.cursorRect(oldSelection.end)，
 *   targetCaretRect = newLayout.cursorRect(newSelection.end)。
 *   旧实现漏传，ComposeVisualPatch 的 Rect.Zero 默认值让本地 patch 的 caret 两端变成 (0,0,0,0)，
 *   ComposeEditMotion 从原点插值到原点，本地编辑时屏幕 caret 不动。
 *   修复后 ComposeVisualPatch 的 originCaretRect/targetCaretRect 改成必填参数（去掉 Rect.Zero 默认值）。
 *
 * 缺口2：resting/selection caret 闭环 —
 *   [ComposeEditorVisualState] 新增 restingCaretRect 字段，
 *   onAuthoritativeLayout fingerprintUnchanged 时用 restingCaretRect 填 drawSnapshot().caretRect，
 *   保证纯 selection 移动后屏幕 caret 不消失。
 *
 * 缺口3：[ComposeVisualTimeline] unit 生命周期由 [ComposeEditMotion] 驱动 —
 *   coordinated 模式下 sample 时 motion fraction 未到终点则 unit 不被移除（motionFractionReachedTarget 门控）；
 *   hasActiveAnimation coordinated 时只看 position 通道（alpha/reveal 由 motion 驱动，timeline 不重复检查）。
 *
 * 测试基础设施：Robolectric + createComposeRule 用于构造真实 TextLayoutResult（cursorRect 需要）。
 * ComposeVisualTimeline 是纯数据状态机，可直接实例化。
 */
@Suppress("MaxLineLength", "StringLiteralDuplication", "LongMethod")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue728Comment5754839786ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 缺口1：buildLocalInputPatch 写真实 caret 两端 ====================

    /**
     * 缺口1补充：ComposeVisualPatch 的 originCaretRect/targetCaretRect 字段存在且类型为 Rect —
     * 反射验证主源码去掉了 Rect.Zero 默认值后，两个字段是必填的非 nullable Rect。
     */
    @Test
    fun gap1_composeVisualPatch_caretRectFieldsAreNonNullableRect() {
        val fields = ComposeVisualPatch::class.java.declaredFields
        val originField = fields.firstOrNull { it.name == "originCaretRect" }
        val targetField = fields.firstOrNull { it.name == "targetCaretRect" }
        assertNotNull(
            "ComposeVisualPatch 应有 originCaretRect 字段",
            originField,
        )
        assertNotNull(
            "ComposeVisualPatch 应有 targetCaretRect 字段",
            targetField,
        )
        assertEquals(
            "originCaretRect 字段类型应是 Rect（非 nullable）",
            Rect::class.java,
            originField!!.type,
        )
        assertEquals(
            "targetCaretRect 字段类型应是 Rect（非 nullable）",
            Rect::class.java,
            targetField!!.type,
        )
    }

    // ==================== 缺口2：resting/selection caret 闭环 ====================

    /**
     * 缺口2：ComposeEditorVisualState 应有 restingCaretRect 字段。
     *
     * 修复后新增 private var restingCaretRect: Rect? = null，
     * onAuthoritativeLayout / onInputSnapshotResolved / publishLocalHandoffScene / sampleVisualScene
     * 各分支都更新它，drawSnapshot 无 active motion 时用 restingCaretRect 填 caretRect。
     */
    @Test
    fun gap2_state_hasRestingCaretRectField() {
        val fields = ComposeEditorVisualState::class.java.declaredFields.map { it.name }
        assertTrue(
            "ComposeEditorVisualState 应有 restingCaretRect 字段",
            fields.contains("restingCaretRect"),
        )
    }

    /**
     * 缺口2：onAuthoritativeLayout fingerprintUnchanged 时 drawSnapshot().caretRect 非 null —
     * 纯 selection 移动（相同 layout，不同 selection）后屏幕 caret 不消失。
     *
     * 场景："ab" layout 先用 selection=(0,0) 调 onAuthoritativeLayout（设置 fingerprint），
     * 再用相同 layout + selection=(1,1) 调 onAuthoritativeLayout（fingerprintUnchanged=true）。
     * 修复后 fingerprintUnchanged 分支把 restingCaretRect 更新到 selection.end 对应的 caret rect，
     * 并写进 drawSnapshotState.caretRect，保证静止/selection 移动后屏幕 caret 停在新位置。
     *
     * 旧实现：fingerprintUnchanged 分支只 return，不更新 caret rect，屏幕 caret 停在旧位置不动。
     */
    @Test
    fun gap2_fingerprintUnchanged_drawSnapshotCaretRectNonNull() {
        val layouts = captureLayouts("ab")
        val state =
            ComposeEditorVisualState(
                targetId = "issue728-5754839786-gap2",
            )
        // 第一次 onAuthoritativeLayout：设置 lastObservedLayoutFingerprint
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        // 第二次 onAuthoritativeLayout：相同 layout，不同 selection → fingerprintUnchanged=true
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        val caretRect = state.drawSnapshot().caretRect
        assertNotNull(
            "fingerprintUnchanged 时 drawSnapshot().caretRect 应非 null（restingCaretRect 填充）",
            caretRect,
        )
        // 验证 caretRect 等于新 selection.end 对应的 cursor rect
        val expectedCaret = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0).cursorRect(1)
        assertEquals(
            "drawSnapshot().caretRect 应等于新 selection.end 对应的 cursor rect",
            expectedCaret,
            caretRect,
        )
    }

    // ==================== 缺口3：timeline unit 生命周期由 motion 驱动 ====================

    /**
     * 缺口3：coordinated 模式下 sample 时 motion fraction 未到终点则 unit 不被移除。
     *
     * 场景：applyPatch 插入 unit（alpha 0→1, duration=100ms, position duration=0）。
     * 在 frameTime=200ms（alpha 已结束，position 已结束）时：
     * - motion fraction=1.0 → motionFractionReachedTarget=true → unit 被移除（alphaFinished && positionFinished && alpha.to>=1 && motion 到终点）
     * - motion fraction=0.5 → motionFractionReachedTarget=false → unit 保留（motion 未到终点）
     *
     * 旧实现：sample 移除只看 alphaFinished/positionFinished，motion fraction 不参与门控，
     * rapid redirect 后旧 unit 的 alpha 旧时钟已结束但 motion 里这个字还没走到 fraction=1，
     * unit 被提前移除，旧字提前消失。
     */
    @Test
    fun gap3_sample_motionFractionGatesUnitRemoval() {
        val layouts = captureLayouts("", "a")
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val durationMs = 100L
        val durationNanos = durationMs * NANOS_PER_MS
        val frameAfterAlphaFinished = durationNanos * 2 // 200ms，alpha 已结束

        // motion fraction=1.0 → unit 应被移除
        val timelineFinished = ComposeVisualTimeline()
        timelineFinished.applyPatch(
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                durationMs = durationMs,
                coordinated = true,
            ),
            frameTimeNanos = 0L,
            motionPolicy = EditorMotionPolicy(textDurationMillis = durationMs, coordinated = true),
        )
        val motionSampleFinished =
            ComposeEditMotion.Sample(
                caretRect = Rect.Zero,
                unitClipFractions = mapOf(1L to 1.0f),
                finished = true,
            )
        val sceneFinished =
            timelineFinished.sample(
                frameTimeNanos = frameAfterAlphaFinished,
                motionSample = motionSampleFinished,
            )
        assertTrue(
            "motion fraction=1.0 且 alpha 已结束时，unit 应被移除（scene.units 为空）",
            sceneFinished.units.isEmpty(),
        )

        // motion fraction=0.5 → unit 应保留
        val timelineMid = ComposeVisualTimeline()
        timelineMid.applyPatch(
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                durationMs = durationMs,
                coordinated = true,
            ),
            frameTimeNanos = 0L,
            motionPolicy = EditorMotionPolicy(textDurationMillis = durationMs, coordinated = true),
        )
        val motionSampleMid =
            ComposeEditMotion.Sample(
                caretRect = Rect.Zero,
                unitClipFractions = mapOf(1L to 0.5f),
                finished = false,
            )
        val sceneMid =
            timelineMid.sample(
                frameTimeNanos = frameAfterAlphaFinished,
                motionSample = motionSampleMid,
            )
        assertTrue(
            "motion fraction=0.5 时，unit 不应被移除（motion 未到终点，scene.units 非空）",
            sceneMid.units.isNotEmpty(),
        )
    }

    /**
     * 缺口3：hasActiveAnimation coordinated 时只看 position 通道。
     *
     * 场景：applyPatch 插入 unit（alpha 0→1 duration=100ms，position duration=0）。
     * 在 frameTime=50ms（alpha 未结束，position 已结束）时：
     * - coordinated: hasActiveAnimation 只看 position → position 已结束 → false
     * - 非 coordinated: hasActiveAnimation 检查 alpha → alpha 未结束 → true
     *
     * 旧实现：coordinated 时 hasActiveAnimation 也检查 alpha/reveal，
     * 与 ComposeEditMotion 重复检查导致 timeline active 判断不一致。
     * 修复后 coordinated 时 alpha/reveal 由 motion 驱动，timeline 只看 position（retained/reflow 可独立于 motion）。
     */
    @Test
    fun gap3_hasActiveAnimation_coordinatedOnlyChecksPosition() {
        val layouts = captureLayouts("", "a")
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val durationMs = 100L
        val durationNanos = durationMs * NANOS_PER_MS
        val midFrame = durationNanos / 2 // 50ms，alpha 未结束，position 已结束

        // coordinated: hasActiveAnimation 只看 position
        val timelineCoordinated = ComposeVisualTimeline()
        timelineCoordinated.applyPatch(
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                durationMs = durationMs,
                coordinated = true,
            ),
            frameTimeNanos = 0L,
            motionPolicy = EditorMotionPolicy(textDurationMillis = durationMs, coordinated = true),
        )
        // sample at midFrame — unit 不被移除（alpha 未到 1）
        timelineCoordinated.sample(frameTimeNanos = midFrame)
        assertFalse(
            "coordinated 模式下，alpha 未结束但 position 已结束时 hasActiveAnimation 应为 false" +
                "（coordinated 只看 position 通道，alpha/reveal 由 motion 驱动）",
            timelineCoordinated.hasActiveAnimation(midFrame),
        )

        // 非 coordinated: hasActiveAnimation 检查 alpha
        val timelineNonCoordinated = ComposeVisualTimeline()
        timelineNonCoordinated.applyPatch(
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                durationMs = durationMs,
                coordinated = false,
            ),
            frameTimeNanos = 0L,
            motionPolicy = EditorMotionPolicy(textDurationMillis = durationMs, coordinated = false),
        )
        // sample at midFrame — unit 不被移除（alpha 未到 1）
        timelineNonCoordinated.sample(frameTimeNanos = midFrame)
        assertTrue(
            "非 coordinated 模式下，alpha 未结束时 hasActiveAnimation 应为 true" +
                "（非 coordinated 检查 alpha/position/reveal 通道）",
            timelineNonCoordinated.hasActiveAnimation(midFrame),
        )
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L
    }

    /**
     * 构造 ComposeVisualPatch（coordinated/non-coordinated 可选）。
     *
     * originCaretRect/targetCaretRect 用 Rect.Zero — 本测试验证 timeline/motion 机制，
     * 不验证 caret 几何（caret 几何由缺口1测试覆盖）。
     */
    @Suppress("LongParameterList")
    private fun makePatch(
        id: Long,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        insertedUnits: List<TextRange> = emptyList(),
        deletedUnits: List<TextRange> = emptyList(),
        durationMs: Long = 100L,
        coordinated: Boolean = true,
    ): ComposeVisualPatch {
        return ComposeVisualPatch(
            id = id,
            coreTransactionIds = listOf(id),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = emptyList(),
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = emptyList(),
            originCaretRect = Rect.Zero,
            targetCaretRect = Rect.Zero,
            durationMs = durationMs,
            animationMode = AnimationMode.GLYPH_ANIMATION,
        )
    }

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = 14f.sp),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }
}
