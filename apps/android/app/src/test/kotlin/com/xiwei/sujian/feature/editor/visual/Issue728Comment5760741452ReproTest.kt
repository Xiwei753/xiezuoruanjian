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
 * Issue #728 评论 5760741452 回归测试 — 两个确定运行错误的修复验证。
 *
 * Issue #737 重写：旧 [ComposeEditMotion]（redirectTo/redirectCaretTo）已删除。
 * 一笔编辑只有一个 [CoordinatedEditMotion]，不再有"先跑 motion1 再 redirect 成 motion2"的分离逻辑。
 *
 * 两个问题（在新架构下的等价覆盖）：
 * 1. ~~角色反转后 glyph 与 caret 同步到达终点~~ —
 *    Issue #737：redirectTo 已删除。glyph 角色在构造时确定，不存在"运行中角色反转"。
 *    等价覆盖：[ComposeEditMotionRedirectRoleTest.deletedGlyph_targetsZero_glyphAndCaretSynced]
 *    验证 Deleted glyph 从 1→0 吞字，且 glyph 与 caret 同步到达终点。
 * 2. ~~redirectCaretTo 后 traversal 顺序保持 12→11→10~~ —
 *    Issue #737：redirectCaretTo 已删除。traversal 顺序在构造时确定。
 *    等价覆盖：[ComposeEditMotionRedirectRoleTest.deletedUnits_keepRightToLeftOrder]
 *    和 [ComposeEditMotionRedirectRoleTest.caretTraversal_preservesSegmentOrder]
 *    验证 deleted 反序分配和 traversal segment 顺序。
 */
@Suppress("MaxLineLength", "LongMethod")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue728Comment5760741452ReproTest {
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

    // ==================== 问题1：角色反转后 glyph 与 caret 同步到达终点 ====================

    /*
     * 以下旧 API 测试已删除（Issue #737 重写）：
     *
     * - ~~regression_issue1_roleReversal_glyphSyncsWithCaret~~ —
     *   旧 [ComposeEditMotion.redirectTo] 已删除。一笔编辑只有一个 [CoordinatedEditMotion]，
     *   glyph 角色在构造时确定，不存在"运行中角色反转"。
     *   等价覆盖：[ComposeEditMotionRedirectRoleTest.deletedGlyph_targetsZero_glyphAndCaretSynced]
     *   验证 Deleted glyph 从 1→0 吞字，glyph 与 caret 同步到达终点（80% 时 fraction=0.4，100% 时 fraction=0）。
     * - ~~regression_issue1_correctBehaviorHeld~~ —
     *   旧 [ComposeEditMotion.redirectTo] 已删除。见上注释。
     */

    // ==================== 问题2：redirectCaretTo traversal 顺序保持 12→11→10 ====================

    /*
     * 以下旧 API 测试已删除（Issue #737 重写）：
     *
     * - ~~regression_issue2_redirectCaretTo_preservesTraversalOrder~~ —
     *   旧 [ComposeEditMotion.redirectCaretTo] 已删除。一笔编辑只有一个 [CoordinatedEditMotion]，
     *   traversal 顺序在构造时确定，不存在"redirect 后 traversal 顺序错乱"。
     *   等价覆盖：[ComposeEditMotionRedirectRoleTest.deletedUnits_keepRightToLeftOrder]
     *   验证 deleted 反序分配区间（右往左吞），key=12 先吞，key=10 后吞。
     * - ~~regression_issue2_channelRangesShowCorrectOrder~~ —
     *   旧 [ComposeEditMotion.redirectCaretTo] 已删除。见上注释。
     *   等价覆盖：[ComposeEditMotionRedirectRoleTest.caretTraversal_preservesSegmentOrder]
     *   验证 [CaretTraversal] segment 列表顺序决定光标遍历顺序。
     */

    /**
     * Issue #737 新增等价覆盖：验证 [CoordinatedEditMotion] 的 Deleted glyph 与 caret 同步。
     *
     * 单 deleted unit 区间 [0.5, 1]，80% 时 progress=0.8，local=0.6，fraction=0.4；
     * caret 80% 时 left=26（还没到 30）。glyph 和 caret 都没到终点 — 同步。
     */
    @Test
    fun newArchitecture_deletedGlyph_syncsWithCaret() {
        val motion =
            makeDeleteMotion(
                originCaretRect = originRect,
                targetCaretRect = targetRect,
                deletedKeys = listOf(1L),
                frameTimeNanos = startTime,
                durationNanos = duration,
            )
        // 80% 时：progress=0.8，deleted 区间 [0.5, 1]，local=0.6，fraction=0.4
        val atEighty = motion.sample(startTime + duration * 4 / 5)
        assertEquals("glyph 80% 时 fraction=0.4", 0.4f, fractionFor(atEighty, 1L), 0.001f)
        assertEquals("caret 80% 时 left=26", 26f, atEighty.caretRect.left, 0.001f)
        // 两者都没到终点 — 同步
        assertTrue("glyph 80% 时还没到终点", fractionFor(atEighty, 1L) > 0.001f)
        assertTrue("caret 80% 时还没到终点", atEighty.caretRect.left < 29.999f)

        // 100% 时同时到达终点
        val atFull = motion.sample(startTime + duration)
        assertEquals("glyph 100% 时 fraction=0", 0f, fractionFor(atFull, 1L), 0.001f)
        assertEquals("caret 100% 时 left=30", 30f, atFull.caretRect.left, 0.001f)
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

    private fun fractionFor(
        sample: CoordinatedEditMotion.Sample,
        key: Long,
    ): Float = sample.glyphOverlays.firstOrNull { it.key == key }?.clipFraction ?: 0f

    private fun makeDeleteMotion(
        originCaretRect: Rect,
        targetCaretRect: Rect,
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
        val deletedCount = deletedKeys.size
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
            newSelection = TextRange(0, 0),
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
