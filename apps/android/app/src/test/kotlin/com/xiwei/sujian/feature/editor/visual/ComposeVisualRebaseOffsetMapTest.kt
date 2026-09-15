package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
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
 * #684 评论 5662132136 第1项：offset map chain 语义验证。
 *
 * Core 定义：
 * - IDENTITY = 文本相同且 offset 不变
 * - SHIFTED  = 文本相同但 offset 改变（被前后增删平移）
 * - 编辑/删除/替换区域 = 根本没有映射条目
 *
 * 验证：
 * 1. SHIFTED 后缀（如删除中间字符后的回流文字）必须生成 retained move（旧实现只处理 IDENTITY 会漏掉）。
 * 2. 删除区域在合成 map 里不能补 identity gap（旧实现会把已删除文字当成存活文字）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualRebaseOffsetMapTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * 删除区域 [1,2) 在合成 map 里不得出现任何条目（无 identity gap 补位）。
     * 这是第1项的核心：旧 buildStageSegments 会把 [1,2) 补成 IDENTITY，
     * 等于把已删除的 "b" 当成存活文字参与 chain 合成。
     */
    @Test
    fun deletedRegionHasNoMappingEntry_noGapFill() {
        val layouts = captureLayouts("abc", "ac")
        val intent =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY),
                                VisualOffsetMapEntry(2, 1, 1, VisualOffsetMapKind.SHIFTED),
                            ),
                    ),
                oldRanges = emptyList(),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "abc",
                expectedNewText = "ac",
            )
        val composed = ComposeVisualRebase.composeOffsetMapChain(listOf(intent))
        assertNotNull("offset map chain 应合成成功", composed)
        assertFalse(
            "删除区域 [1,2) 不应补 identity gap 条目",
            composed!!.any { it.oldStart == 1 && it.length >= 1 },
        )
        assertTrue(
            "SHIFTED 后缀条目应保留",
            composed.any { it.kind == VisualOffsetMapKind.SHIFTED },
        )
    }

    /**
     * 删除一行末尾换行导致后文上移：删除 "ab\n" 末尾的 "\n" -> "ab"。
     * 后续 "cd" 由 old[3,5) 平移到 new[2,4)，应生成 SHIFTED retained move。
     */
    @Test
    fun deleteLineBreak_reflowsSuffixAsShiftedMove() {
        val layouts = captureLayouts("ab\ncd", "abcd")
        val intent =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                // "ab\n"
                                VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY),
                                // "cd" 上移
                                VisualOffsetMapEntry(3, 2, 2, VisualOffsetMapKind.SHIFTED),
                            ),
                    ),
                oldRanges = emptyList(),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "ab\ncd",
                expectedNewText = "abcd",
            )
        val moves =
            ComposeVisualRebase.computeRetainedMoves(
                oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(3, 3), 0),
                newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(4, 4), 0),
                chain = listOf(intent),
            )
        assertTrue("删除换行导致后文回流应产生 retained move", moves.isNotEmpty())
        assertTrue(
            "回流的 'cd'（new[2,4)）应被保留并移动",
            moves.any { it.newRange.start == 2 && it.newRange.end == 4 },
        )
    }

    /**
     * #684 评论 5663032418 断点2：跨多行后缀必须按视觉行切分，不能整段一个 dx/dy。
     *
     * 场景：删除一个换行符使后缀跨行上移。
     * - old = "xx\naa\nbb"（3 行：xx / aa / bb）
     * - new = "xxaa\nbb"（2 行：xxaa / bb）
     *
     * offset map（删除 old[2]='\n'）：
     * - "xx" IDENTITY [0,2) → [0,2)
     * - "aa" SHIFTED [3,5) → [2,4)（从行 1 上移到行 0 末尾，dx = width("xx") > 0, dy < 0）
     * - "\n" SHIFTED [5,6) → [4,5)（换行符前移）
     * - "bb" SHIFTED [6,8) → [5,7)（从行 2 上移到行 1，dx = 0, dy < 0）
     *
     * "aa" 的 dx > 0，"bb" 的 dx = 0 — 位移向量不同。
     * 旧实现先合并相邻 entry（位移都是 -1）再整段算一个 dx/dy，只生成 1 个 move。
     * 新实现按视觉行切分，应生成多个 newRange 不重叠、位移向量不同的 move。
     *
     * 注意：Robolectric 下 rememberTextMeasurer 不做真实字体度量（软换行不可靠），
     * 但硬换行 `\n` 一定产生多行布局，所以用硬换行构造跨多行场景。
     */
    @Test
    fun multiLineReflow_producesMultipleRetainedMovesWithDistinctDisplacement() {
        val layouts = captureLayouts("xx\naa\nbb", "xxaa\nbb")
        val oldLayout = layouts[0]
        val newLayout = layouts[1]

        // 确认硬换行产生了多行布局。
        assertTrue(
            "old 文本应跨多行（硬换行），实际 lineCount=${oldLayout.lineCount}",
            oldLayout.lineCount >= 3,
        )
        assertTrue(
            "new 文本应跨多行（硬换行），实际 lineCount=${newLayout.lineCount}",
            newLayout.lineCount >= 2,
        )

        // offset map：删除 old[2]='\n'，"aa"/"\n"/"bb" 都前移 1。
        val intent =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                // "xx"
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                                // "aa" 上移
                                VisualOffsetMapEntry(3, 2, 2, VisualOffsetMapKind.SHIFTED),
                                // "\n" 前移
                                VisualOffsetMapEntry(5, 4, 1, VisualOffsetMapKind.SHIFTED),
                                // "bb" 上移
                                VisualOffsetMapEntry(6, 5, 2, VisualOffsetMapKind.SHIFTED),
                            ),
                    ),
                oldRanges = emptyList(),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "xx\naa\nbb",
                expectedNewText = "xxaa\nbb",
            )
        val moves =
            ComposeVisualRebase.computeRetainedMoves(
                oldLayout = ComposeLayoutSnapshot(oldLayout, TextRange(2, 2), 0),
                newLayout = ComposeLayoutSnapshot(newLayout, TextRange(2, 2), 0),
                chain = listOf(intent),
            )
        assertTrue("跨多行后缀回流应产生 retained move", moves.isNotEmpty())

        // 关键断言：应生成多个 newRange 不重叠的 move —
        // 旧实现整段一个 dx/dy 只会生成 1 个 move，新实现按视觉行切分应生成 >1 个。
        assertTrue(
            "跨多行后缀应按视觉行切分成多个 retained move（旧实现只有 1 个整段 move），实际 moves.size=${moves.size}",
            moves.size > 1,
        )

        // 各 move 的 newRange 不应完全重叠（说明是不同视觉行的 chunk）。
        val newRanges = moves.map { it.newRange }
        for (i in newRanges.indices) {
            for (j in (i + 1) until newRanges.size) {
                val a = newRanges[i]
                val b = newRanges[j]
                val overlap = !(a.end <= b.start || b.end <= a.start)
                assertFalse(
                    "不同 retained move 的 newRange 不应重叠（move $i: $a vs move $j: $b）",
                    overlap,
                )
            }
        }

        // 各 move 的位移向量 (dx, dy) 不完全一致 —
        // "aa" 从行 1 移到行 0 末尾（dx > 0），"bb" 从行 2 移到行 1（dx = 0），位移不同。
        val displacements =
            moves.map { move ->
                val oldBounds = ComposeVisualRebase.safePathBounds(oldLayout, move.oldRange)
                val newBounds = ComposeVisualRebase.safePathBounds(newLayout, move.newRange)
                requireNotNull(oldBounds) { "oldBounds 不应为 null: ${move.oldRange}" }
                requireNotNull(newBounds) { "newBounds 不应为 null: ${move.newRange}" }
                Pair(newBounds.left - oldBounds.left, newBounds.top - oldBounds.top)
            }
        val distinctDisplacementCount = displacements.toSet().size
        assertTrue(
            "各 retained move 的位移向量应不完全一致（跨多行各行位移不同），实际位移: $displacements",
            distinctDisplacementCount > 1,
        )
    }

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> =
        captureLayoutsWithWidth(texts, maxWidth = 1000)

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
