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
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #728 评论 5760112985：~~[ComposeEditMotion.redirectTo] 角色目标与统一 schedule~~ 专项测试 —
 *
 * Issue #737 重写：旧 [ComposeEditMotion]（redirectTo/redirectCaretTo）已删除。
 * 一笔编辑只有一个 [CoordinatedEditMotion]，不再有"先跑 motion1 再 redirect 成 motion2"的分离逻辑。
 *
 * 本测试重写为验证 [CoordinatedEditMotion] 在单笔 motion 内的等价行为：
 * 1. Deleted glyph 从 1→0 吞字（等价旧"inserted 变 deleted ghost 后目标 0"）。
 * 2. 新 motion 从 fromFraction 开始走完整区间（等价旧"completed inserted 变 deleted 重新动画"）。
 * 3. 多个 deleted unit 反序分配区间，右往左吞（等价旧"redirect 保持右往左顺序"）。
 * 4. 混合 edit inserted 前半段 + deleted 后半段，不重叠（等价旧"redirect 单 schedule 不重叠"）。
 * 5. [CaretTraversal] segment 顺序决定光标遍历顺序（等价旧"redirectCaretTo 保持 traversal 顺序"）。
 *
 * 从 [ComposeEditMotionTest] 拆出，避免触发 detekt LargeClass（阈值 500 行）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeEditMotionRedirectRoleTest {
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
     * Issue #728 评论 5760112985 问题1：~~Inserted→DeletedGhost 角色变化时目标 0~~ —
     * 旧 [ComposeEditMotion.redirectTo] 已删除。一笔编辑只有一个 [CoordinatedEditMotion]，
     * glyph 角色在构造时确定，不存在"运行中角色反转"。
     *
     * 等价覆盖：[CoordinatedEditMotion] 的 Deleted glyph 从 fromFraction=1 走到 toFraction=0，
     * 验证吞字方向正确（fraction 下降），且 glyph 与 caret 同步到达终点。
     *
     * Issue #728 评论 5760741452 问题1 增强：glyph 与 caret 必须**同时**到达终点，
     * 不能 glyph 提前吞完。纯 deleted 单 unit 占全区间 [0, 1]（fromPatch 规则），
     * 80% 时 fraction = 0.2（1+(0-1)*0.8），100% 时 fraction=0；
     * caret 也在 100% 时到目标。
     */
    @Test
    fun deletedGlyph_targetsZero_glyphAndCaretSynced() {
        val motion =
            makeDeleteMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )

        // 起点 fraction=1（完全可见，progress=0 还没开始吞）
        val startSample = motion.sample(startTime)
        assertEquals(1f, fractionFor(startSample, 1L), 0.001f)

        // 80% 时 progress=0.8，纯 deleted 区间 [0, 1]，local=0.8，
        // fraction=1+(0-1)*0.8=0.2，glyph 不应提前吞完
        val atEightyPercent = motion.sample(startTime + duration * 4 / 5)
        val fractionAtEighty = fractionFor(atEightyPercent, 1L)
        assertEquals(
            "80% 时 fraction 应≈0.2（1+(0-1)*0.8），glyph 不应提前吞完，实际=$fractionAtEighty",
            0.2f,
            fractionAtEighty,
            0.001f,
        )
        // 80% 时 caret 还在走：origin=10，target=30，80% 插值=26
        val caretAtEighty = atEightyPercent.caretRect
        assertEquals(
            "caret 80% 时应在 26（还没到目标 30），与 glyph 同步",
            26f,
            caretAtEighty.left,
            0.001f,
        )
        // glyph 还没到终点（fraction=0.4 > 0），caret 还没到终点（left=26 < 30）— 同步
        assertTrue("glyph 80% 时还没到终点（fraction=$fractionAtEighty > 0）", fractionAtEighty > 0.001f)
        assertTrue("caret 80% 时还没到终点（left=${caretAtEighty.left} < 30）", caretAtEighty.left < 29.999f)

        // 100% 时 glyph 和 caret 同时到达终点
        val atFull = motion.sample(startTime + duration)
        assertEquals(
            "100% 时 glyph fraction 应为 0（终点）",
            0f,
            fractionFor(atFull, 1L),
            0.001f,
        )
        assertEquals(
            "100% 时 caret 应到目标（left=30），与 glyph 同时到达",
            30f,
            atFull.caretRect.left,
            0.001f,
        )
    }

    /**
     * Issue #728 评论 5760112985 问题1：~~Completed inserted→deleted 重新动画~~ —
     * 旧 [ComposeEditMotion.redirectTo] 已删除。一笔编辑只有一个 [CoordinatedEditMotion]。
     *
     * 等价覆盖：新 [CoordinatedEditMotion] 从 fromFraction=1 开始走全区间 [0, 1] 到 toFraction=0，
     * 验证新 motion 不沿用任何旧状态，从起点重新动画。
     */
    @Test
    fun newDeleteMotion_startsFreshFromOne_animatesToZero() {
        // 新 delete motion 从 fromFraction=1 开始
        val motion =
            makeDeleteMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )

        // 起点 fraction=1.0（从 fromFraction 开始，不沿用任何旧状态）
        val startSample = motion.sample(startTime)
        assertEquals(1f, fractionFor(startSample, 1L), 0.001f)

        // 75% 时间：progress=0.75，纯 deleted 区间 [0, 1]，local=0.75，
        // fraction=1+(0-1)*0.75=0.25（正在吞，不是固定在 1）
        val afterThreeQuarter = motion.sample(startTime + 3 * duration / 4)
        val fractionThreeQuarter = fractionFor(afterThreeQuarter, 1L)
        assertTrue("新 motion 应正在动画，fraction 应下降，实际=$fractionThreeQuarter", fractionThreeQuarter < 1f)
        assertEquals(0.25f, fractionThreeQuarter, 0.001f)

        // 终点 fraction=0（从 1 走到 0）
        val afterFull = motion.sample(startTime + duration)
        assertEquals(0f, fractionFor(afterFull, 1L), 0.001f)
    }

    /**
     * Issue #728 评论 5760112985 问题2：deleted 保持右往左吞顺序。
     *
     * Issue #737：纯 deleted 时 [CoordinatedEditMotion.fromPatch] 占全区间 [0, 1]，
     * n 个 deleted unit 反序分配区间（右往左吞）。三个 deleted unit keys=10,11,12，
     * 最右边的 key=12 应先开始吞，最左边的 key=10 应最后。
     *
     * 区间分配：key=12 [0, 1/3]（最右先吞），key=11 [1/3, 2/3]，key=10 [2/3, 1]（最左后吞）。
     */
    @Test
    fun deletedUnits_keepRightToLeftOrder() {
        val motion =
            makeDeleteMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedKeys = listOf(10L, 11L, 12L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // progress=1/6：key=12 区间 [0, 1/3] 中点 local=0.5 fraction=0.5（正在吞）；
        // key=10/11 还没开始 fraction=1
        val midSample = motion.sample(startTime + duration / 6)
        assertEquals(0.5f, fractionFor(midSample, 12L), 0.001f)
        assertEquals(1f, fractionFor(midSample, 11L), 0.001f)
        assertEquals(1f, fractionFor(midSample, 10L), 0.001f)

        // progress=1/2：key=12 已完成 fraction=0，key=11 区间 [1/3, 2/3] 中点 fraction=0.5，
        // key=10 还没开始 fraction=1
        val halfSample = motion.sample(startTime + duration / 2)
        assertEquals(0f, fractionFor(halfSample, 12L), 0.001f)
        assertEquals(0.5f, fractionFor(halfSample, 11L), 0.001f)
        assertEquals(1f, fractionFor(halfSample, 10L), 0.001f)

        // key=12 比 key=10 先吞完 — 右→左顺序保持
        assertTrue(
            "key=12 应比 key=10 先吞（fraction 更小）",
            fractionFor(halfSample, 12L) < fractionFor(halfSample, 10L),
        )
    }

    /**
     * Issue #728 评论 5760112985 问题2：mixed edit inserted/deleted 在一条 schedule 上，
     * 不各自从 0 同时开始（不重叠）。inserted key=1 在前半段 [0, 0.5] 先动，
     * deleted key=2 在后半段 [0.5, 1] 后动。
     */
    @Test
    fun mixedEdit_singleScheduleNotOverlapping() {
        val motion =
            makeEditMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L),
                deletedKeys = listOf(2L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )

        // 25% 时间：key=1 在 [0, 0.5] 中点 fraction=0.5（正在吐），
        // key=2 还没开始 [0.5, 1] fraction=1
        val quarterSample = motion.sample(startTime + duration / 4)
        val fraction1Quarter = fractionFor(quarterSample, 1L)
        val fraction2Quarter = fractionFor(quarterSample, 2L)
        assertEquals("key=1 应在 [0,0.5] 中点 fraction=0.5", 0.5f, fraction1Quarter, 0.001f)
        assertEquals("key=2 应还没开始吞（fraction=1）", 1f, fraction2Quarter, 0.001f)

        // 75% 时间：key=1 已完成 fraction=1，key=2 在 [0.5, 1] 中点 fraction=0.5（正在吞）
        val threeQuarterSample = motion.sample(startTime + 3 * duration / 4)
        val fraction1ThreeQuarter = fractionFor(threeQuarterSample, 1L)
        val fraction2ThreeQuarter = fractionFor(threeQuarterSample, 2L)
        assertEquals("key=1 应已完成（fraction=1）", 1f, fraction1ThreeQuarter, 0.001f)
        assertEquals("key=2 应在 [0.5,1] 中点 fraction=0.5", 0.5f, fraction2ThreeQuarter, 0.001f)

        // 不重叠：25% 时 key=2 还没开始（fraction=1），75% 时 key=1 已完成（fraction=1），
        // 两者在不同区间，不是各自归一化到 [0,1] 同时播放。
        assertTrue("不重叠：key=2 在 key=1 之后才动", fraction2Quarter > fraction2ThreeQuarter)
    }

    /**
     * Issue #728 评论 5760741452 问题2：~~redirectCaretTo 后 traversal 顺序保持~~ —
     * 旧 [ComposeEditMotion.redirectCaretTo] 已删除。一笔编辑只有一个 [CoordinatedEditMotion]。
     *
     * 等价覆盖：[CaretTraversal] 的 segment 列表顺序决定光标遍历顺序。
     * 验证多 segment traversal 按 startProgress 升序排列，光标依次经过各 segment。
     */
    @Test
    fun caretTraversal_preservesSegmentOrder() {
        // 构造 3 segment traversal，模拟跨行光标移动
        // segment 0: [0, 1/3]，segment 1: [1/3, 2/3]，segment 2: [2/3, 1]
        val rect0 = Rect(left = 10f, top = 0f, right = 12f, bottom = 20f)
        val rect1 = Rect(left = 50f, top = 20f, right = 52f, bottom = 40f)
        val rect2 = Rect(left = 30f, top = 40f, right = 32f, bottom = 60f)
        val traversal =
            CaretTraversal(
                segments =
                    listOf(
                        CaretTraversal.Segment(rect0, rect1, lineIndex = 0, startProgress = 0f, endProgress = 1f / 3f),
                        CaretTraversal.Segment(
                            rect1,
                            rect2,
                            lineIndex = 1,
                            startProgress = 1f / 3f,
                            endProgress = 2f / 3f,
                        ),
                        CaretTraversal.Segment(rect2, rect2, lineIndex = 2, startProgress = 2f / 3f, endProgress = 1f),
                    ),
                isValid = true,
            )

        // progress=1/6：在 segment 0 中点，caret 在 rect0→rect1 中点
        val caretAtSixth = traversal.sampleCaret(1f / 6f)
        assertEquals("segment 0 中点 left 应为 30（(10+50)/2）", 30f, caretAtSixth.left, 0.001f)

        // progress=1/2：在 segment 1 中点，caret 在 rect1→rect2 中点
        val caretAtHalf = traversal.sampleCaret(0.5f)
        assertEquals("segment 1 中点 left 应为 40（(50+30)/2）", 40f, caretAtHalf.left, 0.001f)

        // progress=5/6：在 segment 2 中点，caret 在 rect2（start=end=rect2）
        val caretAtFiveSixth = traversal.sampleCaret(5f / 6f)
        assertEquals("segment 2 中点 left 应为 30（rect2.left）", 30f, caretAtFiveSixth.left, 0.001f)

        // 验证遍历顺序：rect0 → rect1 → rect2，不能跳过中间 segment
        val caretStart = traversal.sampleCaret(0f)
        assertEquals("起点应在 rect0", rect0.left, caretStart.left, 0.001f)
        val caretEnd = traversal.sampleCaret(1f)
        assertEquals("终点应在 rect2", rect2.left, caretEnd.left, 0.001f)
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
     * fraction == 0 的 glyph 不在 overlays 里，返回 0f。
     */
    private fun fractionFor(
        sample: CoordinatedEditMotion.Sample,
        key: Long,
    ): Float = sample.glyphOverlays.firstOrNull { it.key == key }?.clipFraction ?: 0f

    /**
     * 构造只有 deleted channel 的 [CoordinatedEditMotion] —
     * 模拟 [CoordinatedEditMotion.fromPatch] 只有 deleted 时的区间分配：
     * deleted 占全区间 [0, 1]（无 inserted 时后半段退化为全区间），n 个 unit 反序分配（右往左吞）。
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
            targetCaretRect = targetCaretRect,
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
    @Suppress("LongParameterList")
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
            targetCaretRect = targetCaretRect,
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
        // deleted 区间：混合时占 [0.5, 1]，纯 deleted 时占 [0, 1]，反序（右往左吞）— 与 fromPatch 一致
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
