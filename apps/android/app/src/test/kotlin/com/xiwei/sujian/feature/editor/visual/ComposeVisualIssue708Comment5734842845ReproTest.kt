package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #708 评论 5734842845 — `publishLocalHandoffScene` 只重建 `unitClipFractions`，
 * 没有重建 `unitClipCursors` 的回归测试（修复后断言反映正确行为）。
 *
 * **问题**：
 * `publishLocalHandoffScene()` rebase 完以后只做：
 * ```
 * scene.copy(
 *     hiddenRanges = mergedHidden,
 *     units = rebasedUnits,
 *     cursorRect = ... ,
 *     unitClipFractions = rebasedClipFractions,
 *     coordinatedSpatialClip = ... ,
 * )
 * ```
 * 没有更新 `unitClipCursors`。
 *
 * 这意味着：handoff 一旦 split parent 并给 child 分配新 key，child 有了正确的新 fraction，
 * 但 `scene.unitClipCursors` 仍然只有旧 parent key。
 *
 * **复现场景**（连续两次 split，中间不要 drain timeline）：
 * 1. 第一笔：`"" -> "abcdefghi"`，drain + sample 到约 50ms，
 *    parent [0,9) fraction≈0.5，parentKey=P，`unitClipCursors[P]=cursor1`
 * 2. 第二笔 handoff（不 drain）：`"abcdefghi" -> "abcdefgh"`，删尾部 [8,9)，
 *    parent 被 split：surviving child [0,8) key=C，ghost [8,9)。
 *    handoff 已正确算出 `unitClipFractions[C]≈0.56`，但当前代码没有写
 *    `unitClipCursors[C]=cursor1`，scene 里仍只有旧的 `unitClipCursors[P]`。
 * 3. 第三笔 handoff（仍不 drain）：`"abcdefgh" -> "abcdfgh"`，删中间 [4,5)，
 *    现在要再次 split child C。
 *
 * `ComposeLocalHandoffRebase.rebase()` 第二次处理 C 时：
 * ```
 * val parentOldFraction = scene.unitClipFractions[C]   // 有，约 0.56
 * val parentOldCursorRect = scene.unitClipCursors[C]  // null
 * ```
 * 于是 `computeSliceInitialFraction()` 走：
 * ```
 * if (parentOldCursorRect == null) {
 *     return parentOldFraction
 * }
 * ```
 * 结果第二次 split 的 front surviving / ghost / back surviving 全部继承同一个 0.56。
 *
 * **测试 J1**（修复后断言）：验证第二笔 handoff 后 `scene.unitClipCursors` 包含 surviving child C 的 key。
 * **测试 J2**（修复后断言）：验证第三笔 handoff 后 front fraction > 0.8（cursor 已越过）、
 *   back fraction < 0.2（cursor 还没到），且 front/back 不都等于第一笔 surviving child C 的 parent fraction。
 *
 * **注意**：本测试在 Phase A 复现阶段断言 bug 存在（assertFalse/assertEquals），Phase B 修复后
 * 已更新断言为正确行为（assertTrue front>0.8 back<0.2），现在作为回归测试守护修复。
 */
@Suppress("LongMethod", "MaxLineLength", "LargeClass", "StringLiteralDuplication", "TooManyFunctions")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue708Comment5734842845ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 测试 J1：第二笔 handoff 后 unitClipCursors 缺失 child C 的 key ====================

    /**
     * 测试 J1：第二笔 handoff split 后 `scene.unitClipCursors` 包含 surviving child C 的 key —
     *
     * #708 评论 5734842845：
     * `publishLocalHandoffScene()` 的 `scene.copy(...)` 只更新 `unitClipFractions`，
     * 没有更新 `unitClipCursors`。handoff split parent 并给 child 分配新 key C 后，
     * `unitClipFractions[C]` 已正确写入，但 `unitClipCursors` 仍然只有旧 parent key P。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "abcdefghi"`（插入 9 字符），drain + sample 到 50ms
     * 2. 第二笔 handoff（不 drain）：`"abcdefghi" -> "abcdefgh"`（删尾部 [8,9)），parent split
     * 3. 在 timeline drain 之前检查 handoff scene
     *
     * 断言（修复后的正确行为）：
     * - handoff scene 存在 surviving child C（targetRange=[0,8)）
     * - `unitClipFractions[C]` 已写入（rebase 已正确算出 child fraction）
     * - `unitClipCursors[C]` 存在（publishLocalHandoffScene 已同步发布 unitClipCursors — 修复后）
     */
    @Test
    fun testJ1_secondHandoffSplit_unitClipCursorsMissingForChildC() {
        val layouts =
            captureLayoutsWithWidth(
                arrayOf("", MULTI_CHAR_TEXT, "abcdefgh"),
                1000,
            )
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5734842845-J1",
                classifier = FakeLocalVisualPlanClassifier,
            )
        // Issue #720 评论 5747339452：用非 null intent 绕过本地 reflow 释放门控，
        // 验证 rebase/split 机制（Robolectric bounds 跨文本不稳定导致误释放）。
        state.localInputIntentOverride = nonLocalIntent(1L)

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "abcdefghi"（插入 9 字符，多字符 unit [0,9)，clipTrackId=track1）
        state.recordLocalInput(
            oldText = "",
            newText = MULTI_CHAR_TEXT,
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(9, 9),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 9), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(9, 9), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到 50ms（parent fraction 约 0.5，track1 cursor 在 parent 中间约位置 4.5）
        val scene50 = state.sampleVisualScene(50L * NANOS_PER_MS)
        val parentUnit = scene50.units.firstOrNull { it.targetRange == TextRange(0, 9) }
        assertNotNull(
            "testJ1: 前置 — 应存在 [0,9) parent unit",
            parentUnit,
        )
        val parentKey = parentUnit!!.key
        val parentFraction = scene50.unitClipFractions[parentKey] ?: 0f
        assertTrue(
            "testJ1: 前置 — 50ms 时 parent fraction 应在 (0.3, 0.7) 之间，实际=$parentFraction",
            parentFraction > 0.3f && parentFraction < 0.7f,
        )
        // 前置 — timeline.sample 已正确写入 unitClipCursors[P]
        assertNotNull(
            "testJ1: 前置 — 50ms 时 scene.unitClipCursors[parentKey] 应非 null" +
                "（timeline.sample 已正确产出 unitClipCursors）",
            scene50.unitClipCursors[parentKey],
        )

        // 第二笔 handoff（不 drain）："abcdefghi" -> "abcdefgh"（删尾部 [8,9)）
        // offsetMap: old [0,8) → new [0,8) surviving, old [8,9) ghost
        // parent [0,9) 被 split：surviving child [0,8) key=C, ghost [8,9)
        state.recordLocalInput(
            oldText = MULTI_CHAR_TEXT,
            newText = "abcdefgh",
            oldSelection = TextRange(9, 9),
            newSelection = TextRange(8, 8),
            changes = listOf(LocalInputChange(newRange = TextRange(8, 8), oldRange = TextRange(8, 9))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(8, 8), 0)

        // 在 timeline drain 之前检查 handoff scene — 这是 publishLocalHandoffScene 建立的 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 找到 surviving child C（targetRange=[0,8)）
        val childC =
            handoffScene.units.firstOrNull { it.targetRange == TextRange(0, 8) }
        assertNotNull(
            "testJ1: handoff 应存在 targetRange=[0,8) 的 surviving child C，" +
                "实际 units.targetRanges=${handoffScene.units.mapNotNull { it.targetRange }}",
            childC,
        )
        val childKeyC = childC!!.key

        // 前置 — rebase 已正确算出 child C 的 fraction（写入 unitClipFractions）
        val childFractionC = handoffScene.unitClipFractions[childKeyC]
        assertNotNull(
            "testJ1: 前置 — handoff scene.unitClipFractions[childKeyC] 应非 null" +
                "（rebase 已正确算出 child fraction），实际 childKeyC=$childKeyC," +
                " unitClipFractions=${handoffScene.unitClipFractions.keys}",
            childFractionC,
        )

        // 核心断言（修复后的正确行为）：handoff scene.unitClipCursors 包含 child C 的 key
        // publishLocalHandoffScene 的 scene.copy(...) 已同步发布 unitClipCursors = rebased.initialClipCursorsByKey，
        // rebase 中 parent 有 scene.unitClipCursors[parentKey] 时所有派生 child key 记录同一份 parent clip cursor。
        assertTrue(
            "testJ1: 【修复后】handoff scene.unitClipCursors 应包含 surviving child C 的 key" +
                "（publishLocalHandoffScene 已同步发布 unitClipCursors），" +
                "实际 childKeyC=$childKeyC in unitClipCursors=${handoffScene.unitClipCursors.keys}",
            childKeyC in handoffScene.unitClipCursors,
        )
    }

    // ==================== 测试 J2：第三笔 handoff 后 front/back fraction 都等于 parent fraction ====================

    /**
     * 测试 J2：第三笔 handoff 再次 split child C 时，front surviving fraction > 0.8（cursor 已越过）、
     * back surviving fraction < 0.2（cursor 还没到），且 front/back 不都等于 child C 的 parent fraction —
     *
     * #708 评论 5734842845：
     * 第二笔 handoff 没写 `unitClipCursors[C]`，第三笔 handoff 再次 split C 时：
     * ```
     * val parentOldCursorRect = scene.unitClipCursors[C]  // null
     * ```
     * `computeSliceInitialFraction()` 走 `if (parentOldCursorRect == null) return parentOldFraction`，
     * front surviving / ghost / back surviving 全部继承同一个 parentOldFraction（约 0.56）。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "abcdefghi"`，drain + sample 到 50ms
     * 2. 第二笔 handoff（不 drain）：`"abcdefghi" -> "abcdefgh"`（删尾部 [8,9)），parent split → child C
     * 3. 第三笔 handoff（仍不 drain）：`"abcdefgh" -> "abcdfgh"`（删中间 [4,5)），再次 split child C
     * 4. 在 timeline drain 之前检查 handoff scene
     *
     * 断言（修复后的正确行为）：
     * - 第三笔 handoff 后存在 front surviving [0,4) 和 back surviving [4,7)
     * - front surviving fraction > 0.8（cursor 已越过 front [0,4)）
     * - back surviving fraction < 0.2（cursor 还没到 back [4,7)）
     * - front/back 不能都等于第一笔 surviving child C 的 parent fraction
     */
    @Test
    fun testJ2_thirdHandoffSplit_frontBackFractionBothEqualParentFraction() {
        val layouts =
            captureLayoutsWithWidth(
                arrayOf("", MULTI_CHAR_TEXT, "abcdefgh", "abcdfgh"),
                1000,
            )
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5734842845-J2",
                classifier = FakeLocalVisualPlanClassifier,
            )
        // Issue #720 评论 5747339452：用非 null intent 绕过本地 reflow 释放门控，
        // 验证 rebase/split 机制（Robolectric bounds 跨文本不稳定导致误释放）。
        state.localInputIntentOverride = nonLocalIntent(1L)

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "abcdefghi"（插入 9 字符，多字符 unit [0,9)，clipTrackId=track1）
        state.recordLocalInput(
            oldText = "",
            newText = MULTI_CHAR_TEXT,
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(9, 9),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 9), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(9, 9), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到 50ms（parent fraction 约 0.5）
        val scene50 = state.sampleVisualScene(50L * NANOS_PER_MS)
        val parentUnit = scene50.units.firstOrNull { it.targetRange == TextRange(0, 9) }
        assertNotNull(
            "testJ2: 前置 — 应存在 [0,9) parent unit",
            parentUnit,
        )
        val parentFraction = scene50.unitClipFractions[parentUnit!!.key] ?: 0f
        assertTrue(
            "testJ2: 前置 — 50ms 时 parent fraction 应在 (0.3, 0.7) 之间，实际=$parentFraction",
            parentFraction > 0.3f && parentFraction < 0.7f,
        )

        // 第二笔 handoff（不 drain）："abcdefghi" -> "abcdefgh"（删尾部 [8,9)）
        // parent [0,9) split：surviving child [0,8) key=C, ghost [8,9)
        state.recordLocalInput(
            oldText = MULTI_CHAR_TEXT,
            newText = "abcdefgh",
            oldSelection = TextRange(9, 9),
            newSelection = TextRange(8, 8),
            changes = listOf(LocalInputChange(newRange = TextRange(8, 8), oldRange = TextRange(8, 9))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(8, 8), 0)

        // 确认第二笔后 child C 已建立，记录其 fraction（将作为第三笔的 parentOldFraction）
        val sceneAfterSecond = state.drawSnapshot().scene
        val childC =
            sceneAfterSecond.units.firstOrNull { it.targetRange == TextRange(0, 8) }
        assertNotNull(
            "testJ2: 前置 — 第二笔后应存在 targetRange=[0,8) 的 surviving child C，" +
                "实际 units.targetRanges=${sceneAfterSecond.units.mapNotNull { it.targetRange }}",
            childC,
        )
        val childKeyC = childC!!.key
        val childFractionC = sceneAfterSecond.unitClipFractions[childKeyC]
        assertNotNull(
            "testJ2: 前置 — 第二笔后 unitClipFractions[childKeyC] 应非 null，" +
                "实际 childKeyC=$childKeyC, unitClipFractions=${sceneAfterSecond.unitClipFractions.keys}",
            childFractionC,
        )
        // 第二笔后 unitClipCursors 含 C（修复后）— 与 testJ1 一致
        assertTrue(
            "testJ2: 前置 — 第二笔后 unitClipCursors 应含 child C（修复后），" +
                "实际 childKeyC=$childKeyC in unitClipCursors=${sceneAfterSecond.unitClipCursors.keys}",
            childKeyC in sceneAfterSecond.unitClipCursors,
        )

        // 第三笔 handoff（仍不 drain）："abcdefgh" -> "abcdfgh"（删中间 [4,5)）
        // offsetMap: old [0,4) → new [0,4) surviving, old [4,5) ghost,
        //            old [5,8) → new [4,7) surviving
        // child C [0,8) 再次 split：front surviving [0,4), ghost [4,5), back surviving [4,7)（new 坐标）
        state.recordLocalInput(
            oldText = "abcdefgh",
            newText = "abcdfgh",
            oldSelection = TextRange(5, 5),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(4, 4), oldRange = TextRange(4, 5))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(4, 4), 0)

        // 在 timeline drain 之前检查 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 找到 front surviving [0,4) 和 back surviving [4,7)
        val frontSurviving =
            handoffScene.units.firstOrNull { it.targetRange == TextRange(0, 4) }
        val backSurviving =
            handoffScene.units.firstOrNull { it.targetRange == TextRange(4, 7) }
        assertNotNull(
            "testJ2: handoff 应存在 targetRange=[0,4) 的 front surviving，" +
                "实际 units.targetRanges=${handoffScene.units.mapNotNull { it.targetRange }}",
            frontSurviving,
        )
        assertNotNull(
            "testJ2: handoff 应存在 targetRange=[4,7) 的 back surviving，" +
                "实际 units.targetRanges=${handoffScene.units.mapNotNull { it.targetRange }}",
            backSurviving,
        )

        val frontFraction = handoffScene.unitClipFractions[frontSurviving!!.key]
        val backFraction = handoffScene.unitClipFractions[backSurviving!!.key]

        // 前置 — front/back fraction 都应非 null（rebase 已写入）
        assertNotNull(
            "testJ2: 前置 — front surviving fraction 应非 null，" +
                "实际 key=${frontSurviving.key}, unitClipFractions=${handoffScene.unitClipFractions.keys}",
            frontFraction,
        )
        assertNotNull(
            "testJ2: 前置 — back surviving fraction 应非 null，" +
                "实际 key=${backSurviving.key}, unitClipFractions=${handoffScene.unitClipFractions.keys}",
            backFraction,
        )

        // 核心断言1（修复后的正确行为）：front surviving fraction > 0.8（cursor 已越过 front [0,4)）
        // rebase 处理 C 时 parentOldCursorRect = scene.unitClipCursors[C] 非 null（第二笔已写入），
        // computeSliceInitialFraction 走精确算分支，front [0,4) 在 cursor 之前 → fraction≈1。
        assertTrue(
            "testJ2: 【修复后】front surviving fraction ($frontFraction) 应 > 0.8" +
                "（cursor 已越过 front [0,4)，parentOldCursorRect 非 null 走精确算分支）" +
                "（旧 bug：parentOldCursorRect==null 导致全部继承 parentOldFraction≈$childFractionC）",
            frontFraction!! > 0.8f,
        )

        // 核心断言2（修复后的正确行为）：back surviving fraction < 0.2（cursor 还没到 back [4,7)）
        assertTrue(
            "testJ2: 【修复后】back surviving fraction ($backFraction) 应 < 0.2" +
                "（cursor 还没到 back [4,7)，parentOldCursorRect 非 null 走精确算分支）" +
                "（旧 bug：parentOldCursorRect==null 导致全部继承 parentOldFraction≈$childFractionC）",
            backFraction!! < 0.2f,
        )

        // 核心断言3（修复后的正确行为）：front fraction != back fraction
        // front ≈ 1, back ≈ 0，两者应显著不同 — 不再是"三段文字拿同一个 fraction"的回归。
        val frontBackDelta = kotlin.math.abs(frontFraction - backFraction)
        assertTrue(
            "testJ2: 【修复后】|front - back| = $frontBackDelta 应 > 0.5" +
                "（front≈1, back≈0，parentOldCursorRect 非 null 走精确算分支）" +
                "（旧 bug：三段拿同一 fraction，|front - back| < 0.3）",
            frontBackDelta > 0.5f,
        )

        // 核心断言4（修复后的正确行为）：front/back 不能都等于第一笔 surviving child C 的 parent fraction
        assertTrue(
            "testJ2: 【修复后】front ($frontFraction) 和 back ($backFraction) 不能都等于" +
                " child C 的 parent fraction ($childFractionC)" +
                "（parentOldCursorRect 非 null 走精确算分支，front≈1 back≈0 都不等于 parentFraction≈0.56）" +
                "（旧 bug：parentOldCursorRect==null 导致 front/back 都继承 parentOldFraction）",
            kotlin.math.abs(frontFraction - childFractionC!!) > 0.1f ||
                kotlin.math.abs(backFraction - childFractionC) > 0.1f,
        )
    }

    // ==================== 辅助方法 ====================

    /**
     * Issue #720 评论 5747339452：构造非 null intent 绕过本地 reflow 释放门控 —
     * Robolectric 下 [TextLayoutResult.getPathForRange] 跨文本 bounds 不稳定
     * （同 range 在不同文本中 left/right 不同），导致 [ComposeVisualRebase.naturalGeometryChanged]
     * 误判为几何变化、survivor 被误释放。本测试验证的是 rebase/split 机制（非 #720 释放），
     * 用非 null intent 绕过释放门控。
     */
    private fun nonLocalIntent(id: Long): EditorVisualIntent =
        EditorVisualIntent(
            coreTransactionId = id,
            baseRevision = 0L,
            newRevision = id,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = null,
            oldRanges = emptyList(),
            newRanges = emptyList(),
            textKind = TextVisualKind.None,
            cursor = null,
        )

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L

        /** 多字符文本（9 字符触发 RUN_ANIMATION 产生多字符 unit）。 */
        const val MULTI_CHAR_TEXT: String = "abcdefghi"
    }

    private fun captureLayoutsWithWidth(
        texts: Array<out String>,
        maxWidth: Int,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> =
        captureLayoutsWithMultipleWidths(
            *texts.map { it to maxWidth }.toTypedArray(),
            fontSizeSp = fontSizeSp,
        )

    /**
     * 一次 setContent 内捕获多种宽度的 layout —
     * composeRule.setContent 每个测试只能调用一次，
     * 需要不同宽度 layout 的测试用本方法一次取齐。
     */
    private fun captureLayoutsWithMultipleWidths(
        vararg textWidthPairs: Pair<String, Int>,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            for ((text, width) in textWidthPairs) {
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = fontSizeSp.sp),
                        constraints = Constraints(maxWidth = width),
                    ),
                )
            }
        }
        return results
    }
}
