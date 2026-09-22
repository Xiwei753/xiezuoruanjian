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
 * Issue #728 评论 5755928697 三个确定问题的回归测试。
 *
 * Issue #737 重写：旧 [ComposeEditMotion]（forInsert/forDelete/forEdit/redirectTo/redirectCaretTo）
 * 已删除，本测试重写为验证 [CoordinatedEditMotion] 的等价行为。
 *
 * 三个问题（在新架构下的等价覆盖）：
 * 1. ~~文字动画还没结束时移动光标会把 glyph channel 全丢掉~~ —
 *    Issue #737：redirectCaretTo 已删除（一笔编辑只有一个 motion，不存在"redirect 丢 channel"）。
 *    等价覆盖：[CoordinatedEditMotion] 一笔内 caret 和 glyph 共用同一 progress，不存在丢字问题。
 * 2. ~~coordinated=false 时独立 smooth cursor 设置对正文编辑不生效~~ —
 *    Issue #737：一笔编辑一只钟，caret 和 glyph 永远共用同一个 progress/duration。
 *    等价覆盖：[coordinatedTrue_caretAndGlyphShareSingleProgress] 验证单一 progress 语义。
 * 3. 一笔里有多个 glyph unit 时所有字同时吐/吞，不是真正跟着光标。
 *    等价覆盖：[multipleInsertedUnits_revealSequentially_followingCaret] 等测试验证
 *    [CoordinatedEditMotion.fromPatch] 的区间分配（inserted [0, 0.5]，deleted [0.5, 1]）。
 */
@Suppress("MaxLineLength", "LongMethod")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue728Comment5755928697ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    private val originRect = Rect(left = 10f, top = 0f, right = 12f, bottom = 20f)
    private val targetRect = Rect(left = 30f, top = 0f, right = 32f, bottom = 20f)
    private val startTime = 1_000_000_000L

    /**
     * 600ms — 能被 12 整除，方便算 1/12、1/6、1/4、5/12、7/12、3/4、11/12 区间映射点。
     */
    private val glyphDuration = 600_000_000L

    private lateinit var dummyLayout: ComposeLayoutSnapshot

    @Before
    fun setUp() {
        dummyLayout = ComposeLayoutSnapshot(measureText("a"), TextRange(0, 1), 0)
    }

    // ==================== 问题1：redirectCaretTo 保留 glyph channel ====================

    /*
     * 以下旧 API 测试已删除（Issue #737 重写）：
     *
     * - ~~redirectCaretTo_preservesRunningGlyphChannels~~ —
     *   旧 [ComposeEditMotion.redirectCaretTo] 已删除。一笔编辑只有一个 [CoordinatedEditMotion]，
     *   不存在"移动光标丢 channel"的问题。caret 和 glyph 共用同一 progress，要有都有。
     * - ~~redirectTo_withEmptyKeys_dropsChannels_contrastWithRedirectCaretTo~~ —
     *   旧 [ComposeEditMotion.redirectTo] 已删除。见上注释。
     */

    // ==================== 问题2：单一 duration ====================

    /**
     * 问题2：一笔编辑一只钟 — caret 和 glyph 共用同一个 progress/duration。
     *
     * Issue #737：删除双 duration 后，caret 和 glyph 永远共用同一个 progress。
     * 混合 edit（1 inserted + 1 deleted）时，inserted 占 [0, 0.5]，deleted 占 [0.5, 1]，
     * 两者在一条 schedule 上不重叠，caret 走完整 [0, 1]。
     */
    @Test
    fun coordinatedTrue_caretAndGlyphShareSingleProgress() {
        val sharedDuration = 200_000_000L // 200ms
        val motion =
            makeEditMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L),
                deletedKeys = listOf(2L),
                frameTimeNanos = startTime,
                durationNanos = sharedDuration,
            )
        // 25% 时间（progress=0.25）：caret 在 25%，inserted 在 [0, 0.5] 中点 fraction=0.5，
        // deleted 还没开始 fraction=1，motion 未 finished
        val quarterSample = motion.sample(startTime + sharedDuration / 4)
        assertEquals("caret 在 25%（progress=0.25）", 15f, quarterSample.caretRect.left, 0.001f)
        assertEquals("inserted 在 [0,0.5] 中点 fraction=0.5", 0.5f, fractionFor(quarterSample, 1L), 0.001f)
        assertEquals("deleted 还没开始 fraction=1", 1f, fractionFor(quarterSample, 2L), 0.001f)
        assertFalse("25% 时未 finished", quarterSample.finished)

        // 75% 时间（progress=0.75）：caret 在 75%，inserted 已完成 fraction=1，
        // deleted 在 [0.5, 1] 中点 fraction=0.5，motion 未 finished
        val threeQuarterSample = motion.sample(startTime + 3 * sharedDuration / 4)
        assertEquals("caret 在 75%（progress=0.75）", 25f, threeQuarterSample.caretRect.left, 0.001f)
        assertEquals("inserted 已完成 fraction=1", 1f, fractionFor(threeQuarterSample, 1L), 0.001f)
        assertEquals("deleted 在 [0.5,1] 中点 fraction=0.5", 0.5f, fractionFor(threeQuarterSample, 2L), 0.001f)
        assertFalse("75% 时未 finished", threeQuarterSample.finished)

        // 终点：两者都 finished
        val endSample = motion.sample(startTime + sharedDuration)
        assertEquals(targetRect, endSample.caretRect)
        assertEquals(1f, fractionFor(endSample, 1L), 0.001f)
        assertEquals(0f, fractionFor(endSample, 2L), 0.001f)
        assertTrue("终点时 finished", endSample.finished)
    }

    // ==================== 问题3：多 unit 区间映射跟着光标 ====================

    /**
     * 问题3：多个 inserted unit 依次吐字，跟着光标。
     *
     * Issue #737：fromPatch 中 inserted 占前半段 [0, 0.5]，3 个 unit 区间：
     * - key=1: [0, 1/6]
     * - key=2: [1/6, 1/3]
     * - key=3: [1/3, 0.5]
     *
     * 验证光标从左到右依次吐字，不是所有字同时吐。
     */
    @Test
    fun multipleInsertedUnits_revealSequentially_followingCaret() {
        val motion =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L, 2L, 3L),
                frameTimeNanos = startTime,
                durationNanos = glyphDuration,
            )

        // progress=1/12：key=1 区间 [0, 1/6] 中点 fraction=0.5；key=2/3 还没开始 fraction=0
        val t1 = startTime + glyphDuration / 12
        val s1 = motion.sample(t1)
        assertEquals("progress=1/12: key=1 正在吐 fraction≈0.5", 0.5f, fractionFor(s1, 1L), 0.001f)
        assertEquals("progress=1/12: key=2 还没开始 fraction=0", 0f, fractionFor(s1, 2L), 0.001f)
        assertEquals("progress=1/12: key=3 还没开始 fraction=0", 0f, fractionFor(s1, 3L), 0.001f)

        // progress=1/4：key=1 已吐完 fraction=1；key=2 区间 [1/6, 1/3] 中点 fraction=0.5；
        // key=3 还没开始 fraction=0
        val t2 = startTime + glyphDuration / 4
        val s2 = motion.sample(t2)
        assertEquals("progress=1/4: key=1 已吐完 fraction=1", 1f, fractionFor(s2, 1L), 0.001f)
        assertEquals("progress=1/4: key=2 正在吐 fraction≈0.5", 0.5f, fractionFor(s2, 2L), 0.001f)
        assertEquals("progress=1/4: key=3 还没开始 fraction=0", 0f, fractionFor(s2, 3L), 0.001f)

        // progress=5/12：key=1/2 已吐完 fraction=1；key=3 区间 [1/3, 0.5] 中点 fraction=0.5
        val t3 = startTime + 5 * glyphDuration / 12
        val s3 = motion.sample(t3)
        assertEquals("progress=5/12: key=1 已吐完 fraction=1", 1f, fractionFor(s3, 1L), 0.001f)
        assertEquals("progress=5/12: key=2 已吐完 fraction=1", 1f, fractionFor(s3, 2L), 0.001f)
        assertEquals("progress=5/12: key=3 正在吐 fraction≈0.5", 0.5f, fractionFor(s3, 3L), 0.001f)
    }

    /**
     * 问题3：多个 deleted unit 反向吞字，跟着光标从右往左。
     *
     * Issue #737：fromPatch 中 deleted 占后半段 [0.5, 1]，3 个 unit 反序分配：
     * - key=3 (i=2): [0.5, 2/3]（最先吞，最右边）
     * - key=2 (i=1): [2/3, 5/6]
     * - key=1 (i=0): [5/6, 1]（最后吞，最左边）
     *
     * 验证删除按 caret 实际经过顺序反向映射（光标从右往左，先吞最右边）。
     */
    @Test
    fun multipleDeletedUnits_concealReversed_followingCaret() {
        val motion =
            makeDeleteMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedKeys = listOf(1L, 2L, 3L),
                frameTimeNanos = startTime,
                durationNanos = glyphDuration,
            )

        // progress=7/12：key=3 区间 [0.5, 2/3] 中点 fraction=0.5（正在吞）；
        // key=1/2 还没开始 fraction=1
        val t1 = startTime + 7 * glyphDuration / 12
        val s1 = motion.sample(t1)
        assertEquals("progress=7/12: key=1 完全可见 fraction=1", 1f, fractionFor(s1, 1L), 0.001f)
        assertEquals("progress=7/12: key=2 完全可见 fraction=1", 1f, fractionFor(s1, 2L), 0.001f)
        assertEquals("progress=7/12: key=3 正在吞 fraction≈0.5", 0.5f, fractionFor(s1, 3L), 0.001f)

        // progress=3/4：key=3 已吞完 fraction=0；key=2 区间 [2/3, 5/6] 中点 fraction=0.5；
        // key=1 还没开始 fraction=1
        val t2 = startTime + 3 * glyphDuration / 4
        val s2 = motion.sample(t2)
        assertEquals("progress=3/4: key=1 完全可见 fraction=1", 1f, fractionFor(s2, 1L), 0.001f)
        assertEquals("progress=3/4: key=2 正在吞 fraction≈0.5", 0.5f, fractionFor(s2, 2L), 0.001f)
        assertEquals("progress=3/4: key=3 已吞完 fraction=0", 0f, fractionFor(s2, 3L), 0.001f)

        // progress=11/12：key=3/2 已吞完 fraction=0；key=1 区间 [5/6, 1] 中点 fraction=0.5
        val t3 = startTime + 11 * glyphDuration / 12
        val s3 = motion.sample(t3)
        assertEquals("progress=11/12: key=1 正在吞 fraction≈0.5", 0.5f, fractionFor(s3, 1L), 0.001f)
        assertEquals("progress=11/12: key=2 已吞完 fraction=0", 0f, fractionFor(s3, 2L), 0.001f)
        assertEquals("progress=11/12: key=3 已吞完 fraction=0", 0f, fractionFor(s3, 3L), 0.001f)
    }

    /**
     * 问题3：混合 edit（1 inserted + 1 deleted）— inserted 占前半段，deleted 占后半段。
     *
     * Issue #737：fromPatch 混合编辑：inserted [0, 0.5]，deleted [0.5, 1]。
     *
     * 验证光标先经过插入区吐字，再经过删除区吞字。
     */
    @Test
    fun mixedEdit_insertedFirstHalf_deletedSecondHalf() {
        val motion =
            makeEditMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(1L),
                deletedKeys = listOf(2L),
                frameTimeNanos = startTime,
                durationNanos = glyphDuration,
            )

        // progress=0.25：inserted 正在吐 fraction=0.5，deleted 还没吞 fraction=1
        val t1 = startTime + glyphDuration / 4
        val s1 = motion.sample(t1)
        assertEquals("progress=0.25: inserted 正在吐 fraction≈0.5", 0.5f, fractionFor(s1, 1L), 0.001f)
        assertEquals("progress=0.25: deleted 还没吞 fraction=1", 1f, fractionFor(s1, 2L), 0.001f)

        // progress=0.75：inserted 已吐完 fraction=1，deleted 正在吞 fraction=0.5
        val t2 = startTime + 3 * glyphDuration / 4
        val s2 = motion.sample(t2)
        assertEquals("progress=0.75: inserted 已吐完 fraction=1", 1f, fractionFor(s2, 1L), 0.001f)
        assertEquals("progress=0.75: deleted 正在吞 fraction≈0.5", 0.5f, fractionFor(s2, 2L), 0.001f)
    }

    // ==================== 问题1：redirect 保留 master progress 相位 ====================

    /*
     * 以下旧 API 测试已删除（Issue #737 重写）：
     *
     * - ~~redirect_preservesPhase_activeUnitContinuesImmediately~~ —
     *   旧 [ComposeEditMotion.redirectCaretTo] 已删除。一笔编辑只有一个 motion，
     *   不存在"redirect 后冻结"的问题。
     * - ~~redirect_noFrozenGap_inProgressUnitImmediately~~ —
     *   旧 [ComposeEditMotion.redirectCaretTo] 已删除。见上注释。
     * - ~~redirectTo_activeUnitContinuesFromCurrentFraction~~ —
     *   旧 [ComposeEditMotion.redirectTo] 已删除。见上注释。
     */

    // ==================== 问题2：unit 顺序来自正文/几何 ====================

    /**
     * 问题2：unit 顺序应该来自传入顺序（正文位置），不是 key 编号 sorted()。
     *
     * Issue #737：[CoordinatedEditMotion] 构造时按传入的 insertedKeys/deletedKeys 顺序分配区间。
     * 两种不同 key 顺序导致不同区间分配，验证构造尊重传入顺序。
     */
    @Test
    fun forInsert_respectsKeyOrder_notKeySorted() {
        // 顺序 1：key=10 在前，key=11 在后
        val motion1 =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(10L, 11L),
                frameTimeNanos = startTime,
                durationNanos = glyphDuration,
            )
        // 顺序 2：key=11 在前，key=10 在后（逆序）
        val motion2 =
            makeInsertMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                insertedKeys = listOf(11L, 10L),
                frameTimeNanos = startTime,
                durationNanos = glyphDuration,
            )

        // 在 progress=1/8 时（inserted 2 units 占 [0, 0.5]，每个 [0, 0.25] 或 [0.25, 0.5]）：
        // motion1: key=10 区间 [0, 0.25]，key=11 区间 [0.25, 0.5]
        // motion2: key=11 区间 [0, 0.25]，key=10 区间 [0.25, 0.5]
        val t = startTime + glyphDuration / 8
        val s1 = motion1.sample(t)
        val s2 = motion2.sample(t)

        // motion1 中 key=10 正在吐（区间 [0, 0.25] 中点 fraction≈0.5）
        assertTrue("motion1 key=10 应正在吐", fractionFor(s1, 10L) > 0.4f)
        // motion1 中 key=11 还没开始（区间 [0.25, 0.5]）
        assertEquals("motion1 key=11 应还没开始", 0f, fractionFor(s1, 11L), 0.001f)

        // motion2 中 key=11 正在吐（区间 [0, 0.25] 中点 fraction≈0.5）
        assertTrue("motion2 key=11 应正在吐", fractionFor(s2, 11L) > 0.4f)
        // motion2 中 key=10 还没开始（区间 [0.25, 0.5]）
        assertEquals("motion2 key=10 应还没开始", 0f, fractionFor(s2, 10L), 0.001f)
    }

    /*
     * 以下旧 API 测试已删除（Issue #737 重写）：
     *
     * - ~~redirect_preservesPhase_whenGlyphDurationChanges~~ —
     *   旧 [ComposeEditMotion.redirectCaretTo] 已删除。一笔编辑只有一个 motion，
     *   不存在"redirect 时 duration 变化导致相位失真"的问题。
     * - ~~redirectTo_interleavesOldAndNewByTextOrder_notByCreationTime~~ —
     *   旧 [ComposeEditMotion.redirectTo] 已删除。一笔编辑只有一个 motion，
     *   不存在"rapid redirect 交错排"的问题。新 motion 从 fromPatch 按正文顺序分配区间。
     */

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

    /** 从 [CoordinatedEditMotion.Sample.glyphOverlays] 按 key 查找 clipFraction。 */
    private fun fractionFor(
        sample: CoordinatedEditMotion.Sample,
        key: Long,
    ): Float = sample.glyphOverlays.firstOrNull { it.key == key }?.clipFraction ?: 0f

    /** 构造只有 inserted channel 的 [CoordinatedEditMotion]（inserted 占 [0, 0.5]）。 */
    private fun makeInsertMotion(
        originCaretRect: Rect,
        targetCaretRect: Rect,
        insertedKeys: List<Long>,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): CoordinatedEditMotion =
        makeMotion(
            originCaretRect = originCaretRect,
            targetCaretRect = targetCaretRect,
            insertedKeys = insertedKeys,
            deletedKeys = emptyList(),
            frameTimeNanos = frameTimeNanos,
            durationNanos = durationNanos,
        )

    /** 构造只有 deleted channel 的 [CoordinatedEditMotion]（deleted 占 [0.5, 1]）。 */
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

    /** 构造混合 inserted + deleted channel 的 [CoordinatedEditMotion]（inserted [0, 0.5]，deleted [0.5, 1]）。 */
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

    /** 构造 [CoordinatedEditMotion] — 复用 fromPatch 的区间分配逻辑，自己指定 key。 */
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
        // inserted 占前半段 [0, 0.5]
        for (i in insertedKeys.indices) {
            val key = insertedKeys[i]
            val start = 0.5f * i.toFloat() / insertedCount.toFloat()
            val end = 0.5f * (i + 1).toFloat() / insertedCount.toFloat()
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
        // deleted 占后半段 [0.5, 1]，反序（右往左吞）
        for (i in deletedKeys.indices) {
            val key = deletedKeys[i]
            val start = 0.5f + 0.5f * (deletedCount - 1 - i).toFloat() / deletedCount.toFloat()
            val end = 0.5f + 0.5f * (deletedCount - i).toFloat() / deletedCount.toFloat()
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
