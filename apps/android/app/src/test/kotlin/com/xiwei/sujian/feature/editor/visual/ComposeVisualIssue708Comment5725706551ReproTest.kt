package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #708 评论 5725706551 — `publishLocalHandoffScene()` scene rebase 修复的验证测试。
 *
 * **修复内容**：`publishLocalHandoffScene()` 已重构 — 现在调用
 * [ComposeLocalHandoffRebase.rebase] 把旧 visible scene 映射到新正文坐标系，
 * 不再直接复制旧 hiddenRanges/units。这修复了快速输入/删除时的重影、旧字残留和闪烁问题。
 *
 * **核心问题**（旧 bug）：
 * 1. 旧 hiddenRanges 直接复制到新坐标系 — 坐标不匹配；
 * 2. 为 deletedUnits 新建 alpha=1 的完整 ghost — 如果旧 active unit 正在动画中
 *    （alpha 在 0..1），直接新建 alpha=1 的 ghost 会导致重影；
 * 3. 旧 active unit 没有做 rebase — 仍然在旧坐标系中。
 *
 * **修复后**：
 * 1. 旧 active unit（targetRange != null）通过 offsetMap 映射到新正文坐标；
 *    存活 slice 改 newRange/newLayout，保持当前屏幕位置；
 *    被删除 slice 从当前可见 alpha/position 转 handoff ghost（不新建 alpha=1 的完整 ghost）。
 * 2. hiddenRanges 从 rebase 后所有 targetRange != null 的 unit 重新推导
 *    （不再从旧 hiddenRanges 复制），确保 hiddenRanges 和 newLayout 属于同一个坐标系。
 * 3. deletedUnits 只补"没有被旧 active unit 接管"的部分
 *    （避免同一 glyph 同时出现 Inserted + DeletedGhost 的重影）。
 *
 * **测试 A**：active insert 前方再插入 — 验证旧 active unit 的 targetRange 被 rebase 到新坐标。
 * **测试 B**：active insert 立即删除 — 验证不出现旧 Inserted + 新 alpha=1 DeletedGhost 的重影。
 */
@Suppress("LongMethod", "MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue708Comment5725706551ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 测试 A：active insert 前方再插入 ====================

    /**
     * 测试 A：active insert 前方再插入 —
     *
     * #708 评论 5725706551：
     * 旧 bug：`publishLocalHandoffScene()` 直接复制旧 hiddenRanges/units（旧正文坐标），
     * 不做坐标映射。第二笔 "a"->"ba" 后，旧 'a' 的 targetRange 仍然是 [0,1)（旧坐标），
     * 但新正文中 [0,1) 是 'b'，[1,2) 才是 'a'。overlay 在旧位置 [0,1) 画 'a'，
     * BasicTextField 在新位置 [1,2) 也画 'a'，两个 'a' 同时可见 = 重影。
     *
     * 修复后：[ComposeLocalHandoffRebase.rebase] 把旧 'a' 的 targetRange 从 [0,1) 映射成 [1,2)，
     * hiddenRanges 包含 [1,2)（新坐标系下的 'a'）和 [0,1)（新插入的 'b'），
     * BasicTextField 不画 'a' 和 'b'，只有 overlay 画。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "a"`（插入 'a'）
     * 2. sample 到 'a' 仍 active（alpha 在 0..1 之间）
     * 3. 第二笔：`"a" -> "ba"`（在 'a' 前面插入 'b'）
     * 4. 在第二笔 timeline drain 之前检查 `drawSnapshot.scene`
     *
     * 断言：
     * - 旧 'a' 的 targetRange 已从 `[0,1)` 映射成 `[1,2)`（新正文坐标）
     * - `hiddenRanges` 包含 `[1,2)`（新坐标系下的 'a'）
     * - `hiddenRanges` 包含 `[0,1)`（新插入的 'b'）
     * - 不能同时出现 BasicTextField 新位置 'a' + overlay 旧位置 'a'（即不能有两个 unit 都画 'a'）
     */
    @Test
    fun testA_activeInsertFrontInsert_rebaseMapsOldUnitToNewCoordinates() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "a", "ba"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5725706551-A",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "a"（插入 'a'）
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )

        // onAuthoritativeLayout 配对生成 localPatch，建立首帧 scene（触发 publishLocalHandoffScene）
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        // drainPendingPatchesAtFrame 把 patch 应用到 timeline
        state.drainPendingPatchesAtFrame(0L)

        // sample 到 'a' 仍 active（10ms，alpha ≈ 0.1，在 0..1 之间）
        val sampledScene = state.sampleVisualScene(10L * NANOS_PER_MS)

        // 验证 'a' 仍 active：alpha.from 在 0..1 之间
        val unitA = sampledScene.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull(
            "testA: 第一笔后应存在 targetRange=[0,1) 的 unit（'a'）",
            unitA,
        )
        assertTrue(
            "testA: 'a' 应仍 active（alpha.from 在 0..1 之间），实际 alpha.from=${unitA!!.alpha.from}" +
                "（如果 alpha.from==1f 说明动画已完成，需要用更早的 sample 时间）",
            unitA.alpha.from > 0f && unitA.alpha.from < 1f,
        )

        // 第二笔："a" -> "ba"（在 'a' 前面插入 'b'）
        state.recordLocalInput(
            oldText = "a",
            newText = "ba",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )

        // onAuthoritativeLayout 配对生成第二笔 localPatch，建立 handoff scene（触发 publishLocalHandoffScene）
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0)

        // 在 timeline drain 之前检查 drawSnapshot.scene — 这是 publishLocalHandoffScene 建立的 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 断言1：旧 'a' 的 targetRange 已从 [0,1) 映射成 [1,2)（新正文坐标）
        // 修复后：ComposeLocalHandoffRebase.rebase 通过 offsetMap 把旧 'a' 的 [0,1) 映射到新 [1,2)
        val mappedUnit = handoffScene.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        assertNotNull(
            "testA: rebase 后应存在 targetRange=[1,2) 的 unit（旧 'a' 映射到新坐标），" +
                "实际 units.targetRanges=${handoffScene.units.mapNotNull { it.targetRange }}" +
                "（旧 bug：旧 'a' 的 targetRange 仍然是 [0,1)（旧坐标），不做 rebase）",
            mappedUnit,
        )

        // 断言2：hiddenRanges 包含 [1,2)（新坐标系下的 'a'）
        // 修复后：hiddenRanges 从 rebase 后的 targetRange 重新推导，不再从旧 hiddenRanges 复制
        assertTrue(
            "testA: hiddenRanges 应包含 [1,2)（新坐标系下的 'a'），" +
                "实际 hiddenRanges=${handoffScene.hiddenRanges}" +
                "（修复后：hiddenRanges 从 rebase 后的 targetRange 重新推导，不再从旧 hiddenRanges 复制）",
            handoffScene.hiddenRanges.any { it.start == 1 && it.end == 2 },
        )

        // 断言3：hiddenRanges 包含 [0,1)（新插入的 'b'）
        // insertedUnits 加入 hiddenRanges，让 BasicTextField 先不画新字，由 overlay 吐字
        assertTrue(
            "testA: hiddenRanges 应包含 [0,1)（新插入的 'b'），" +
                "实际 hiddenRanges=${handoffScene.hiddenRanges}" +
                "（insertedUnits 加入 hiddenRanges，让 BasicTextField 先不画新字，由 overlay 吐字）",
            handoffScene.hiddenRanges.any { it.start == 0 && it.end == 1 },
        )

        // 断言4：不能同时出现两个 unit 都画 'a'
        // 旧 bug：旧 'a' 的 targetRange=[0,1)（旧坐标），新 'a' 在 BasicTextField 的 [1,2)（新坐标）
        // overlay 在 [0,1) 画 'a'，BasicTextField 在 [1,2) 也画 'a'，两个 'a' 同时可见 = 重影
        // 修复后：旧 'a' 的 targetRange 映射成 [1,2)，[1,2) 在 hiddenRanges 中，
        // BasicTextField 不画 'a'，只有 overlay 画
        val unitsAtOldPosition = handoffScene.units.filter { it.targetRange == TextRange(0, 1) }
        assertEquals(
            "testA: 不应存在 targetRange=[0,1) 的 unit（旧 'a' 不应仍在旧坐标），" +
                "实际 unitsAtOldPosition=$unitsAtOldPosition" +
                "（旧 bug：旧 'a' 的 targetRange 仍然是 [0,1)，overlay 在旧位置画 'a'，" +
                "BasicTextField 在新位置 [1,2) 也画 'a'，两个 'a' 同时可见 = 重影）",
            0,
            unitsAtOldPosition.size,
        )
    }

    // ==================== 测试 B：active insert 立即删除 ====================

    /**
     * 测试 B：active insert 立即删除 —
     *
     * #708 评论 5725706551：
     * 旧 bug：`publishLocalHandoffScene()` 不做 scene rebase，直接复制旧 units + 为 deletedUnits
     * 新建 alpha=1 的完整 ghost。如果旧 'a' 正在做 alpha 0→1 的插入动画（当前 alpha=0.1），
     * handoff scene 中同时存在：
     * - 旧 Inserted unit（targetRange=[0,1)，alpha=0.1，仍在画 'a'）
     * - 新 DeletedGhost（targetRange=null，range=[0,1)，alpha=1.0，也在画 'a'）
     * 两个 unit 同时画同一个字 = 重影。
     *
     * 修复后：[ComposeLocalHandoffRebase.rebase] 把旧 active unit（正在动画中的 'a'）转成
     * handoff ghost，alpha 继承当前可见值（0.1），不新建 alpha=1 的完整 ghost。
     * 同一 glyph 只有一个视觉 owner。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "a"`（插入 'a'）
     * 2. sample 到 'a' alpha 在 0..1 之间
     * 3. 第二笔：`"a" -> ""`（删除 'a'）
     * 4. 在第二笔 timeline drain 之前检查 handoff scene
     *
     * 断言：
     * - 不存在一份旧 `Inserted`（alpha 在 0..1）+ 一份新的 `alpha=1` `DeletedGhost` 同时画同一个字
     * - ghost 起始 alpha 必须继承上一帧当前可见 alpha（不能是 1f）
     * - 同一 glyph 只能有一个视觉 owner
     */
    @Test
    fun testB_activeInsertImmediateDelete_noDuplicateGhost() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "a", ""), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5725706551-B",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "a"（插入 'a'）
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )

        // onAuthoritativeLayout 配对生成 localPatch，建立首帧 scene（触发 publishLocalHandoffScene）
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        // drainPendingPatchesAtFrame 把 patch 应用到 timeline
        state.drainPendingPatchesAtFrame(0L)

        // sample 到 'a' alpha 在 0..1 之间（10ms，alpha ≈ 0.1）
        val sampledScene = state.sampleVisualScene(10L * NANOS_PER_MS)

        // 验证 'a' 仍 active：alpha.from 在 0..1 之间
        val unitA = sampledScene.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull(
            "testB: 第一笔后应存在 targetRange=[0,1) 的 unit（'a'）",
            unitA,
        )
        assertTrue(
            "testB: 'a' 应仍 active（alpha.from 在 0..1 之间），实际 alpha.from=${unitA!!.alpha.from}" +
                "（如果 alpha.from==1f 说明动画已完成，需要用更早的 sample 时间）",
            unitA.alpha.from > 0f && unitA.alpha.from < 1f,
        )

        // 记录当前可见 alpha，用于后续断言 ghost 继承
        val visibleAlpha = unitA.alpha.from

        // 第二笔："a" -> ""（删除 'a'）
        state.recordLocalInput(
            oldText = "a",
            newText = "",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )

        // onAuthoritativeLayout 配对生成第二笔 localPatch，建立 handoff scene（触发 publishLocalHandoffScene）
        state.onAuthoritativeLayout(layouts[2], TextRange(0, 0), 0)

        // 在 timeline drain 之前检查 handoff scene — 这是 publishLocalHandoffScene 建立的 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 断言1：同一 glyph 只能有一个视觉 owner
        // 旧 bug：旧 Inserted unit（alpha=0.1）+ 新 DeletedGhost（alpha=1.0）同时画 'a'
        // 修复后：旧 'a' 转 handoff ghost，deletedUnits 被 ghostedCoverage 覆盖，不新建 alpha=1 ghost
        val unitsDrawingA = handoffScene.units.filter { it.range == TextRange(0, 1) }
        assertEquals(
            "testB: 同一 glyph（range=[0,1)）应只有一个视觉 owner，" +
                "实际数量=${unitsDrawingA.size}, roles=${unitsDrawingA.map { it.role }}, " +
                "alphas=${unitsDrawingA.map { it.alpha.from }}" +
                "（旧 bug：旧 Inserted + 新 alpha=1 DeletedGhost 同时画同一个字 = 重影）",
            1,
            unitsDrawingA.size,
        )

        // 断言2：ghost 起始 alpha 必须继承上一帧当前可见 alpha（不能是 1f）
        // 修复后：toHandoffGhost 把 alpha 固定在当前可见值（TimedFloat(from, from, 0, 0)）
        val ghostUnit = unitsDrawingA.first()
        assertTrue(
            "testB: ghost 起始 alpha 应继承上一帧当前可见 alpha（$visibleAlpha），不能是 1f，" +
                "实际 alpha.from=${ghostUnit.alpha.from}" +
                "（旧 bug：为 deletedUnits 新建 alpha=1 的完整 ghost，不继承当前可见 alpha）",
            ghostUnit.alpha.from < 1f,
        )
        assertEquals(
            "testB: ghost 起始 alpha 应等于上一帧当前可见 alpha（$visibleAlpha），" +
                "实际 alpha.from=${ghostUnit.alpha.from}",
            visibleAlpha,
            ghostUnit.alpha.from,
            // float comparison tolerance
            0.001f,
        )

        // 断言3：不存在旧 Inserted（alpha 在 0..1）+ 新 alpha=1 DeletedGhost 同时画同一个字
        // 修复后：deletedUnits 被 ghostedCoverage 覆盖，不新建 alpha=1 ghost
        val alphaOneGhosts =
            handoffScene.units.filter {
                it.role == VisualUnitRole.DeletedGhost && it.alpha.from >= 1f
            }
        assertTrue(
            "testB: 不应存在 alpha=1 的 DeletedGhost（旧 bug：为 deletedUnits 新建 alpha=1 的完整 ghost），" +
                "实际 alphaOneGhosts=${alphaOneGhosts.map { "range=${it.range}, alpha=${it.alpha.from}" }}" +
                "（修复后：deletedUnits 被 ghostedCoverage 覆盖，不新建 alpha=1 ghost）",
            alphaOneGhosts.isEmpty(),
        )

        // 断言4：ghost 的 targetRange 必须为 null（它是 ghost，不是 active unit）
        assertNull(
            "testB: ghost 的 targetRange 应为 null（它是 ghost，不是 active unit），" +
                "实际 targetRange=${ghostUnit.targetRange}",
            ghostUnit.targetRange,
        )
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L
    }

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> = captureLayoutsWithWidth(texts, 1000)

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
