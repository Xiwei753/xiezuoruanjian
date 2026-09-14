package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #684 评论 5670941608 复现测试 — oldAnimationUnits ownership subtraction。
 *
 * 根因：[ComposeVisualFrameCoordinator] 中 `startFrame.ownedOldRanges` 只从
 * `mergedOldRanges` 里扣掉（生成 `effectiveOldRanges`），却没有同步从
 * `oldAnimationUnits` 里扣掉。这会导致"上一笔动画尚未结束，下一笔马上删除/替换
 * 刚才那段文字"时，同一段旧文字同时由 startFrame fading slice 和 unit-delete
 * 两条路径绘制，出现重影。
 *
 * 修复：让 oldAnimationUnits 与 effectiveOldRanges 使用同一个
 * `startFrame.ownedOldRanges` ownership subtraction。
 *
 * 场景1（完全覆盖）：Backspace 删除刚插入的字符 — B.oldAnimationUnits 必须为空。
 * 场景2（部分覆盖）：多 unit 中只有一个被下一笔替换 — 验证 subtractRanges 保留剩余片段，
 *   且 state 驱动下 B.oldAnimationUnits 只扣除被 startFrame 接管的部分。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualOldAnimationUnitsOwnershipReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 场景1：完全覆盖 — Backspace 删除刚插入的字符 ====================

    /**
     * 场景1：A 插入 "x" 跑到中途（progress=0.5），B 立刻 Backspace 删除 "x"。
     *
     * 预期：
     * - B.startFrame 非 null（从 A 物化半途动画）。
     * - B.startFrame.ownedOldRanges 包含 [0,1)（A 的插入字符被 B 的 startFrame 接管）。
     * - B.oldRanges（effectiveOldRanges）为空（已扣除 ownedOldRanges）。
     * - B.oldAnimationUnits 也必须为空（核心断言：oldAnimationUnits 也扣除 ownedOldRanges，
     *   否则同一段文字被 startFrame fading slice 和 unit-delete 双重绘制 → 重影）。
     */
    @Test
    fun fullCoverage_backspace_delete_just_inserted_char_oldAnimationUnits_empty() {
        val layouts = captureLayouts("", "x", "")
        val state = ComposeEditorVisualState(targetId = "test-target-old-units-ownership-1")

        // === 基线 layout "" 到达 ===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // === 生成事务 A（CLUSTER_ANIMATION Insert "" → "x"）===
        val intentA =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 1),
                expectedOldText = "",
                expectedNewText = "x",
                oldAnimationUnits = emptyList(),
                newAnimationUnits = listOf(TextRange(0, 1)),
            )
        state.onVisualIntent(
            intentA,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        val txA = state.activeTransaction.value
        assertNotNull("事务 A 应生成", txA)

        // A 跑到 progress=0.5（动画只跑到中途）。
        state.reportProgress(state.activeTransaction.value?.id ?: 0L, 0.5f)

        // === 生成事务 B（CLUSTER_ANIMATION Delete "x" → ""）===
        // offsetMap 用空 entries 表示整段删除（无存活映射）。
        val intentB =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = VisualOffsetMap(entries = emptyList()),
                oldRanges = listOf(TextRange(0, 1)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 1, newStart = 0, newEnd = 0),
                expectedOldText = "x",
                expectedNewText = "",
                oldAnimationUnits = listOf(TextRange(0, 1)),
                newAnimationUnits = emptyList(),
            )
        state.onVisualIntent(
            intentB,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(0, 0), 0)

        val txB = state.activeTransaction.value
        assertNotNull("事务 B 应生成", txB)

        // B.startFrame 应非 null（从 A 物化半途动画）。
        val startFrameB = txB?.startFrame
        assertNotNull(
            "B.startFrame 应非 null（从 A 物化半途动画），实际=${startFrameB}",
            startFrameB,
        )

        // B.startFrame.ownedOldRanges 应包含 [0,1)（A 的插入字符被 B 的 startFrame 接管）。
        val ownedOldRangesB = startFrameB?.ownedOldRanges.orEmpty()
        assertTrue(
            "B.startFrame.ownedOldRanges 应包含 [0,1)，实际=$ownedOldRangesB\n" +
                "A 的插入字符 'x' 应被 B 的 startFrame 接管",
            ownedOldRangesB.contains(TextRange(0, 1)),
        )

        // B.oldRanges（effectiveOldRanges）应为空（已扣除 ownedOldRanges）。
        assertTrue(
            "B.oldRanges（effectiveOldRanges）应为空（已扣除 ownedOldRanges），实际=${txB?.oldRanges}",
            txB?.oldRanges.isNullOrEmpty(),
        )

        // 核心断言：B.oldAnimationUnits 也必须为空。
        // 修复前：oldAnimationUnits 未扣除 ownedOldRanges，仍为 [TextRange(0,1)]，
        // 同一段文字 'x' 同时由 startFrame fading slice 和 unit-delete 路径绘制 → 重影。
        // 修复后：oldAnimationUnits 也扣除 ownedOldRanges，为空，'x' 只由 startFrame 画一次。
        assertTrue(
            "B.oldAnimationUnits 必须为空（已扣除 startFrame.ownedOldRanges），实际=${txB?.oldAnimationUnits}\n" +
                "#684 评论 5670941608：oldAnimationUnits 必须与 effectiveOldRanges 使用同一个 " +
                "ownership subtraction，否则同一段旧文字被 startFrame 和 unit-delete 双重绘制 → 重影",
            txB?.oldAnimationUnits.isNullOrEmpty(),
        )
    }

    // ==================== 场景2：部分覆盖 — 多 unit 中只有一个被下一笔替换 ====================

    /**
     * 场景2 纯函数断言：subtractRanges 对部分覆盖保留剩余片段。
     *
     * blocker [0,1) 只覆盖 unit [0,2) 的一部分，subtraction 后应保留 [1,2)。
     * 这验证了 effectiveOldAnimationUnits 的 flatMap 逻辑会保留剩余片段，
     * 保持 Core 的动画粒度，不把整个 unit 丢弃。
     */
    @Test
    fun partialCoverage_subtractRanges_keeps_remaining_fragment() {
        // unit [0,2) 被 blocker [0,1) 部分覆盖，应保留 [1,2)。
        val result =
            ComposeVisualRebase.subtractRanges(
                candidates = listOf(TextRange(0, 2)),
                blockers = listOf(TextRange(0, 1)),
            )
        assertEquals(
            "subtractRanges([0,2), [0,1)) 应保留剩余片段 [1,2)，实际=$result\n" +
                "#684 评论 5670941608：blocker 只覆盖 unit 一部分时，保留 subtraction 后剩下的片段，" +
                "保持 Core 的动画粒度",
            listOf(TextRange(1, 2)),
            result,
        )

        // 多 unit 场景：[0,2) 和 [3,5)，blocker [0,1) 只覆盖第一个 unit 的一部分。
        // 第一个 unit 保留 [1,2)，第二个 unit 完整保留 [3,5)。
        val multiUnitResult =
            ComposeVisualRebase.subtractRanges(
                candidates = listOf(TextRange(0, 2), TextRange(3, 5)),
                blockers = listOf(TextRange(0, 1)),
            )
        assertEquals(
            "多 unit 部分覆盖：[0,2) 和 [3,5) 扣除 [0,1) 应得 [1,2) 和 [3,5)，实际=$multiUnitResult",
            listOf(TextRange(1, 2), TextRange(3, 5)),
            multiUnitResult,
        )

        // 完全覆盖：unit [0,1) 被 blocker [0,1) 完全覆盖，结果为空。
        val fullCoveredResult =
            ComposeVisualRebase.subtractRanges(
                candidates = listOf(TextRange(0, 1)),
                blockers = listOf(TextRange(0, 1)),
            )
        assertTrue(
            "完全覆盖：[0,1) 扣除 [0,1) 应为空，实际=$fullCoveredResult",
            fullCoveredResult.isEmpty(),
        )
    }

    /**
     * 场景2 state 驱动断言：A 插入 "ab"（两个 unit）跑到中途，B 删除第一个字符 'a'。
     *
     * 预期：
     * - B.startFrame.ownedOldRanges 包含 [0,1)（'a' 被 startFrame 接管）。
     * - B.oldAnimationUnits 为空（composedOldAnimationUnits=[0,1)，扣除 ownedOldRanges=[0,1) 后为空）。
     *   'a' 只由 startFrame 画一次，不被 unit-delete 双重绘制。
     */
    @Test
    fun partialCoverage_state_driven_oldAnimationUnits_only_subtracts_owned_part() {
        val layouts = captureLayouts("", "ab", "b")
        val state = ComposeEditorVisualState(targetId = "test-target-old-units-ownership-2")

        // === 基线 layout "" 到达 ===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // === 生成事务 A（CLUSTER_ANIMATION Insert "" → "ab"，两个 unit）===
        val intentA =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 2)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 2),
                expectedOldText = "",
                expectedNewText = "ab",
                oldAnimationUnits = emptyList(),
                newAnimationUnits = listOf(TextRange(0, 1), TextRange(1, 2)),
            )
        state.onVisualIntent(
            intentA,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)
        val txA = state.activeTransaction.value
        assertNotNull("事务 A 应生成", txA)

        // A 跑到 progress=0.5（动画只跑到中途）。
        state.reportProgress(state.activeTransaction.value?.id ?: 0L, 0.5f)

        // === 生成事务 B（CLUSTER_ANIMATION Delete "ab" → "b"，删除第一个字符 'a'）===
        // 'b' 从 old offset 1 映射到 new offset 0。
        val intentB =
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
                                VisualOffsetMapEntry(
                                    oldStart = 1,
                                    newStart = 0,
                                    length = 1,
                                    kind = VisualOffsetMapKind.IDENTITY,
                                ),
                            ),
                    ),
                oldRanges = listOf(TextRange(0, 1)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 1, newStart = 0, newEnd = 0),
                expectedOldText = "ab",
                expectedNewText = "b",
                oldAnimationUnits = listOf(TextRange(0, 1)),
                newAnimationUnits = emptyList(),
            )
        state.onVisualIntent(
            intentB,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0)

        val txB = state.activeTransaction.value
        assertNotNull("事务 B 应生成", txB)

        // B.startFrame 应非 null（从 A 物化半途动画）。
        val startFrameB = txB?.startFrame
        assertNotNull(
            "B.startFrame 应非 null（从 A 物化半途动画），实际=${startFrameB}",
            startFrameB,
        )

        // B.startFrame.ownedOldRanges 应包含 [0,1)（'a' 被 startFrame 接管）。
        val ownedOldRangesB = startFrameB?.ownedOldRanges.orEmpty()
        assertTrue(
            "B.startFrame.ownedOldRanges 应包含 [0,1)（'a' 被 startFrame 接管），实际=$ownedOldRangesB",
            ownedOldRangesB.contains(TextRange(0, 1)),
        )

        // 核心断言：B.oldAnimationUnits 不应包含 [0,1)（被扣除）。
        // composedOldAnimationUnits=[TextRange(0,1)]，扣除 ownedOldRanges=[0,1) 后应为空。
        // 'a' 只由 startFrame 画一次，不被 unit-delete 双重绘制。
        val oldAnimationUnitsB = txB?.oldAnimationUnits.orEmpty()
        assertTrue(
            "B.oldAnimationUnits 不应包含 [0,1)（已被 startFrame.ownedOldRanges 扣除），实际=$oldAnimationUnitsB\n" +
                "#684 评论 5670941608：oldAnimationUnits 必须扣除被 startFrame 接管的部分，" +
                "否则 'a' 被 startFrame 和 unit-delete 双重绘制 → 重影",
            !oldAnimationUnitsB.contains(TextRange(0, 1)),
        )
    }

    // ==================== 辅助方法 ====================

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
