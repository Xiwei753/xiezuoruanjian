package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.unit.Constraints
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
                                VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY), // "ab\n"
                                VisualOffsetMapEntry(3, 2, 2, VisualOffsetMapKind.SHIFTED), // "cd" 上移
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

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }
}
