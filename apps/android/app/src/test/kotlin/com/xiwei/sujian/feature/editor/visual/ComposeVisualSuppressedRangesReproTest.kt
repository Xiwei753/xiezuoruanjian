package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
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
 * #684 评论 5663862982 复现测试 — 两个显示链 bug：
 *
 * Bug 1：hiddenRanges 未包含 retained move newRanges，导致重影/跳行。
 *   overlay 画 retainedMoves（被挤到下一行的保留文字），但这些 retainedMoves 的
 *   newRange 对应的正文没有被隐藏，BasicTextField 在最终新位置的那份正文同时可见。
 *
 * Bug 2：多笔 intent 合成一个屏幕事务时，startFrame 用最后一笔 replaceBounds 映射，坐标系错。
 *   T0 -> T1 -> T2 多笔 intent 合成一个屏幕事务，startFrame 仍只按最后一笔的
 *   replaceBounds（T1->T2 坐标）映射，而 startFrame 的 targetRange 是 T0 坐标。
 *   直接拿最后一笔 bounds 去切 T0 range 是错坐标系。
 *
 * 两个测试在当前代码下都应失败（证明 bug 存在）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualSuppressedRangesReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * Bug 1 复现：删除换行符导致后文回流，retained move 的 newRange 应被 hiddenRanges 包含。
     *
     * 场景：old = "ab\ncd" → new = "abcd"（删除 old[2]='\n'）。
     * "cd" 从 old 第二行回流到 new 第一行末尾，生成 retained move(newRange=[2,4))。
     * overlay 会画这个 retained move，但 hiddenRanges 不含 [2,4)，
     * 导致 BasicTextField 在 new[2,4) 的正文同时可见 → 重影/跳行。
     *
     * 当前代码下 hiddenRanges = mergedNewRanges.filter{...} = emptyList（Delete 的 newRanges 为空），
     * 不含 retained move 的 newRange → 断言失败。
     */
    @Test
    fun retainedMove_newRange_notInHiddenRanges_bug1() {
        val layouts = captureLayouts("ab\ncd", "abcd")
        val oldLayout = layouts[0]
        val newLayout = layouts[1]

        // 确认硬换行产生了多行布局。
        assertTrue(
            "old 文本应跨多行（硬换行），实际 lineCount=${oldLayout.lineCount}",
            oldLayout.lineCount >= 2,
        )

        val state = ComposeEditorVisualState(targetId = "test-target")

        // 1. 基线 layout "ab\ncd" 到达。
        state.onAuthoritativeLayout(oldLayout, TextRange(2, 2), 0)

        // 2. Delete 事务：删除 old[2]='\n'，"cd" 回流。
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
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY), // "ab"
                                VisualOffsetMapEntry(3, 2, 2, VisualOffsetMapKind.SHIFTED), // "cd" 上移
                            ),
                    ),
                oldRanges = listOf(TextRange(2, 3)), // 删除的 "\n"
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 2, oldEnd = 3, newStart = 2, newEnd = 2),
                expectedOldText = "ab\ncd",
                expectedNewText = "abcd",
            )
        state.onVisualIntent(
            intent,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // 3. 新 layout "abcd" 到达 → 生成事务。
        state.onAuthoritativeLayout(newLayout, TextRange(4, 4), 0)

        val transaction = state.activeTransaction.value
        assertNotNull("事务应生成", transaction)

        // 验证 retained moves 确实产生了（"cd" 回流）。
        val retainedMoves = transaction?.retainedMoves ?: emptyList()
        assertTrue(
            "删除换行导致后文回流应产生 retained move，实际 moves=$retainedMoves",
            retainedMoves.isNotEmpty(),
        )
        val cdMove =
            retainedMoves.firstOrNull { it.newRange.start == 2 && it.newRange.end == 4 }
        assertNotNull(
            "回流的 'cd'（new[2,4)）应被保留并移动，实际 moves=$retainedMoves",
            cdMove,
        )

        // 核心断言：hiddenRanges 应包含 retained move 的 newRange [2,4)。
        // 当前代码下 hiddenRanges = mergedNewRanges.filter{...} = emptyList（Delete 的 newRanges 为空），
        // 不含 [2,4) → 重影/跳行。
        val hiddenRanges = state.hiddenRanges.value
        assertTrue(
            "hiddenRanges 应包含 retained move 的 newRange [2,4)，实际 hiddenRanges=$hiddenRanges\n" +
                "#684 评论 5663862982 Bug1：overlay 画 retainedMoves 但 hiddenRanges 不含其 newRange，" +
                "BasicTextField 在最终新位置的那份正文同时可见 → 重影/跳行",
            hiddenRanges.any { it.start == 2 && it.end == 4 },
        )
    }

    /**
     * Bug 2 复现：多笔 intent 合成一个屏幕事务时，startFrame 应按整条 chain 的 composedOffsetMap 映射。
     *
     * 场景：
     * - 事务 A：Insert "abcdefgh"（A.newRanges=[0,8)，surviving slice targetRange=[0,8) in T0）。
     * - 事务 B chain：两笔连续 intent
     *   - intent1 (T0="abcdefgh" -> T1="abefgh")：删除 old[2,4)="cd"，
     *     replaceBounds1=(2,4,2,2)，offsetMap1: "ab" IDENTITY [0,0,2], "efgh" SHIFTED [4,2,4]。
     *   - intent2 (T1="abefgh" -> T2="XYabefgh")：在 0 前插入 "XY"，
     *     replaceBounds2=(0,0,0,2)，offsetMap2: "abefgh" SHIFTED [0,2,6]。
     * - composed map (T0->T2): "ab" T0[0,2)->T2[2,4) SHIFTED, "efgh" T0[4,8)->T2[4,8) SHIFTED。
     *
     * A 的 surviving slice targetRange=[0,8) in T0。正确 startFrame 应按 composed map 切：
     * "ab" [0,2)->[2,4) surviving, "cd" [2,4) fading, "efgh" [4,8)->[4,8) surviving。
     *
     * 当前代码用 lastIntent.replaceBounds=replaceBounds2=(0,0,0,2)（T1->T2 坐标）切 [0,8)（T0 坐标）：
     * 整段 [0,8) 当 suffix 平移 delta=2 → [2,10)。坐标系错！
     */
    @Test
    fun multiIntentChain_startFrame_usesComposedOffsetMap_bug2() {
        val layouts = captureLayouts("", "abcdefgh", "abefgh", "XYabefgh")
        val state = ComposeEditorVisualState(targetId = "test-target")

        // === 生成事务 A（Insert "abcdefgh"）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        val intentA =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 8)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 8),
                expectedOldText = "",
                expectedNewText = "abcdefgh",
            )
        state.onVisualIntent(
            intentA,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(8, 8), 0)
        val txA = state.activeTransaction.value
        assertNotNull("事务 A 应生成", txA)

        // A 跑到 progress=0.5（surviving slice alpha=0.5，物化 B 时 startFrame 非 null）。
        state.reportProgress(0.5f)

        // === 生成事务 B（两笔 intent chain T0->T1->T2）===
        // intent1: T0="abcdefgh" -> T1="abefgh"（删除 old[2,4)="cd"）
        val intent1 =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY), // "ab"
                                VisualOffsetMapEntry(4, 2, 4, VisualOffsetMapKind.SHIFTED), // "efgh"
                            ),
                    ),
                oldRanges = listOf(TextRange(2, 4)), // 删除的 "cd"
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 2, oldEnd = 4, newStart = 2, newEnd = 2),
                expectedOldText = "abcdefgh",
                expectedNewText = "abefgh",
            )
        state.onVisualIntent(
            intent1,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // intent2: T1="abefgh" -> T2="XYabefgh"（在 0 前插入 "XY"）
        val intent2 =
            EditorVisualIntent(
                coreTransactionId = 3L,
                baseRevision = 2L,
                newRevision = 3L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                VisualOffsetMapEntry(0, 2, 6, VisualOffsetMapKind.SHIFTED), // "abefgh" 前移
                            ),
                    ),
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 2)), // 插入的 "XY"
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 2),
                expectedOldText = "abefgh",
                expectedNewText = "XYabefgh",
            )
        state.onVisualIntent(
            intent2,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // 新 layout "XYabefgh" 到达 → 生成事务 B（chain=[intent1, intent2]）。
        state.onAuthoritativeLayout(layouts[3], TextRange(2, 2), 0)

        val txB = state.activeTransaction.value
        assertNotNull("事务 B 应生成", txB)

        val startFrame = txB?.startFrame
        assertNotNull(
            "B.startFrame 应非 null（从 A 在 progress=0.5 物化）",
            startFrame,
        )

        val slices = startFrame?.slices ?: emptyList()

        // 核心断言：startFrame slices 的 targetRange 应按整条 chain 的 composedOffsetMap（T0->T2）映射。
        // composed map: "ab" T0[0,2)->T2[2,4), "efgh" T0[4,8)->T2[4,8)。
        // A 的 surviving slice targetRange=[0,8) in T0 应被切成：
        //   [0,2)->[2,4) surviving, [2,4) fading, [4,8)->[4,8) surviving。
        assertTrue(
            "startFrame slices 应含按 composed map 映射的 [2,4)（'ab' T0[0,2)->T2[2,4)），" +
                "实际 slices targetRanges=${slices.map { it.targetRange }}\n" +
                "#684 评论 5663862982 Bug2：当前代码用最后一笔 replaceBounds=(0,0,0,2)（T1->T2 坐标）" +
                "切 T0 坐标 [0,8)，整段当 suffix 平移到 [2,10)，坐标系错",
            slices.any { it.targetRange == TextRange(2, 4) },
        )

        // 当前代码错误产物：整段 [0,8) 平移到 [2,10)。
        assertFalse(
            "startFrame slices 不应含整段 [2,10)（错坐标系产物），" +
                "实际 slices targetRanges=${slices.map { it.targetRange }}",
            slices.any { it.targetRange == TextRange(2, 10) },
        )
    }

    /**
     * 用 [rememberTextMeasurer] 在 Compose 测试环境里构造真实 [TextLayoutResult]。
     * 一次 setContent 构造多份 layout，供测试里多代 rebase 使用。
     * maxWidth=1000 避免折行，让 bounds 计算确定；硬换行 `\n` 一定产生多行布局。
     */
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
