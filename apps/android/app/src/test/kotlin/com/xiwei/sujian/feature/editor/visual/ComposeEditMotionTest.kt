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
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #737：[CoordinatedEditMotion] 单元测试 —
 * 验证一笔协调 motion 的 sample、isFinished 语义。
 *
 * Issue #737 重写：旧 [ComposeEditMotion]（forInsert/forDelete/forEdit/redirectTo/redirectCaretTo）
 * 已删除，本测试重写为验证 [CoordinatedEditMotion] 的等价行为：
 * - 直接构造 [CoordinatedEditMotion]（caret + glyph channels 共用同一 progress）
 * - [sample] 同时产出 caret rect 和 glyph overlays
 * - 一笔编辑只有一个 motion，不再有 redirectTo/redirectCaretTo 分离 redirect 逻辑
 *
 * Issue #728 评论 5755928697：三个确定问题的收口测试 —
 * 1. ~~redirectCaretTo 保留现有 glyph channel fraction~~ —
 *    Issue #737：redirectCaretTo 已删除（一笔编辑只有一个 motion，不再有分离 redirect）。
 *    等价覆盖：[CoordinatedEditMotion] 一笔内 caret 和 glyph 共用同一 progress，
 *    不存在"redirect 丢 channel"的问题。
 * 3. 多 unit 区间映射，光标跟着吞吐。
 *
 * Issue #735 评论 5773604666 问题2：删除双 duration —
 * caret 和 glyph 共用同一个 durationNanos/progress，不再有独立的 caret/glyph 时长。
 * 删除 `sample_coordinatedFalse_caretAndGlyphIndependent`、
 * `sample_caretInstantGlyphRunning_notFinished`、`sample_glyphInstantCaretRunning_notFinished`、
 * `isFinished_requiresBothCaretAndGlyphFinished` 等依赖双 duration 的测试。
 *
 * 一笔编辑一只钟：caret 移动和文字吞吐共用同一个 progress。
 */
@Suppress("TooManyFunctions")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeEditMotionTest {
    @get:Rule
    val composeRule = createComposeRule()

    private val originRect = Rect(left = 10f, top = 0f, right = 12f, bottom = 20f)
    private val targetRect = Rect(left = 30f, top = 0f, right = 32f, bottom = 20f)
    private val startTime = 1_000_000_000L
    private val duration = 100_000_000L // 100ms in nanos

    private lateinit var dummyLayout: ComposeLayoutSnapshot

    @Before
    fun setUp() {
        dummyLayout = ComposeLayoutSnapshot(measureText("a"), TextRange(0, 1), 0)
    }

    /**
     * Issue #737：inserted glyph 从 0→1 吐字。
     *
     * 构造 2 个 inserted channel（区间 [0, 0.5]，按 fromPatch 分配规则），
     * 验证 sample 在起点 fraction=0、终点 fraction=1。
     */
    @Test
    fun forInsert_createsChannelsFromZeroToOne() {
        val motion =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L, 2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // At start (progress=0): fraction should be 0 (invisible)
        val startSample = motion.sample(startTime)
        assertEquals(0f, fractionFor(startSample, 1L), 0.001f)
        assertEquals(0f, fractionFor(startSample, 2L), 0.001f)
        assertFalse(startSample.finished)

        // At end (progress=1): fraction should be 1 (fully visible)
        val endSample = motion.sample(startTime + duration)
        assertEquals(1f, fractionFor(endSample, 1L), 0.001f)
        assertEquals(1f, fractionFor(endSample, 2L), 0.001f)
        assertTrue(endSample.finished)
    }

    /**
     * Issue #737：deleted glyph 从 1→0 吞字。
     */
    @Test
    fun forDelete_createsChannelsFromOneToZero() {
        val motion =
            makeDeleteMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // At start (progress=0): fraction should be 1 (fully visible)
        val startSample = motion.sample(startTime)
        assertEquals(1f, fractionFor(startSample, 1L), 0.001f)

        // At end (progress=1): fraction should be 0 (invisible)
        val endSample = motion.sample(startTime + duration)
        assertEquals(0f, fractionFor(endSample, 1L), 0.001f)
        assertTrue(endSample.finished)
    }

    /**
     * Issue #728 评论 5755928697 问题3：混合 edit 时 inserted 占前半段 [0, 0.5]，
     * deleted 占后半段 [0.5, 1]。光标先经过插入区吐字，再经过删除区吞字。
     */
    @Test
    fun forEdit_handlesBothInsertedAndDeletedUnits() {
        val motion =
            makeEditMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L),
                deletedKeys = listOf(2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // progress=0.25：inserted key=1 在区间 [0, 0.5] 中点，local=0.5，fraction=0.5
        val quarterSample = motion.sample(startTime + duration / 4)
        assertEquals(0.5f, fractionFor(quarterSample, 1L), 0.001f)
        // deleted key=2 还没开始它的区间 [0.5, 1]，fraction=1（完全可见）
        assertEquals(1f, fractionFor(quarterSample, 2L), 0.001f)

        // progress=0.5：inserted key=1 已走完 [0, 0.5]，fraction=1（完全吐出）
        val midSample = motion.sample(startTime + duration / 2)
        assertEquals(1f, fractionFor(midSample, 1L), 0.001f)
        // deleted key=2 刚到区间起点 [0.5, 1]，还没开始吞，fraction=1
        assertEquals(1f, fractionFor(midSample, 2L), 0.001f)

        // progress=0.75：inserted key=1 已走完，fraction=1
        val threeQuarterSample = motion.sample(startTime + 3 * duration / 4)
        assertEquals(1f, fractionFor(threeQuarterSample, 1L), 0.001f)
        // deleted key=2 在区间 [0.5, 1] 中点，local=0.5，fraction=1+(0-1)*0.5=0.5
        assertEquals(0.5f, fractionFor(threeQuarterSample, 2L), 0.001f)
    }

    /**
     * Issue #728 评论 5755928697 问题3：多个 inserted unit 依次吐字。
     *
     * Issue #737：纯 inserted 时 fromPatch 占全区间 [0, 1]，n=2 时
     * key=1 区间 [0, 0.5]，key=2 区间 [0.5, 1]。
     */
    @Test
    fun forInsert_multipleUnitsRevealSequentially() {
        val motion =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L, 2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // progress=0.25：key=1 在 [0, 0.5] 中点 local=0.5 fraction=0.5；
        // key=2 还没开始 [0.5, 1] fraction=0
        val quarterSample = motion.sample(startTime + duration / 4)
        assertEquals(0.5f, fractionFor(quarterSample, 1L), 0.001f)
        assertEquals(0f, fractionFor(quarterSample, 2L), 0.001f)

        // progress=0.5：key=1 已走完 fraction=1；key=2 刚开始 fraction=0
        val halfSample = motion.sample(startTime + duration / 2)
        assertEquals(1f, fractionFor(halfSample, 1L), 0.001f)
        assertEquals(0f, fractionFor(halfSample, 2L), 0.001f)

        // progress=0.75：key=1 已走完 fraction=1；key=2 在 [0.5, 1] 中点 fraction=0.5
        val threeQuarterSample = motion.sample(startTime + 3 * duration / 4)
        assertEquals(1f, fractionFor(threeQuarterSample, 1L), 0.001f)
        assertEquals(0.5f, fractionFor(threeQuarterSample, 2L), 0.001f)
    }

    /**
     * Issue #728 评论 5755928697 问题3：多个 deleted unit 反向吞字（先吞最右边）。
     *
     * Issue #737：纯 deleted 时 fromPatch 占全区间 [0, 1]，n=2 反序分配
     * key=1 区间 [0.5, 1]，key=2 区间 [0, 0.5]。key=2 先吞（光标从右往左）。
     */
    @Test
    fun forDelete_multipleUnitsConcealReversed() {
        val motion =
            makeDeleteMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedKeys = listOf(1L, 2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // progress=0.25：key=2 在 [0, 0.5] 中点 local=0.5 fraction=1+(0-1)*0.5=0.5；
        // key=1 还没开始 [0.5, 1] fraction=1
        val quarterSample = motion.sample(startTime + duration / 4)
        assertEquals(1f, fractionFor(quarterSample, 1L), 0.001f)
        assertEquals(0.5f, fractionFor(quarterSample, 2L), 0.001f)

        // progress=0.5：key=2 已走完 fraction=0；key=1 刚开始 fraction=1
        val halfSample = motion.sample(startTime + duration / 2)
        assertEquals(1f, fractionFor(halfSample, 1L), 0.001f)
        assertEquals(0f, fractionFor(halfSample, 2L), 0.001f)

        // progress=0.75：key=2 已走完 fraction=0；key=1 在 [0.5, 1] 中点 fraction=0.5
        val threeQuarterSample = motion.sample(startTime + 3 * duration / 4)
        assertEquals(0.5f, fractionFor(threeQuarterSample, 1L), 0.001f)
        assertEquals(0f, fractionFor(threeQuarterSample, 2L), 0.001f)
    }

    /**
     * Issue #728 评论 5755928697 问题3：单个 unit 退化为全区间。
     */
    @Test
    fun forInsert_singleUnitUsesFullRange() {
        val motion =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // mid progress → fraction=0.5（退化为原行为）
        val midSample = motion.sample(startTime + duration / 2)
        assertEquals(0.5f, fractionFor(midSample, 1L), 0.001f)
    }

    @Test
    fun sample_caretRectInterpolatesFromOriginToTarget() {
        val motion =
            CoordinatedEditMotion.forSelectionMove(
                oldLayout = dummyLayout,
                newLayout = dummyLayout,
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                originCaretOffset = 0,
                targetCaretOffset = 1,
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // At start: caret at origin
        val startSample = motion.sample(startTime)
        assertEquals(originRect, startSample.caretRect)

        // At end: caret at target
        val endSample = motion.sample(startTime + duration)
        assertEquals(targetRect, endSample.caretRect)

        // At mid: caret at midpoint
        val midSample = motion.sample(startTime + duration / 2)
        assertEquals(20f, midSample.caretRect.left, 0.001f) // (10+30)/2
    }

    @Test
    fun sample_instantDurationCompletesImmediately() {
        val motion =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = 0L,
            )
        val sample = motion.sample(startTime)
        assertTrue(sample.finished)
        assertEquals(targetRect, sample.caretRect)
        assertEquals(1f, fractionFor(sample, 1L), 0.001f)
    }

    @Test
    fun isFinished_matchesSampleFinishedSemantics() {
        val motion =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        assertFalse(motion.isFinished(startTime))
        assertFalse(motion.isFinished(startTime + duration / 2))
        assertTrue(motion.isFinished(startTime + duration))
    }

    @Test
    fun isFinished_instantDurationAlwaysTrue() {
        val motion =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = 0L,
            )
        assertTrue(motion.isFinished(startTime))
    }

    /**
     * 以下旧 API 测试已删除（Issue #737 重写）：
     *
     * - ~~redirectTo_existingUnitContinuesFromCurrentFraction~~ —
     *   旧 [ComposeEditMotion.redirectTo] 已删除。一笔编辑只有一个 motion，
     *   不再有"redirect 保留 channel fraction"的分离逻辑。
     *   等价覆盖：[CoordinatedEditMotion] 一笔内 caret 和 glyph 共用同一 progress，
     *   不存在"redirect 丢 channel"的问题。
     * - ~~redirectTo_newUnitStartsFromZeroOrOne~~ —
     *   旧 [ComposeEditMotion.redirectTo] 已删除。见 [redirectTo_existingUnitContinuesFromCurrentFraction] 注释。
     * - ~~redirectTo_finishedMotionUsesNewOrigin~~ —
     *   旧 [ComposeEditMotion.redirectTo] 已删除。见 [redirectTo_existingUnitContinuesFromCurrentFraction] 注释。
     * - ~~redirectCaretTo_preservesExistingUnitFractions~~ —
     *   旧 [ComposeEditMotion.redirectCaretTo] 已删除。一笔编辑只有一个 motion，
     *   不再有"redirect caret 保留 channel fraction"的分离逻辑。
     *   等价覆盖：[CoordinatedEditMotion] 一笔内 caret 和 glyph 共用同一 progress。
     * - ~~redirectCaretTo_finishedMotionUsesNewOrigin~~ —
     *   旧 [ComposeEditMotion.redirectCaretTo] 已删除。见上注释。
     * - ~~redirectCaretTo_caretMovesFromCurrentToNewTarget~~ —
     *   旧 [ComposeEditMotion.redirectCaretTo] 已删除。见上注释。
     */

    @Test
    fun forSelectionMove_hasNoUnitChannels() {
        val motion =
            CoordinatedEditMotion.forSelectionMove(
                oldLayout = dummyLayout,
                newLayout = dummyLayout,
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                originCaretOffset = 0,
                targetCaretOffset = 1,
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        val sample = motion.sample(startTime + duration / 2)
        assertTrue(sample.glyphOverlays.isEmpty())
        // Caret still interpolates
        assertEquals(20f, sample.caretRect.left, 0.001f)
    }

    @Test
    fun sample_beforeStartTimeReturnsOriginFractions() {
        val motion =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // Sample before start time — should return origin values
        val beforeSample = motion.sample(startTime - 1000L)
        assertEquals(originRect, beforeSample.caretRect)
        assertEquals(0f, fractionFor(beforeSample, 1L), 0.001f)
        assertFalse(beforeSample.finished)
    }

    // ==================== 辅助方法 ====================

    private fun measureText(text: String): TextLayoutResult {
        lateinit var result: TextLayoutResult
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            result =
                textMeasurer.measure(
                    text = AnnotatedString(text),
                    style = TextStyle(fontSize = 14f.sp),
                    constraints = Constraints(maxWidth = 1000),
                )
        }
        return result
    }

    /**
     * 从 [CoordinatedEditMotion.Sample.glyphOverlays] 按 key 查找 clipFraction。
     *
     * Issue #737：旧 [ComposeEditMotion.Sample.unitClipFractions] 已删除，
     * 改用 [CoordinatedEditMotion.Sample.glyphOverlays]（fraction > 0 的 glyph 才进 overlays）。
     * fraction == 0 的 glyph 不在 overlays 里，返回 0f。
     */
    private fun fractionFor(
        sample: CoordinatedEditMotion.Sample,
        key: Long,
    ): Float = sample.glyphOverlays.firstOrNull { it.key == key }?.clipFraction ?: 0f

    /**
     * 构造只有 inserted channel 的 [CoordinatedEditMotion] —
     * 模拟 [CoordinatedEditMotion.fromPatch] 只有 inserted 时的区间分配：
     * inserted 占前半段 [0, 0.5]，n 个 unit 按 [0, 0.5/n], [0.5/n, 2*0.5/n], ... 分配。
     */
    private fun makeInsertMotion(
        originCaretRect: Rect,
        targetCaretRect: Rect,
        insertedKeys: List<Long>,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): CoordinatedEditMotion =
        makeMotion(
            originCaretRect = originCaretRect,
            targetCaretRect = targetRect,
            insertedKeys = insertedKeys,
            deletedKeys = emptyList(),
            frameTimeNanos = frameTimeNanos,
            durationNanos = durationNanos,
        )

    /**
     * 构造只有 deleted channel 的 [CoordinatedEditMotion] —
     * 模拟 [CoordinatedEditMotion.fromPatch] 只有 deleted 时的区间分配：
     * deleted 占后半段 [0.5, 1]，n 个 unit 反序分配（右往左吞）。
     */
    private fun makeDeleteMotion(
        originCaretRect: Rect,
        targetCaretRect: Rect,
        deletedKeys: List<Long>,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): CoordinatedEditMotion =
        makeMotion(
            originCaretRect = originCaretRect,
            targetCaretRect = targetRect,
            insertedKeys = emptyList(),
            deletedKeys = deletedKeys,
            frameTimeNanos = frameTimeNanos,
            durationNanos = durationNanos,
        )

    /**
     * 构造混合 inserted + deleted channel 的 [CoordinatedEditMotion] —
     * 模拟 [CoordinatedEditMotion.fromPatch] 混合编辑时的区间分配：
     * inserted 占前半段 [0, 0.5]，deleted 占后半段 [0.5, 1]。
     */
    private fun makeEditMotion(
        originCaretRect: Rect,
        targetCaretRect: Rect,
        insertedKeys: List<Long>,
        deletedKeys: List<Long>,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): CoordinatedEditMotion =
        makeMotion(
            originCaretRect = originCaretRect,
            targetCaretRect = targetRect,
            insertedKeys = insertedKeys,
            deletedKeys = deletedKeys,
            frameTimeNanos = frameTimeNanos,
            durationNanos = durationNanos,
        )

    /**
     * 构造 [CoordinatedEditMotion] — 复用 [CoordinatedEditMotion.fromPatch] 的区间分配逻辑，
     * 但自己指定 key（fromPatch 内部用 nextGlyphKey++ 自动分配，key 不可控）。
     */
    @Suppress("LongParameterList")
    private fun makeMotion(
        originCaretRect: Rect,
        targetCaretRect: Rect,
        insertedKeys: List<Long>,
        deletedKeys: List<Long>,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): CoordinatedEditMotion {
        val traversal =
            CaretTraversal(
                segments =
                    listOf(
                        CaretTraversal.Segment(
                            startRect = originCaretRect,
                            endRect = targetCaretRect,
                            lineIndex = 0,
                            startProgress = 0f,
                            endProgress = 1f,
                        ),
                    ),
                isValid = true,
                oldLine = 0,
                newLine = 0,
            )
        val channels = mutableMapOf<Long, CoordinatedEditMotion.GlyphChannel>()
        val insertedCount = insertedKeys.size
        val deletedCount = deletedKeys.size
        val hasBoth = insertedCount > 0 && deletedCount > 0
        // inserted 区间：混合时占 [0, 0.5]，纯 inserted 时占 [0, 1]
        val insertedSpan = if (hasBoth) 0.5f else 1f
        for (i in insertedKeys.indices) {
            val key = insertedKeys[i]
            val start = insertedSpan * i.toFloat() / insertedCount.toFloat()
            val end = insertedSpan * (i + 1).toFloat() / insertedCount.toFloat()
            channels[key] =
                CoordinatedEditMotion.GlyphChannel(
                    key = key,
                    range = TextRange(i, i + 1),
                    layout = dummyLayout,
                    role = CoordinatedEditMotion.GlyphRole.Inserted,
                    fromFraction = 0f,
                    toFraction = 1f,
                    startProgress = start,
                    endProgress = end,
                )
        }
        // deleted 区间：混合时占 [0.5, 1]，纯 deleted 时占 [0, 1]，反序（右往左吞）
        val deletedOffset = if (hasBoth) 0.5f else 0f
        val deletedSpan = if (hasBoth) 0.5f else 1f
        for (i in deletedKeys.indices) {
            val key = deletedKeys[i]
            val start = deletedOffset + deletedSpan * (deletedCount - 1 - i).toFloat() / deletedCount.toFloat()
            val end = deletedOffset + deletedSpan * (deletedCount - i).toFloat() / deletedCount.toFloat()
            channels[key] =
                CoordinatedEditMotion.GlyphChannel(
                    key = key,
                    range = TextRange(i, i + 1),
                    layout = dummyLayout,
                    role = CoordinatedEditMotion.GlyphRole.Deleted,
                    fromFraction = 1f,
                    toFraction = 0f,
                    startProgress = start,
                    endProgress = end,
                )
        }
        return CoordinatedEditMotion(
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(0, insertedCount),
            oldCaretRect = originCaretRect,
            newCaretRect = targetCaretRect,
            oldLine = -1,
            newLine = -1,
            traversal = traversal,
            glyphChannels = channels,
            startedAtNanos = frameTimeNanos,
            durationNanos = durationNanos,
        )
    }
}
