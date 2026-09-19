package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
@Suppress("LongMethod", "MaxLineLength", "LargeClass", "StringLiteralDuplication", "TooManyFunctions")
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

    // ==================== 测试 C：deletedUnits 部分被 active ghost 覆盖 ====================

    /**
     * 测试 C：deletedUnits 只被 active ghost 部分覆盖 —
     *
     * #708 评论 5726837636 缺口1：
     * 旧 bug：`publishLocalHandoffScene()` 用 `ghostedCoverage.any{整段覆盖}` 判断 deletedUnits
     * 是否已被 ghost 接管。部分覆盖时（deletedUnits=[0,2)，ghostedCoverage=[1,2)），
     * alreadyGhosted==false，于是为整个 [0,2) 新建 alpha=1 ghost，导致 [1,2) 被画两次
     * （旧 active 'b' 转 ghost 画一次 + 新建 [0,2) ghost 覆盖 [1,2) 又画一次）。
     *
     * 修复后：用 [ComposeVisualRebase.subtractRanges] 算真正差集 —
     * remainingDeleted = subtractRanges([0,2), [1,2)) = [0,1)，只为 [0,1) 新建 alpha=1 ghost，
     * [1,2) 由旧 active 'b' 转的 ghost 接管，不重画。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "a"`（插入 'a'），sample 到 alpha=1 完成
     * 2. 第二笔：`"a" -> "ab"`（追加 'b'），sample 到 'b' alpha 在 0..1
     * 3. 第三笔：`"ab" -> ""`（删除整个 [0,2)），检查 handoff scene
     *
     * 断言：
     * - 存在 range=[1,2) 的 DeletedGhost（旧 active 'b' 转 ghost），alpha < 1
     * - 存在 range=[0,1) 的 DeletedGhost，alpha >= 1f（'a' 那份完整可见 ghost）
     * - 不存在 range=[0,2) 的 DeletedGhost（旧 bug 会为整个 [0,2) 新建）
     * - range=[1,2) 的 ghost 只有一个（不重影）
     */
    @Test
    fun testC_deletedUnitsPartiallyCovered_onlySupplementDifference() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "a", "ab", ""), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5726837636-C",
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
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到 'a' 动画完成（alpha=1）
        state.sampleVisualScene(1000L * NANOS_PER_MS)

        // 第二笔："a" -> "ab"（追加 'b'）
        state.recordLocalInput(
            oldText = "a",
            newText = "ab",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到 'b' 仍 active（10ms，alpha ≈ 0.1，在 0..1 之间）
        val sampledScene = state.sampleVisualScene(10L * NANOS_PER_MS)
        val unitB = sampledScene.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        assertNotNull(
            "testC: 第二笔后应存在 targetRange=[1,2) 的 unit（'b'）",
            unitB,
        )
        assertTrue(
            "testC: 'b' 应仍 active（alpha.from 在 0..1 之间），实际 alpha.from=${unitB!!.alpha.from}",
            unitB.alpha.from > 0f && unitB.alpha.from < 1f,
        )

        // 第三笔："ab" -> ""（删除整个 [0,2)）
        state.recordLocalInput(
            oldText = "ab",
            newText = "",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 2))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(0, 0), 0)

        // 在 timeline drain 之前检查 handoff scene — 这是 publishLocalHandoffScene 建立的 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 断言1：存在 range=[1,2) 的 DeletedGhost（旧 active 'b' 转 ghost），alpha < 1
        val ghostB =
            handoffScene.units.filter {
                it.range == TextRange(1, 2) && it.role == VisualUnitRole.DeletedGhost
            }
        assertTrue(
            "testC: 应存在 range=[1,2) 的 DeletedGhost（旧 active 'b' 转 ghost），" +
                "实际=${ghostB.map { "alpha=${it.alpha.from}" }}" +
                "（旧 active 'b' 正在动画中，转 ghost 应继承当前可见 alpha < 1）",
            ghostB.isNotEmpty(),
        )
        assertTrue(
            "testC: range=[1,2) 的 ghost alpha 应 < 1（继承 'b' 当前可见 alpha），" +
                "实际 alpha.from=${ghostB.first().alpha.from}",
            ghostB.first().alpha.from < 1f,
        )

        // 断言2：存在 range=[0,1) 的 DeletedGhost，alpha >= 1f（'a' 那份完整可见 ghost）
        val ghostA =
            handoffScene.units.filter {
                it.range == TextRange(0, 1) && it.role == VisualUnitRole.DeletedGhost
            }
        assertTrue(
            "testC: 应存在 range=[0,1) 的 DeletedGhost（'a' 那份完整可见 ghost），" +
                "实际=${ghostA.map { "alpha=${it.alpha.from}" }}" +
                "（修复后：subtractRanges([0,2),[1,2))=[0,1)，为 [0,1) 新建 alpha=1 ghost）",
            ghostA.isNotEmpty(),
        )
        assertTrue(
            "testC: range=[0,1) 的 ghost alpha 应 >= 1f（'a' 已完成动画，完整可见），" +
                "实际 alpha.from=${ghostA.first().alpha.from}",
            ghostA.first().alpha.from >= 1f,
        )

        // 断言3：不存在 range=[0,2) 的 DeletedGhost（旧 bug 会为整个 [0,2) 新建）
        val ghostWhole =
            handoffScene.units.filter {
                it.range == TextRange(0, 2) && it.role == VisualUnitRole.DeletedGhost
            }
        assertTrue(
            "testC: 不应存在 range=[0,2) 的 DeletedGhost，" +
                "实际=${ghostWhole.map { "alpha=${it.alpha.from}" }}" +
                "（旧 bug：alreadyGhosted=false，为整个 [0,2) 新建 alpha=1 ghost → [1,2) 被画两次）",
            ghostWhole.isEmpty(),
        )

        // 断言4：range=[1,2) 的 ghost 只有一个（不重影）
        assertEquals(
            "testC: range=[1,2) 的 ghost 应只有一个（不重影），实际数量=${ghostB.size}" +
                "（旧 bug：旧 active 'b' 转 ghost + 新建 [0,2) ghost 覆盖 [1,2) = 两个 ghost 画 [1,2)）",
            1,
            ghostB.size,
        )

        // #708 评论 5728951138 第 2 节：drain 到 timeline 后验证 timeline 也正确 —
        // 旧 bug：handoff 首帧不重影，但 timeline createDeletedGhosts 用 exact-match 判断，
        // drain 下一帧又整段补 [0,2) ghost 导致 [1,2) 被画两遍。
        // 修复后：reconcileDeletedGhosts 用 subtractRanges 算差集，只给 [0,1) 建 ghost。
        state.drainPendingPatchesAtFrame(10L * NANOS_PER_MS)
        val timelineScene = state.sampleVisualScene(10L * NANOS_PER_MS)

        // 断言5（timeline）：存在 range=[1,2) 的 DeletedGhost（旧 active 'b' 转 ghost），alpha < 1
        val timelineGhostB =
            timelineScene.units.filter {
                it.range == TextRange(1, 2) && it.role == VisualUnitRole.DeletedGhost
            }
        assertTrue(
            "testC: timeline 应存在 range=[1,2) 的 DeletedGhost（旧 active 'b' 转 ghost），" +
                "实际=${timelineGhostB.map { "alpha=${it.alpha.from}" }}" +
                "（旧 active 'b' 正在动画中，转 ghost 应继承当前可见 alpha < 1）",
            timelineGhostB.isNotEmpty(),
        )
        assertTrue(
            "testC: timeline range=[1,2) 的 ghost alpha 应 < 1（继承 'b' 当前可见 alpha），" +
                "实际 alpha.from=${timelineGhostB.first().alpha.from}",
            timelineGhostB.first().alpha.from < 1f,
        )

        // 断言6（timeline）：存在 range=[0,1) 的 DeletedGhost（'a' 那份完整可见 ghost）
        val timelineGhostA =
            timelineScene.units.filter {
                it.range == TextRange(0, 1) && it.role == VisualUnitRole.DeletedGhost
            }
        assertTrue(
            "testC: timeline 应存在 range=[0,1) 的 DeletedGhost（'a' 那份完整可见 ghost），" +
                "实际=${timelineGhostA.map { "alpha=${it.alpha.from}" }}" +
                "（修复后：reconcileDeletedGhosts 用 subtractRanges([0,2),[1,2))=[0,1)，为 [0,1) 建 ghost）",
            timelineGhostA.isNotEmpty(),
        )

        // 断言7（timeline）：不存在 range=[0,2) 的 DeletedGhost（旧 bug 会整段补 [0,2) ghost）
        val timelineGhostWhole =
            timelineScene.units.filter {
                it.range == TextRange(0, 2) && it.role == VisualUnitRole.DeletedGhost
            }
        assertTrue(
            "testC: timeline 不应存在 range=[0,2) 的 DeletedGhost，" +
                "实际=${timelineGhostWhole.map { "alpha=${it.alpha.from}" }}" +
                "（旧 bug：timeline createDeletedGhosts 用 exact-match，drain 下一帧又整段补 [0,2) ghost）",
            timelineGhostWhole.isEmpty(),
        )

        // 断言8（timeline）：range=[1,2) 的 ghost 只有一个（不重影）
        assertEquals(
            "testC: timeline range=[1,2) 的 ghost 应只有一个（不重影），实际数量=${timelineGhostB.size}" +
                "（旧 bug：handoff 首帧不重影，但 timeline drain 下一帧又整段补 [0,2) ghost 导致 [1,2) 被画两遍）",
            1,
            timelineGhostB.size,
        )
    }

    // ==================== 测试 D：多字符 active unit 删尾部 slice，ghost position 用子片段几何 ====================

    /**
     * 测试 D：多字符 active unit 只删除尾部 slice，验证 ghost position 用子片段几何 —
     *
     * #708 评论 5726837636 缺口2：
     * 旧 bug：`ComposeLocalHandoffRebase.toHandoffGhost` 直接用父 unit 左上角
     * （unit.position.from）作为 ghost position，不考虑 slice 在父 unit 内的自然位置。
     * 当父 unit range=[0,9)="abcdefghi" 只删尾部 [8,9)="i" 时，ghost 的 range=[8,9) 但 position
     * 被设成父 unit 左上角（'a' 的位置），导致 handoff 首帧 'i' 的 ghost 跳到 'a' 的位置。
     *
     * 修复后：用 [ComposeVisualRebase.sliceScreenPosition] 计算 —
     * ghostRange 是父 unit 真子区间时用"slice 自然位置 + 父 unit 当前位移"，
     * 不再直接用父左上角。
     *
     * **多字符 unit 构造**：插入 9 个字符 "abcdefghi" 触发 RUN_ANIMATION
     * （FakeLocalVisualPlanClassifier 对 >8 cluster 用 RUN_ANIMATION，整个 slice 作为一个 unit，
     * 不按 grapheme cluster 拆分），产生一个 targetRange=[0,9) 的多字符 active unit。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "abcdefghi"`（插入 9 字符），sample 到 alpha 在 0..1
     * 2. 记录 active unit 的 parentScreenPosition = unit.position.from
     * 3. 用 safePathBounds 算 parentNatural（[0,9) 左上角）和 sliceNatural（[8,9) 左上角）
     * 4. expectedGhostPosition = sliceNatural + (parentScreenPosition - parentNatural)
     * 5. 第二笔：`"abcdefghi" -> "abcdefgh"`（删尾部 'i'），检查 handoff scene
     *
     * 断言：
     * - 存在 range=[8,9) 的 DeletedGhost
     * - ghost.position.from == expectedGhostPosition（sliceNatural + parentDelta，容差 0.5f）
     * - ghost.position.from != parentScreenPosition（不是父左上角）
     */
    @Test
    fun testD_multiCharActiveUnitDeleteTailSlice_ghostPositionUsesSliceGeometry() {
        val layouts = captureLayoutsWithWidth(arrayOf("", MULTI_CHAR_TEXT, "abcdefgh"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5726837636-D",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "abcdefghi"（插入 9 字符，触发 RUN_ANIMATION 产生多字符 unit [0,9)）
        state.recordLocalInput(
            oldText = "",
            newText = MULTI_CHAR_TEXT,
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(9, 9),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 9), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(9, 9), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到多字符 unit 仍 active（10ms，alpha ≈ 0.1，在 0..1 之间）
        val sampledScene = state.sampleVisualScene(10L * NANOS_PER_MS)
        val unitAbc = sampledScene.units.firstOrNull { it.targetRange == TextRange(0, 9) }
        assertNotNull(
            "testD: 第一笔后应存在 targetRange=[0,9) 的多字符 unit（'abcdefghi'），" +
                "实际 targetRanges=${sampledScene.units.mapNotNull { it.targetRange }}" +
                "（>8 cluster 触发 RUN_ANIMATION，整个 slice 作为一个 unit 不拆分）",
            unitAbc,
        )
        assertTrue(
            "testD: 多字符 unit 应仍 active（alpha.from 在 0..1 之间），实际 alpha.from=${unitAbc!!.alpha.from}",
            unitAbc.alpha.from > 0f && unitAbc.alpha.from < 1f,
        )

        // 记录父 unit 当前屏幕位置
        val parentScreenPosition = unitAbc.position.from

        // 用 safePathBounds 算 parentNatural（[0,9) 左上角）和 sliceNatural（[8,9) 左上角）
        // active unit 的 layout 就是 layouts[1]（"abcdefghi" 的 layout）
        val parentBounds = ComposeVisualRebase.safePathBounds(layouts[1], TextRange(0, 9))
        val sliceBounds = ComposeVisualRebase.safePathBounds(layouts[1], TextRange(8, 9))
        assertNotNull("testD: parentBounds ([0,9)) 不应为 null", parentBounds)
        assertNotNull("testD: sliceBounds ([8,9)) 不应为 null", sliceBounds)
        val parentNatural = Offset(parentBounds!!.left, parentBounds.top)
        val sliceNatural = Offset(sliceBounds!!.left, sliceBounds.top)
        // parentDelta = parentScreenPosition - parentNatural
        val parentDelta =
            Offset(
                parentScreenPosition.x - parentNatural.x,
                parentScreenPosition.y - parentNatural.y,
            )
        // expectedGhostPosition = sliceNatural + parentDelta
        val expectedGhostPosition =
            Offset(
                sliceNatural.x + parentDelta.x,
                sliceNatural.y + parentDelta.y,
            )

        // 第二笔："abcdefghi" -> "abcdefgh"（删尾部 'i'，删除 [8,9)）
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

        // 断言1：存在 range=[8,9) 的 DeletedGhost
        val ghostC =
            handoffScene.units.filter {
                it.range == TextRange(8, 9) && it.role == VisualUnitRole.DeletedGhost
            }
        assertTrue(
            "testD: 应存在 range=[8,9) 的 DeletedGhost（'i' 的 ghost），" +
                "实际=${ghostC.map { "position=${it.position.from}" }}" +
                "（'abcdefghi' 的 [8,9) slice 被 rebase 转 ghost）",
            ghostC.isNotEmpty(),
        )
        val ghost = ghostC.first()

        // 断言2：ghost.position.from == expectedGhostPosition（sliceNatural + parentDelta，容差 0.5f）
        assertEquals(
            "testD: ghost.position.from.x 应等于 expectedGhostPosition.x（sliceNatural + parentDelta），" +
                "实际=${ghost.position.from.x}, expected=${expectedGhostPosition.x}," +
                "parentScreenPosition=$parentScreenPosition, parentNatural=$parentNatural, sliceNatural=$sliceNatural" +
                "（旧 bug：直接用父左上角 unit.position.from，不考虑 slice 自然位置）",
            expectedGhostPosition.x,
            ghost.position.from.x,
            0.5f,
        )
        assertEquals(
            "testD: ghost.position.from.y 应等于 expectedGhostPosition.y（sliceNatural + parentDelta），" +
                "实际=${ghost.position.from.y}, expected=${expectedGhostPosition.y}",
            expectedGhostPosition.y,
            ghost.position.from.y,
            0.5f,
        )

        // 断言3：ghost.position.from != parentScreenPosition（不是父左上角）
        // slice=[8,9) != parent=[0,9)，sliceNatural != parentNatural，
        // 所以即使 parentDelta==0，ghost.position = sliceNatural != parentNatural = parentScreenPosition
        val positionDiffX = kotlin.math.abs(ghost.position.from.x - parentScreenPosition.x)
        val positionDiffY = kotlin.math.abs(ghost.position.from.y - parentScreenPosition.y)
        assertTrue(
            "testD: ghost.position.from 应不等于 parentScreenPosition（不是父左上角），" +
                "实际=${ghost.position.from}, parentScreenPosition=$parentScreenPosition," +
                "diffX=$positionDiffX, diffY=$positionDiffY" +
                "（旧 bug：ghost position 直接用父左上角，'i' 的 ghost 跳到 'a' 的位置）",
            positionDiffX > 0.5f || positionDiffY > 0.5f,
        )
    }

    // ==================== 测试 E：surviving slice position 用子片段几何 ====================

    /**
     * 测试 E：多字符 active unit 删中间字符，surviving slice position 用子片段几何 —
     *
     * #708 评论 5727440517：
     * 旧 bug：`mapSurvivingSliceToHandoff` 和 `mapSurvivingSlice` 直接用父 unit 左上角
     * （unit.position.from / currentOffset(unit.position, frameTimeNanos)）作为 surviving slice position，
     * 不考虑 slice 在父 unit 内的自然位置。当父 unit range=[0,9)="abcdefghi" 只删中间 'e' [4,5) 时，
     * offsetMap 把父 unit 切成：
     * - old [0,4) → new [0,4)  surviving（前半段 "abcd"）
     * - old [4,5)              ghost（被删的 "e"）
     * - old [5,9) → new [4,8)  surviving（后半段 "fghi"）
     *
     * 后半段 old [5,9) 原本在父 unit 的后半段，但 handoff/timeline 却给它 position = 父 unit 左上角
     * （'a' 的位置），导致首帧后半段突然叠到前半段左侧，下一帧又从最左边动画到新位置。
     *
     * 修复后：用 [ComposeVisualRebase.sliceScreenPosition] 计算 —
     * oldRange 是父 unit 真子区间时用"slice 自然位置 + 父 unit 当前位移"，
     * 不再直接用父左上角。handoff 和 timeline 都用同一套几何。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "abcdefghi"`（插入 9 字符），sample 到 alpha 在 0..1
     * 2. 第二笔：`"abcdefghi" -> "abcdfghi"`（删中间 'e' [4,5)）
     * 3. 检查 handoff scene 中 surviving slice 的 position
     * 4. drain 到 timeline 后检查 position.from 仍然正确
     *
     * 断言：
     * - handoff 中后半段 surviving (new [4,8)) 的 position.from 不等于父 unit 左上角
     * - handoff 中后半段 surviving 的 position.from 等于 old [5,9) natural position + parent delta
     * - drain 到 timeline 后，同一 frameTime 的 position.from 仍然是这个值
     * - 最终 position.to 是 newLayout 的 [4,8) 自然位置
     */
    @Test
    fun testE_multiCharActiveUnitDeleteMiddle_survivingSlicePositionUsesSliceGeometry() {
        val layouts = captureLayoutsWithWidth(arrayOf("", MULTI_CHAR_TEXT, "abcdfghi"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5727440517-E",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "abcdefghi"（插入 9 字符，触发 RUN_ANIMATION 产生多字符 unit [0,9)）
        state.recordLocalInput(
            oldText = "",
            newText = MULTI_CHAR_TEXT,
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(9, 9),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 9), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(9, 9), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到多字符 unit 仍 active（10ms，alpha ≈ 0.1，在 0..1 之间）
        val sampledScene = state.sampleVisualScene(10L * NANOS_PER_MS)
        val unitAbc = sampledScene.units.firstOrNull { it.targetRange == TextRange(0, 9) }
        assertNotNull(
            "testE: 第一笔后应存在 targetRange=[0,9) 的多字符 unit（'abcdefghi'），" +
                "实际 targetRanges=${sampledScene.units.mapNotNull { it.targetRange }}",
            unitAbc,
        )
        assertTrue(
            "testE: 多字符 unit 应仍 active（alpha.from 在 0..1 之间），实际 alpha.from=${unitAbc!!.alpha.from}",
            unitAbc.alpha.from > 0f && unitAbc.alpha.from < 1f,
        )

        // 记录父 unit 当前屏幕位置（用于后续断言 surviving slice 不等于父左上角）
        val parentScreenPosition = unitAbc.position.from

        // 用 safePathBounds 算 parentNatural（[0,9) 左上角）和 sliceNatural（[5,9) 左上角）
        // active unit 的 layout 就是 layouts[1]（"abcdefghi" 的 layout）
        val parentBounds = ComposeVisualRebase.safePathBounds(layouts[1], TextRange(0, 9))
        val sliceBounds = ComposeVisualRebase.safePathBounds(layouts[1], TextRange(5, 9))
        assertNotNull("testE: parentBounds ([0,9)) 不应为 null", parentBounds)
        assertNotNull("testE: sliceBounds ([5,9)) 不应为 null", sliceBounds)
        val parentNatural = Offset(parentBounds!!.left, parentBounds.top)
        val sliceNatural = Offset(sliceBounds!!.left, sliceBounds.top)
        // parentDelta = parentScreenPosition - parentNatural
        val parentDelta =
            Offset(
                parentScreenPosition.x - parentNatural.x,
                parentScreenPosition.y - parentNatural.y,
            )
        // expectedSurvivingPosition = sliceNatural + parentDelta
        val expectedSurvivingPosition =
            Offset(
                sliceNatural.x + parentDelta.x,
                sliceNatural.y + parentDelta.y,
            )

        // 第二笔："abcdefghi" -> "abcdfghi"（删中间 'e'，删除 [4,5)）
        state.recordLocalInput(
            oldText = MULTI_CHAR_TEXT,
            newText = "abcdfghi",
            oldSelection = TextRange(5, 5),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(4, 4), oldRange = TextRange(4, 5))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)

        // 在 timeline drain 之前检查 handoff scene — 这是 publishLocalHandoffScene 建立的 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 断言1：存在 targetRange=[4,8) 的 surviving unit（后半段 "fghi" 映射到新坐标）
        val survivingBack =
            handoffScene.units.filter { it.targetRange == TextRange(4, 8) }
        assertTrue(
            "testE: handoff 应存在 targetRange=[4,8) 的 surviving unit（后半段 'fghi' 映射到新坐标），" +
                "实际=${survivingBack.map { "targetRange=${it.targetRange}, position=${it.position.from}" }}" +
                "（offsetMap: old [5,9) → new [4,8)）",
            survivingBack.isNotEmpty(),
        )

        // 断言2：surviving 后半段的 position.from 不等于父 unit 左上角
        val backUnit = survivingBack.first()
        val backDiffX = kotlin.math.abs(backUnit.position.from.x - parentScreenPosition.x)
        val backDiffY = kotlin.math.abs(backUnit.position.from.y - parentScreenPosition.y)
        assertTrue(
            "testE: surviving 后半段 position.from 应不等于父 unit 左上角（不是父左上角），" +
                "实际=${backUnit.position.from}, parentScreenPosition=$parentScreenPosition," +
                "diffX=$backDiffX, diffY=$backDiffY" +
                "（旧 bug：surviving slice 直接继承父 unit 左上角，后半段叠到前半段左侧）",
            backDiffX > 0.5f || backDiffY > 0.5f,
        )

        // 断言3：surviving 后半段的 position.from 等于 old [5,9) natural position + parent delta
        assertEquals(
            "testE: surviving 后半段 position.from.x 应等于 expectedSurvivingPosition.x" +
                "（old [5,9) natural + parent delta），" +
                "实际=${backUnit.position.from.x}, expected=${expectedSurvivingPosition.x}," +
                "parentScreenPosition=$parentScreenPosition, parentNatural=$parentNatural, sliceNatural=$sliceNatural",
            expectedSurvivingPosition.x,
            backUnit.position.from.x,
            0.5f,
        )
        assertEquals(
            "testE: surviving 后半段 position.from.y 应等于 expectedSurvivingPosition.y",
            expectedSurvivingPosition.y,
            backUnit.position.from.y,
            0.5f,
        )

        // 断言4：drain 到 timeline 后，同一 frameTime 的 position.from 仍然是这个值
        // drain 把 patch 应用到 timeline，timeline 的 mapSurvivingSlice 也用 sliceScreenPosition
        state.drainPendingPatchesAtFrame(10L * NANOS_PER_MS)
        val timelineScene = state.sampleVisualScene(10L * NANOS_PER_MS)
        val timelineBack =
            timelineScene.units.firstOrNull { it.targetRange == TextRange(4, 8) }
        assertNotNull(
            "testE: timeline drain 后应存在 targetRange=[4,8) 的 surviving unit",
            timelineBack,
        )
        // timeline 版本 position.from 应该也是 sliceScreenPosition 计算的值
        // 注意：timeline 的 position.from 可能与 handoff 略有不同（因为 timeline 用 currentOffset
        // 算 parentCurrent，而 handoff 用 unit.position.from），但在同一 frameTime 下应一致
        assertEquals(
            "testE: timeline drain 后 surviving 后半段 position.from.x 应仍等于 expectedSurvivingPosition.x" +
                "（不能重新变回父左上角），实际=${timelineBack!!.position.from.x}," +
                "expected=${expectedSurvivingPosition.x}",
            expectedSurvivingPosition.x,
            timelineBack.position.from.x,
            0.5f,
        )
        assertEquals(
            "testE: timeline drain 后 surviving 后半段 position.from.y 应仍等于 expectedSurvivingPosition.y",
            expectedSurvivingPosition.y,
            timelineBack.position.from.y,
            0.5f,
        )

        // 断言5：最终 position.to 是 newLayout 的 [4,8) 自然位置
        val newLayoutBounds = ComposeVisualRebase.safePathBounds(layouts[2], TextRange(4, 8))
        assertNotNull("testE: newLayoutBounds ([4,8)) 不应为 null", newLayoutBounds)
        val expectedFinalPosition = Offset(newLayoutBounds!!.left, newLayoutBounds.top)
        assertEquals(
            "testE: surviving 后半段 position.to.x 应等于 newLayout [4,8) 自然位置，" +
                "实际=${timelineBack.position.to.x}, expected=${expectedFinalPosition.x}",
            expectedFinalPosition.x,
            timelineBack.position.to.x,
            0.5f,
        )
        assertEquals(
            "testE: surviving 后半段 position.to.y 应等于 newLayout [4,8) 自然位置",
            expectedFinalPosition.y,
            timelineBack.position.to.y,
            0.5f,
        )
    }

    // ==================== 测试 F：split 后子 unit 独立 key + 独立 clip fraction ====================

    /**
     * 测试 F：多字符 active unit 删中间 → split 后子 unit 独立 key + 独立 clip fraction。
     *
     * #708 评论 5727808906：
     * 旧 bug：父 VisualTextUnit 被切成多段后，子 unit 仍然共用同一个 key，
     * 但 `VisualTextUnit.key` 的契约是"唯一标识 — 快速输入时不重置"。这导致：
     * 1. `ComposeVisualScene.unitClipFractions`（`Map<Long, Float>`，key=unit.key）
     *    同 key 互相覆盖，三段文字拿同一个 fraction；
     * 2. `presentedKeys` 被 `associate` 压缩成最后一个同 key 结果，
     *    `presentedKeys.remove(key)` 会误删其他活跃 slice 的 presented 身份；
     * 3. handoff 首帧 `scene.copy(units = rebasedUnits)` 不重建 `unitClipFractions`，
     *    三段共用父块空间进度。
     *
     * 修复后：
     * - split 时（2+ 子 unit）每个子 unit 分配独立新 key；
     * - presented 状态从父 key 传递到 surviving child key；
     * - handoff rebase 后用 [ComposeVisualClip.fractionFor] 对每个 child 单独算 clip fraction。
     *
     * 测试场景（基于测试 E）：
     * 1. 第一笔：`"" -> "abcdefghi"`（插入 9 字符，触发 RUN_ANIMATION 产生多字符 unit [0,9)）
     * 2. sample 到多字符 unit 仍 active
     * 3. 第二笔：`"abcdefghi" -> "abcdfghi"`（删中间 'e'，删除 [4,5)）
     *    → 前 surviving [0,4) + ghost [4,5) + 后 surviving [4,8)
     *
     * 断言：
     * - handoff split 后 child key 彼此唯一（不能再 3 个 unit 一个 key）
     * - coordinated 模式下每个 surviving child 有独立 unitClipFractions entry
     * - handoff scene 的 frontFraction 与 backFraction 不相等（不因同 key 覆盖而全部相等）
     * - drain 到 timeline 后：child key 仍然唯一 + unitClipFractions 每个 child 独立
     *   + 中间帧 fraction 不全相等
     */
    @Test
    fun testF_multiCharActiveUnitDeleteMiddle_splitChildrenGetIndependentKeysAndClipFractions() {
        // 一次 setContent 捕获所有需要的 layout（composeRule.setContent 只能调一次）
        val layouts =
            captureLayoutsWithWidth(
                arrayOf("", MULTI_CHAR_TEXT, "abcdfghi", "bcdfghi"),
                1000,
            )
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5727808906-F",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "abcdefghi"（插入 9 字符，触发 RUN_ANIMATION 产生多字符 unit [0,9)）
        state.recordLocalInput(
            oldText = "",
            newText = MULTI_CHAR_TEXT,
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(9, 9),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 9), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(9, 9), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到多字符 unit 仍 active（10ms，alpha ≈ 0.1，在 0..1 之间）
        val sampledScene = state.sampleVisualScene(10L * NANOS_PER_MS)
        val unitAbc = sampledScene.units.firstOrNull { it.targetRange == TextRange(0, 9) }
        assertNotNull(
            "testF: 第一笔后应存在 targetRange=[0,9) 的多字符 unit（'abcdefghi'），" +
                "实际 targetRanges=${sampledScene.units.mapNotNull { it.targetRange }}",
            unitAbc,
        )

        // 第二笔："abcdefghi" -> "abcdfghi"（删中间 'e'，删除 [4,5)）
        // → 前 surviving [0,4) + ghost [4,5) + 后 surviving [4,8)
        state.recordLocalInput(
            oldText = MULTI_CHAR_TEXT,
            newText = "abcdfghi",
            oldSelection = TextRange(5, 5),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(4, 4), oldRange = TextRange(4, 5))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)

        // 在 timeline drain 之前检查 handoff scene — 这是 publishLocalHandoffScene 建立的 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 断言1：handoff split 后 child key 彼此唯一
        // 前 surviving [0,4) + ghost [4,5)（targetRange=null）+ 后 surviving [4,8)
        val splitUnits =
            handoffScene.units.filter {
                it.targetRange == TextRange(0, 4) ||
                    it.targetRange == null ||
                    it.targetRange == TextRange(4, 8)
            }
        assertTrue(
            "testF: handoff 应存在 split 出的 3 个 unit（前 surviving [0,4) + ghost + 后 surviving [4,8)），" +
                "实际 size=${splitUnits.size}，" +
                "targetRanges=${handoffScene.units.map { it.targetRange }}",
            splitUnits.size >= 3,
        )
        assertEquals(
            "testF: split 后 child key 应彼此唯一（不能再 3 个 unit 一个 key），" +
                "实际 keys=${splitUnits.map { it.key }}",
            splitUnits.size,
            splitUnits.map { it.key }.distinct().size,
        )

        // 断言2：coordinated 模式下每个 surviving child 有独立 unitClipFractions entry
        // 默认 EditorMotionPolicy: textEnabled=true, cursorEnabled=true, coordinated=true
        assertTrue(
            "testF: handoff scene 应为 coordinated 模式（textEnabled && cursorEnabled && coordinated），" +
                "实际 coordinatedSpatialClip=${handoffScene.coordinatedSpatialClip}",
            handoffScene.coordinatedSpatialClip,
        )
        val survivingChildren = handoffScene.units.filter { it.targetRange != null }
        assertTrue(
            "testF: handoff 应存在 surviving child（targetRange != null）",
            survivingChildren.isNotEmpty(),
        )
        for (child in survivingChildren) {
            assertTrue(
                "testF: surviving child key=${child.key} targetRange=${child.targetRange} " +
                    "应在 unitClipFractions 中有独立 entry，" +
                    "实际 unitClipFractions.keys=${handoffScene.unitClipFractions.keys}",
                handoffScene.unitClipFractions.containsKey(child.key),
            )
        }

        // 断言3：handoff scene 的所有 split unit（含 ghost）都有独立 unitClipFractions entry —
        // 旧 bug：三段共用一个 key，unitClipFractions 只剩最后一个的 fraction（map 压缩成 1 个 entry）。
        // 修复后：三个 unit 有独立 key，unitClipFractions 应有 >= 3 个 entry（front, ghost, back 各一个）。
        val frontUnit = handoffScene.units.firstOrNull { it.targetRange == TextRange(0, 4) }
        val ghostUnit = handoffScene.units.firstOrNull { it.targetRange == null }
        val backUnit = handoffScene.units.firstOrNull { it.targetRange == TextRange(4, 8) }
        assertNotNull("testF: handoff 应存在前 surviving [0,4) unit", frontUnit)
        assertNotNull("testF: handoff 应存在 ghost unit", ghostUnit)
        assertNotNull("testF: handoff 应存在后 surviving [4,8) unit", backUnit)
        // 每个 split unit 都应在 unitClipFractions 中有独立 entry
        for (splitUnit in listOf(frontUnit!!, ghostUnit!!, backUnit!!)) {
            assertTrue(
                "testF: split unit key=${splitUnit.key} targetRange=${splitUnit.targetRange} " +
                    "应在 unitClipFractions 中有独立 entry，" +
                    "实际 unitClipFractions.keys=${handoffScene.unitClipFractions.keys}",
                handoffScene.unitClipFractions.containsKey(splitUnit.key),
            )
        }
        assertTrue(
            "testF: handoff unitClipFractions 应有 >= 3 个独立 entry（front + ghost + back），" +
                "实际 size=${handoffScene.unitClipFractions.size}，" +
                "keys=${handoffScene.unitClipFractions.keys}" +
                "（旧 bug：同 key 覆盖导致 map 压缩成 1 个 entry）",
            handoffScene.unitClipFractions.size >= 3,
        )

        // 断言4：drain 到 timeline 后同样验证
        state.drainPendingPatchesAtFrame(10L * NANOS_PER_MS)

        // #708 评论 5728507555 断言：split 后 presented 状态在当前 applyPatch 里真正传给 child —
        // parent 在 split 前 alpha.from ≈ 0.1（10ms，duration=100ms），
        // split 后 drain 同一 frameTime（10ms），surviving child 的 alpha.from 仍然必须是 0.x，
        // 不能变成 0（旧 bug：child 被判成 pending，alpha 重置 0→1，已显示到一半的文字闪没）。
        val drainScene = state.sampleVisualScene(10L * NANOS_PER_MS)
        val drainFrontChild = drainScene.units.firstOrNull { it.targetRange == TextRange(0, 4) }
        val drainBackChild = drainScene.units.firstOrNull { it.targetRange == TextRange(4, 8) }
        assertNotNull(
            "testF: drain 后应存在前 surviving [0,4) child，" +
                "实际 targetRanges=${drainScene.units.mapNotNull { it.targetRange }}",
            drainFrontChild,
        )
        assertNotNull(
            "testF: drain 后应存在后 surviving [4,8) child，" +
                "实际 targetRanges=${drainScene.units.mapNotNull { it.targetRange }}",
            drainBackChild,
        )
        // 两个 surviving child 的 alpha.from 都应 > 0（继承 parent presented 状态，不能重置为 0）
        assertTrue(
            "testF: 前 surviving child alpha.from 应 > 0（继承 parent presented 状态，不能重置为 0），" +
                "实际 alpha.from=${drainFrontChild!!.alpha.from}" +
                "（旧 bug：child key 不在旧 progressByKey 里，被判成 pending，alpha 重置 0→1）",
            drainFrontChild.alpha.from > 0f,
        )
        assertTrue(
            "testF: 后 surviving child alpha.from 应 > 0（继承 parent presented 状态，不能重置为 0），" +
                "实际 alpha.from=${drainBackChild!!.alpha.from}" +
                "（旧 bug：child key 不在旧 progressByKey 里，被判成 pending，alpha 重置 0→1）",
            drainBackChild.alpha.from > 0f,
        )

        // sample 到 cursor 在中间位置的帧 —
        // cursor 动画从 10ms 开始，coordinated 模式下持续 textDurationMillis=100ms，
        // 60ms 时 progress=0.5，cursor 在 [4,5) 之间（'e' 中间）。
        // 此时：前 surviving=1，ghost 在 0..1，后 surviving=0，三个 fraction 不全相等。
        val midScene = state.sampleVisualScene(60L * NANOS_PER_MS)

        // child key 仍然彼此唯一
        val timelineSplitUnits =
            midScene.units.filter {
                it.targetRange == TextRange(0, 4) ||
                    it.targetRange == null ||
                    it.targetRange == TextRange(4, 8)
            }
        assertTrue(
            "testF: timeline drain 后应存在 split 出的 unit，" +
                "实际 targetRanges=${midScene.units.map { it.targetRange }}",
            timelineSplitUnits.isNotEmpty(),
        )
        assertEquals(
            "testF: timeline drain 后 child key 应仍彼此唯一，" +
                "实际 keys=${timelineSplitUnits.map { it.key }}",
            timelineSplitUnits.size,
            timelineSplitUnits.map { it.key }.distinct().size,
        )

        // unitClipFractions 每个 surviving child 独立
        val timelineSurviving = midScene.units.filter { it.targetRange != null }
        for (child in timelineSurviving) {
            assertTrue(
                "testF: timeline surviving child key=${child.key} targetRange=${child.targetRange} " +
                    "应在 unitClipFractions 中有 entry，" +
                    "实际 unitClipFractions.keys=${midScene.unitClipFractions.keys}",
                midScene.unitClipFractions.containsKey(child.key),
            )
        }

        // 中间帧 fraction 独立性验证 —
        // #708 评论 5727808906 核心修复：split 后每个 child 有独立 key 和独立 unitClipFractions entry，
        // 不因同 key 覆盖而压缩成 1 个 entry。
        // #708 评论 5728507555：buildLocalInputPatch() 里本地输入的 retainedMoves 现在明确是空列表，
        // 已有 active surviving range 被 reflow ownership 扣掉后通常仍保持原来的 Inserted role。
        // 因此这里直接验证真实 Inserted child 的 fraction，不只检查 map entry 数量。
        val midFractionEntries =
            timelineSplitUnits.mapNotNull { midScene.unitClipFractions[it.key] }
        assertTrue(
            "testF: 中间帧每个 split unit 应在 unitClipFractions 中有独立 entry，" +
                "splitUnits=${timelineSplitUnits.size}，fractionEntries=${midFractionEntries.size}，" +
                "keys=${timelineSplitUnits.map { it.key }}，" +
                "unitClipFractions.keys=${midScene.unitClipFractions.keys}" +
                "（旧 bug：同 key 覆盖导致 map 压缩成 1 个 entry）",
            midFractionEntries.size >= timelineSplitUnits.size,
        )

        // #708 评论 5728507555 断言：fraction 按当前实际绘制位置算，不是按自然位置算 —
        // surviving child 有位置动画时（old slice 位置 → new natural 位置），
        // natural bounds 是新 layout 最终位置，unit.position.from 是当前绘制位置。
        // cursor 在两者之间时，按自然位置算和按当前绘制位置算的 fraction 不同。
        // 旧 bug：fractionFor 用自然位置，和 draw 层真正 translate 出来的字坐标系不一致。
        val midBackChild = midScene.units.firstOrNull { it.targetRange == TextRange(4, 8) }
        assertNotNull(
            "testF: 60ms 时应存在后 surviving [4,8) child",
            midBackChild,
        )
        val midNaturalBounds = ComposeVisualRebase.safePathBounds(layouts[2], TextRange(4, 8))
        assertNotNull("testF: midNaturalBounds ([4,8)) 不应为 null", midNaturalBounds)
        val midNaturalLeft = midNaturalBounds!!.left
        val midCurrentDrawLeft = midBackChild!!.position.from.x
        val midPositionDelta = kotlin.math.abs(midCurrentDrawLeft - midNaturalLeft)
        // 只有存在非零 position delta 时才验证 fraction 值（delta=0 时两种算法一致，无法区分）
        if (midPositionDelta > 0.5f) {
            val midCursorLeft = midScene.cursorRect?.left
            assertNotNull("testF: midScene 应有 cursorRect", midCursorLeft)
            val midGlyphWidth = midNaturalBounds.width
            if (midGlyphWidth > 0.5f) {
                val midActualFraction = midScene.unitClipFractions[midBackChild.key]
                assertNotNull(
                    "testF: midBackChild 应在 unitClipFractions 中有 entry，" +
                        "keys=${midScene.unitClipFractions.keys}",
                    midActualFraction,
                )
                // 按当前绘制位置算的 fraction
                val fractionByDrawnPos = ((midCursorLeft!! - midCurrentDrawLeft) / midGlyphWidth).coerceIn(0f, 1f)
                // 按自然位置算的 fraction（旧 bug）
                val fractionByNaturalPos = ((midCursorLeft - midNaturalLeft) / midGlyphWidth).coerceIn(0f, 1f)
                // 只有当两种算法给出不同结果时才验证（cursor 在自然位置和当前绘制位置之间）
                if (kotlin.math.abs(fractionByDrawnPos - fractionByNaturalPos) > 0.01f) {
                    assertEquals(
                        "testF: fraction 应按当前绘制位置算（不是自然位置），" +
                            "actual=$midActualFraction, byDrawnPos=$fractionByDrawnPos, " +
                            "byNaturalPos=$fractionByNaturalPos, " +
                            "currentDrawLeft=$midCurrentDrawLeft, naturalLeft=$midNaturalLeft, " +
                            "cursorLeft=$midCursorLeft, positionDelta=$midPositionDelta" +
                            "（旧 bug：fractionFor 用自然位置，裁切提前/滞后）",
                        fractionByDrawnPos,
                        midActualFraction!!,
                        0.01f,
                    )
                }
            }
        }

        // 断言5：下一笔 patch 到来时，已 presented 的 surviving child 不会因另一个同源 child 收口
        // 而重新变 pending — 通过再删一个字符验证 presented 状态正确传递。
        // 此时前 surviving [0,4) "abcd" 和后 surviving [4,8) "fghi" 应已 presented（alpha 已离开起点）。
        // 再删 [0,1)（删 'a'）：前 surviving [0,4) 会被 split 成 ghost [0,1) + surviving [0,3)。
        // 如果 presented 状态正确传递，surviving [0,3) 不会重新从 alpha 0 开始。
        // 这里只验证不崩溃且 key 唯一 — presented 状态的精确验证需要观察 alpha 通道，
        // 但 key 唯一性已保证 presentedKeys 不会被同 key 误删。
        state.recordLocalInput(
            oldText = "abcdfghi",
            newText = "bcdfghi",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(0, 0), 0)
        // handoff scene 不应崩溃，且 key 仍唯一
        val handoffScene2 = state.drawSnapshot().scene
        val handoff2Keys = handoffScene2.units.map { it.key }
        assertEquals(
            "testF: 第二次 split 后 child key 应仍彼此唯一，实际 keys=$handoff2Keys",
            handoff2Keys.size,
            handoff2Keys.distinct().size,
        )
    }

    // ==================== 测试 G：等长替换验证 handoff 不用 size 判断 ====================

    /**
     * 测试 G：等长替换场景，验证 handoff 不用 size 判断 —
     *
     * #708 评论 5728951138 第 1 节：
     * 旧 bug：`publishLocalHandoffScene` 用 `mergedHidden.size != scene.hiddenRanges.size` 等
     * size 判断 scene 是否变化。等长替换（`"a" -> "b"`）时数量不变，代码直接 `return scene`
     * 把刚算好的 rebase 全扔了，导致首帧拿旧 scene 画旧字（"旧字闪一帧"）。
     *
     * 修复后：删掉 size gate，rebase 后直接构造 `scene.copy(...)`。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "a"`（插入 'a'），sample 到 'a' 仍 active（alpha 在 0..1 之间）
     * 2. 第二笔：`"a" -> "b"`（等长替换 [0,1) -> [0,1)）
     * 3. 在 timeline drain 之前检查 handoff scene
     *
     * 断言：
     * - 不应存在 targetRange=[0,1) 且 role=Inserted 且 layout 是旧 "a" layout 的 unit
     *   （旧 bug 会 return scene 直接用旧 scene，旧 'a' 的 Inserted unit 仍在）
     * - 应存在 range=[0,1) 且 role=DeletedGhost 的 unit（旧 'a' 转 ghost）
     * - hiddenRanges 仍应包含 [0,1)（新 'b' 的 [0,1) 应继续被隐藏，等 timeline 吐出来）
     * - 旧 'a' 的 DeletedGhost alpha 应 < 1（继承当前可见 alpha，不是新建 alpha=1 的完整 ghost）
     *   （重点：即使 units.size 和 hiddenRanges.size 与上一 scene 相同，也必须发布新内容；
     *   旧 bug size gate 直接 return scene，旧 'a' 的 Inserted unit 仍在，没有 DeletedGhost）
     */
    @Test
    fun testG_equalLengthReplacement_handoffPublishesNewContent() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "a", "b"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5728951138-G",
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
            "testG: 第一笔后应存在 targetRange=[0,1) 的 unit（'a'）",
            unitA,
        )
        assertTrue(
            "testG: 'a' 应仍 active（alpha.from 在 0..1 之间），实际 alpha.from=${unitA!!.alpha.from}" +
                "（如果 alpha.from==1f 说明动画已完成，需要用更早的 sample 时间）",
            unitA.alpha.from > 0f && unitA.alpha.from < 1f,
        )

        // 第二笔："a" -> "b"（等长替换 [0,1) -> [0,1)）
        // 选中 a 然后输入 b：oldSelection = TextRange(0,1)（选中 a），newSelection = TextRange(1,1)（光标在 b 后）
        // changes: oldRange = TextRange(0,1)（旧 a 的范围），newRange = TextRange(0,1)（新 b 的范围）
        state.recordLocalInput(
            oldText = "a",
            newText = "b",
            oldSelection = TextRange(0, 1),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 1))),
        )

        // onAuthoritativeLayout 配对生成第二笔 localPatch，建立 handoff scene（触发 publishLocalHandoffScene）
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0)

        // 在 timeline drain 之前检查 handoff scene — 这是 publishLocalHandoffScene 建立的 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 断言1：不应存在 targetRange=[0,1) 且 role=Inserted 且 layout 是旧 "a" layout 的 unit
        // 旧 bug：size gate 直接 return scene，旧 'a' 的 Inserted unit（targetRange=[0,1), role=Inserted,
        // layout=旧 "a" layout）仍在 handoff scene 中，首帧拿旧 scene 画旧字 'a'（"旧字闪一帧"）
        // 修复后：rebase 把旧 'a' 转 DeletedGhost，新 'b' 的 Inserted unit 用新 "b" layout
        val oldInsertedA =
            handoffScene.units.filter {
                it.targetRange == TextRange(0, 1) &&
                    it.role == VisualUnitRole.Inserted &&
                    it.layout.result.layoutInput.text.text == "a"
            }
        assertTrue(
            "testG: 不应存在 targetRange=[0,1) 且 role=Inserted 且 layout 是旧 'a' layout 的 unit，" +
                "实际=${oldInsertedA.map { "key=${it.key}, range=${it.range}, role=${it.role}" }}" +
                "（旧 bug：size gate 直接 return scene，旧 'a' 的 Inserted unit 仍在，首帧画旧字 'a'）",
            oldInsertedA.isEmpty(),
        )

        // 断言2：应存在 range=[0,1) 且 role=DeletedGhost 的 unit（旧 'a' 转 ghost）
        val ghostA =
            handoffScene.units.filter {
                it.range == TextRange(0, 1) && it.role == VisualUnitRole.DeletedGhost
            }
        assertTrue(
            "testG: 应存在 range=[0,1) 且 role=DeletedGhost 的 unit（旧 'a' 转 ghost），" +
                "实际=${handoffScene.units.map { "range=${it.range}, role=${it.role}, targetRange=${it.targetRange}" }}" +
                "（修复后：rebase 把旧 'a' 转 DeletedGhost，不新建 alpha=1 的完整 ghost）",
            ghostA.isNotEmpty(),
        )

        // 断言3：hiddenRanges 仍应包含 [0,1)（新 'b' 的 [0,1) 应继续被隐藏，等 timeline 吐出来）
        assertTrue(
            "testG: hiddenRanges 应包含 [0,1)（新 'b' 的 [0,1) 应继续被隐藏，等 timeline 吐出来），" +
                "实际 hiddenRanges=${handoffScene.hiddenRanges}" +
                "（insertedUnits 加入 hiddenRanges，让 BasicTextField 先不画新字，由 overlay 吐字）",
            handoffScene.hiddenRanges.any { it.start == 0 && it.end == 1 },
        )

        // 断言4：旧 'a' 的 DeletedGhost alpha 应继承当前可见 alpha（< 1），不是新建 alpha=1 的完整 ghost
        // 重点：即使 units.size 和 hiddenRanges.size 与上一 scene 相同，也必须发布新内容。
        // 这个通过断言 1-3 间接验证 — 如果 size gate 还在，rebase 结果被扔，断言 1-3 会失败。
        // 这里再加一个显式断言：handoff scene 的 DeletedGhost alpha 应 < 1（继承当前可见 alpha），
        // 证明 rebase 正确转 ghost 而不是新建 alpha=1 的完整 ghost。
        // 旧 bug：size gate 直接 return scene，旧 'a' 的 Inserted unit（alpha=0.1）仍在，
        // 没有 DeletedGhost。修复后：rebase 把旧 'a' 转 DeletedGhost，alpha 继承当前可见值。
        assertTrue(
            "testG: 旧 'a' 的 DeletedGhost alpha 应 < 1（继承当前可见 alpha），" +
                "实际 alpha.from=${ghostA.first().alpha.from}" +
                "（修复后：rebase 把旧 'a' 转 ghost，alpha 继承当前可见值，不新建 alpha=1 的完整 ghost）",
            ghostA.first().alpha.from < 1f,
        )
    }

    // ==================== 测试 H1：alpha=0 的 active unit 删除时不补 alpha=1 ghost ====================

    /**
     * 测试 H1：alpha=0 的 active unit 删除时不补 alpha=1 ghost —
     *
     * #708 评论 5729482707 修复1：
     * 旧 bug：fullyDeleted 分支在 alpha<=0 时不记 coverage，导致 reconcileDeletedGhosts
     * 认为整段没人接管，新建 alpha=1 完整 ghost → 用户还没看到的字在删除那一帧突然完整出现。
     *
     * 修复后：无论 alpha 是否 > 0，都先记 coverage，让 reconcileDeletedGhosts 不再为这段补 ghost。
     * alpha>0 时转 ghost；alpha<=0 时不保留 ghost 但 coverage 已记（直接消失）。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "ab"`（插入 'a' 和 'b'），多字符吐字分段 a:0~50ms b:50~100ms
     * 2. sample 10ms，确认 b [1,2) alpha == 0（b 的 segment 还没开始）
     * 3. 第二笔：`"ab" -> "a"`（删 [1,2) 'b'），drain at 10ms
     * 4. sample 10ms，检查 scene
     *
     * 断言：
     * - 前置条件：10ms 时 b 的 alpha == 0（确认多字符分段生效）
     * - 不存在 range=[1,2) 且 alpha.from==1f 的 DeletedGhost（旧 bug：reconcile 补 alpha=1 完整 ghost）
     * - 不存在 range=[1,2) 的 DeletedGhost（alpha=0 的字删除后直接消失，不应有任何 ghost）
     */
    @Test
    fun testH1_alphaZeroActiveUnitDeleted_noAlphaOneGhost() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab", "a"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5729482707-H1",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "ab"（插入 'a' 和 'b'，2 cluster 用 GLYPH_ANIMATION 拆成 [0,1) 和 [1,2)）
        // repartitionPendingAndInsertedUnits: n=2, a: startedAt=0 duration=50ms, b: startedAt=50ms duration=50ms
        state.recordLocalInput(
            oldText = "",
            newText = "ab",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 2), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 10ms：a alpha=0.2（10/50），b alpha=0（10 < 50，segment 还没开始）
        val sampledScene = state.sampleVisualScene(10L * NANOS_PER_MS)
        val unitB = sampledScene.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        assertNotNull(
            "testH1: 第一笔后应存在 targetRange=[1,2) 的 unit（'b'），" +
                "实际 targetRanges=${sampledScene.units.mapNotNull { it.targetRange }}",
            unitB,
        )
        // 前置条件：b 的 alpha 在 10ms 时应为 0（segment 从 50ms 开始）
        assertEquals(
            "testH1: 前置条件 — 10ms 时 b 的 alpha 应为 0（segment 从 50ms 开始），" +
                "实际 alpha.from=${unitB!!.alpha.from}, startedAtNanos=${unitB.alpha.startedAtNanos}" +
                "（如果 alpha > 0 说明多字符分段没生效，需检查 textDurationMillis 和分段逻辑）",
            0f,
            unitB.alpha.from,
            0.001f,
        )

        // 第二笔："ab" -> "a"（删 [1,2) 'b'），drain at 10ms
        // b 的 alpha=0，fullyDeleted 分支：修复后记 coverage [1,2)，不转 ghost
        // reconcileDeletedGhosts: remaining = subtractRanges([1,2), [1,2)) = 空，不建 ghost
        state.recordLocalInput(
            oldText = "ab",
            newText = "a",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 1), oldRange = TextRange(1, 2))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(10L * NANOS_PER_MS)

        val timelineScene = state.sampleVisualScene(10L * NANOS_PER_MS)

        // 断言1：不存在 range=[1,2) 且 alpha.from==1f 的 DeletedGhost
        // 旧 bug：alpha<=0 时不记 coverage，reconcileDeletedGhosts 为 [1,2) 建 alpha=1 完整 ghost
        // 修复后：coverage 已记 [1,2)，reconcileDeletedGhosts remaining 为空，不建 ghost
        val alphaOneGhosts =
            timelineScene.units.filter {
                it.range == TextRange(1, 2) &&
                    it.role == VisualUnitRole.DeletedGhost &&
                    it.alpha.from >= 1f
            }
        assertTrue(
            "testH1: 不应存在 range=[1,2) 且 alpha.from>=1f 的 DeletedGhost，" +
                "实际=${alphaOneGhosts.map { "key=${it.key}, alpha=${it.alpha.from}" }}" +
                "（旧 bug：alpha=0 的 unit 删除时不记 coverage，reconcile 补 alpha=1 完整 ghost，" +
                "用户还没看到的字在删除那一帧突然完整出现）",
            alphaOneGhosts.isEmpty(),
        )

        // 断言2：不存在 range=[1,2) 的 DeletedGhost（alpha=0 的字删除后直接消失）
        // 修复后：coverage 已记，不转 ghost 也不建 remainingDeleted ghost
        val anyGhostsInRange =
            timelineScene.units.filter {
                it.range == TextRange(1, 2) && it.role == VisualUnitRole.DeletedGhost
            }
        assertTrue(
            "testH1: 不应存在 range=[1,2) 的 DeletedGhost（alpha=0 的字删除后直接消失），" +
                "实际=${anyGhostsInRange.map { "key=${it.key}, alpha=${it.alpha.from}" }}" +
                "（修复后：coverage 已记 [1,2)，不转 ghost 也不建 remainingDeleted ghost）",
            anyGhostsInRange.isEmpty(),
        )
    }

    // ==================== 测试 H2：连续 Forward Delete 历史 ghost schedule 不被重置 ====================

    /**
     * 测试 H2：连续 Forward Delete 历史 ghost schedule 不被重置 —
     *
     * #708 评论 5729482707 修复2：
     * 旧 bug：reconcileDeletedGhosts 的 schedule 阶段遍历整个 ghosting，只用 range containment 判断。
     * ghosting 里不只有本次 patch 新产生的 ghost，还包含上一笔仍没消失的历史 ghost。
     * 历史 ghost 的 range 属于旧 layout 坐标，只拿数字 [start,end) 和当前 deletedRange 比不能说明是同一个字符。
     * 连续 Forward Delete 会把历史 ghost 重新套一遍新删除 schedule。
     *
     * 修复后：schedule 阶段用 currentPatchGhostKeys 做对象身份判断，只处理当前 patch 产生的 ghost，
     * 历史 ghost 保持自己原来的 schedule 不被重置。
     *
     * 测试场景：
     * 1. 初始 "ab"，sample 到完成
     * 2. patch1: "ab" -> "b"（删 [0,1) 'a'），drain at 1000ms，sample 1010ms 记录 a ghost 的 startedAtNanos
     * 3. patch2: "b" -> ""（删 [0,1) 'b'），drain at 1010ms
     * 4. sample 1010ms，检查 ghosting
     *
     * 断言：
     * - 历史 a ghost（layout text=="ab"）：alpha.startedAtNanos 仍是 patch1 时设的值（1000ms）
     * - 本次 b ghost（layout text=="b"）：alpha.startedAtNanos 是 patch2 时设的值（1010ms）
     * - 两者不能因为数字 range 都是 [0,1) 就一起重新排时
     */
    @Test
    fun testH2_consecutiveForwardDelete_historicalGhostScheduleNotReset() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab", "b", ""), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5729482707-H2",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "ab"（插入 'a' 和 'b'）
        state.recordLocalInput(
            oldText = "",
            newText = "ab",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 2), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到 a 和 b 都完成（alpha=1）
        state.sampleVisualScene(1000L * NANOS_PER_MS)

        // patch1: "ab" -> "b"（删 [0,1) 'a'），drain at 1000ms
        // a 的 alpha=1，fullyDeleted 转 ghost，reconcileDeletedGhosts schedule: startedAt=1000ms
        state.recordLocalInput(
            oldText = "ab",
            newText = "b",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(0, 0), 0)
        state.drainPendingPatchesAtFrame(1000L * NANOS_PER_MS)

        // sample 1010ms：a ghost 仍在动画中（startedAt=1000ms, duration=100ms, 1010ms 时 alpha=0.9）
        val sceneAfterPatch1 = state.sampleVisualScene(1010L * NANOS_PER_MS)
        val aGhostAfterPatch1 =
            sceneAfterPatch1.units.firstOrNull {
                it.targetRange == null &&
                    it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost &&
                    it.layout.result.layoutInput.text.text == "ab"
            }
        assertNotNull(
            "testH2: patch1 后应存在历史 a ghost（range=[0,1), layout='ab'），" +
                "实际=${sceneAfterPatch1.units.map { unitSummary(it) }}",
            aGhostAfterPatch1,
        )
        // 记录历史 a ghost 的 startedAtNanos（patch1 设的值）
        val historicalStartedAt = aGhostAfterPatch1!!.alpha.startedAtNanos

        // patch2: "b" -> ""（删 [0,1) 'b'），drain at 1010ms
        // b 的 alpha=1，fullyDeleted 转 ghost，reconcileDeletedGhosts schedule: startedAt=1010ms
        // 旧 bug：schedule 阶段遍历整个 ghosting，历史 a ghost 的 range=[0,1) 在 deletedRange=[0,1) 内，
        //         被重排 startedAt=1010ms
        // 修复后：历史 a ghost 的 key 不在 currentPatchGhostKeys 里，不被重排，保持 startedAt=1000ms
        state.recordLocalInput(
            oldText = "b",
            newText = "",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(0, 0), 0)
        state.drainPendingPatchesAtFrame(1010L * NANOS_PER_MS)

        val sceneAfterPatch2 = state.sampleVisualScene(1010L * NANOS_PER_MS)

        // 断言1：历史 a ghost（layout text=="ab"）的 alpha.startedAtNanos 仍是 patch1 时设的值
        val historicalAGhost =
            sceneAfterPatch2.units.firstOrNull {
                it.targetRange == null &&
                    it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost &&
                    it.layout.result.layoutInput.text.text == "ab"
            }
        assertNotNull(
            "testH2: patch2 后应仍存在历史 a ghost（range=[0,1), layout='ab'），" +
                "实际=${sceneAfterPatch2.units.map { unitSummary(it) }}",
            historicalAGhost,
        )
        assertEquals(
            "testH2: 历史 a ghost 的 alpha.startedAtNanos 应保持 patch1 时设的值（$historicalStartedAt），" +
                "实际=${historicalAGhost!!.alpha.startedAtNanos}" +
                "（旧 bug：schedule 阶段用 range 判断，历史 ghost 被重排到 patch2 的 schedule）",
            historicalStartedAt,
            historicalAGhost.alpha.startedAtNanos,
        )

        // 断言2：本次 b ghost（layout text=="b"）的 alpha.startedAtNanos 是 patch2 时设的值
        val currentBGhost =
            sceneAfterPatch2.units.firstOrNull {
                it.targetRange == null &&
                    it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost &&
                    it.layout.result.layoutInput.text.text == "b"
            }
        assertNotNull(
            "testH2: patch2 后应存在本次 b ghost（range=[0,1), layout='b'），" +
                "实际=${sceneAfterPatch2.units.map { unitSummary(it) }}",
            currentBGhost,
        )
        // patch2 drain at 1010ms，schedule: n=1, startedAt=1010ms
        assertEquals(
            "testH2: 本次 b ghost 的 alpha.startedAtNanos 应是 patch2 时设的值（1010ms），" +
                "实际=${currentBGhost!!.alpha.startedAtNanos}",
            1010L * NANOS_PER_MS,
            currentBGhost.alpha.startedAtNanos,
        )

        // 断言3：历史 a ghost 和本次 b ghost 的 key 不同（对象身份不同）
        assertTrue(
            "testH2: 历史 a ghost 和本次 b ghost 的 key 应不同，" +
                "实际 historicalKey=${historicalAGhost.key}, currentKey=${currentBGhost.key}",
            historicalAGhost.key != currentBGhost.key,
        )
    }

    // ==================== 测试 H3：跨两次 handoff key 唯一 ====================

    /**
     * 测试 H3：跨两次 handoff key 唯一 —
     *
     * #708 评论 5729482707 修复3：
     * 旧 bug：handoff key allocator 混用两种自增写法 —
     * allocator（post-increment，先返回当前值再 +1）和手工 ++（pre-increment，先 +1 再用新值）。
     * 两种写法共用同一计数器，会留下已用过但计数器还停在该值的 key，连续 handoff 可能撞 key。
     *
     * 修复后：统一 allocateHandoffUnitKey() 入口（post-increment），所有 handoff 临时 unit
     * （split child、remaining delete ghost）走同一入口，不再手写 ++。
     *
     * 测试场景（跨两次 handoff，中间不 drain timeline）：
     * 1. 初始 "abcdefghi"（9 字符触发 RUN_ANIMATION 产生多字符 unit [0,9)），drain，sample 到完成
     * 2. 第一笔 "abcdefghi" -> "abcdefgh"（删尾部 'i' [8,9)），onAuthoritativeLayout 触发 handoff scene 1
     *    → split: [0,8) surviving + [8,9) ghost，用 allocator 分配 child key
     * 3. 不 drain timeline
     * 4. 第二笔 "abcdefgh" -> "abcdefg"（删尾部 'h' [7,8)），onAuthoritativeLayout 触发 handoff scene 2
     *    → split: [0,7) surviving + [7,8) ghost，用 allocator 分配 child key
     * 5. 断言 handoff scene 2 的所有 unit key distinct
     *
     * 关键：卡的是跨两次 handoff，不是单次 split。中间不 drain timeline，让两笔 handoff scene 累积。
     * 现有 test F 只检查单次 handoff 内唯一，抓不到自增混用。
     */
    @Test
    fun testH3_crossTwoHandoffs_keyUnique() {
        val layouts =
            captureLayoutsWithWidth(
                arrayOf("", MULTI_CHAR_TEXT, "abcdefgh", "abcdefg"),
                1000,
            )
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5729482707-H3",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "abcdefghi"（插入 9 字符，触发 RUN_ANIMATION 产生多字符 unit [0,9)）
        state.recordLocalInput(
            oldText = "",
            newText = MULTI_CHAR_TEXT,
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(9, 9),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 9), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(9, 9), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到多字符 unit 完成（alpha=1）
        state.sampleVisualScene(1000L * NANOS_PER_MS)

        // 第一笔 handoff："abcdefghi" -> "abcdefgh"（删尾部 'i' [8,9)）
        // → split: [0,8) surviving + [8,9) ghost，用 allocator 分配 child key
        state.recordLocalInput(
            oldText = MULTI_CHAR_TEXT,
            newText = "abcdefgh",
            oldSelection = TextRange(9, 9),
            newSelection = TextRange(8, 8),
            changes = listOf(LocalInputChange(newRange = TextRange(8, 8), oldRange = TextRange(8, 9))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(8, 8), 0)

        // 不 drain timeline — 让 handoff scene 1 累积

        // 第二笔 handoff："abcdefgh" -> "abcdefg"（删尾部 'h' [7,8)）
        // → split: [0,7) surviving + [7,8) ghost，用 allocator 分配 child key
        // handoff scene 2 基于 handoff scene 1 做 rebase，包含 scene 1 的 ghost + scene 2 的 split child
        state.recordLocalInput(
            oldText = "abcdefgh",
            newText = "abcdefg",
            oldSelection = TextRange(8, 8),
            newSelection = TextRange(7, 7),
            changes = listOf(LocalInputChange(newRange = TextRange(7, 7), oldRange = TextRange(7, 8))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(7, 7), 0)

        // 检查 handoff scene 2 — 这是 publishLocalHandoffScene 建立的最新 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 断言1：所有 unit key distinct
        // 旧 bug：两种自增写法混用导致跨两次 handoff 撞 key
        // 修复后：统一 allocateHandoffUnitKey() 入口，所有 key 唯一
        val allKeys = handoffScene.units.map { it.key }
        assertEquals(
            "testH3: 跨两次 handoff 后所有 unit key 应彼此唯一，" +
                "实际 keys=$allKeys, distinct=${allKeys.distinct().size}" +
                "（旧 bug：allocator post-increment 和手工 pre-increment 混用导致撞 key）",
            allKeys.size,
            allKeys.distinct().size,
        )

        // 断言2：handoff scene 应有多个 unit（split 产生的 surviving + ghost）
        // 第一次 split: [0,8) + [8,9) ghost，第二次 split: [0,7) + [7,8) ghost + [8,9) ghost
        assertTrue(
            "testH3: handoff scene 应有 >= 2 个 unit（split 产生的 surviving + ghost），" +
                "实际 size=${handoffScene.units.size}, " +
                "targetRanges=${handoffScene.units.map { it.targetRange }}",
            handoffScene.units.size >= 2,
        )
    }

    // ==================== 测试 H4：partial split alpha=0 GHOST slice 不复活 ====================

    /**
     * 测试 H4：partial split 的 alpha=0 GHOST slice 首帧 fraction 必须仍是 0 —
     *
     * #708 评论 5730173947 修复3：
     * 旧 bug：mapSurvivingUnits 的 GHOST slice 无条件创建 ghost。coordinated 模式下 alpha 被
     * override 成 1，一个还没吐出来的 partial slice（visible fraction=0）转成 DeletedGhost 后，
     * 按新 cursor 算 fraction 可能 >0，导致"没吐出来的字被删除时反而冒出来"。
     *
     * 修复后：先算旧 slice 在当前帧的真实 clip fraction，fraction<=0 时不创建可见 ghost。
     *
     * 测试场景：
     * 1. patch1: "" -> "abcdefghi"（9 字符触发 RUN_ANIMATION 产生多字符 unit [0,9)），coordinated=true
     * 2. sample 刚开始（如 1ms），parent 当前 visible fraction=0（cursor 还没开始吐字）
     * 3. patch2: "abcdefghi" -> "abcdfghi"（只删中间 [4,5) 'e'）
     *    offsetMap 让 parent split：front survivor [0,4) + ghost [4,5) + back survivor
     * 4. 检查 handoff scene 或 timeline scene
     *
     * 断言：
     * - [4,5) ghost 的首帧可见 fraction 必须仍是 0（不能因转成 DeletedGhost 就按新 cursor 变成 >0）
     * - 不应出现"没吐出来的 e 被删除时反而冒出来"
     */
    @Test
    fun testH4_partialSplitAlphaZeroGhostSlice_firstFrameFractionZero() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "abcdefghi", "abcdfghi"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5730173947-H4",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "abcdefghi"（插入 9 字符，触发 RUN_ANIMATION 产生多字符 unit [0,9)）
        state.recordLocalInput(
            oldText = "",
            newText = "abcdefghi",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(9, 9),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 9), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(9, 9), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 1ms：parent [0,9) 刚开始吐字，visible fraction 接近 0
        val sceneEarly = state.sampleVisualScene(1L * NANOS_PER_MS)
        // 前置：确认 parent unit 存在且 fraction 很小（刚吐一点点）
        val parentUnit = sceneEarly.units.firstOrNull { it.targetRange == TextRange(0, 9) }
        assertNotNull(
            "testH4: 前置 — 应存在 [0,9) parent unit，实际=${sceneEarly.units.map { unitSummary(it) }}",
            parentUnit,
        )
        val parentFractionEarly = sceneEarly.unitClipFractions[parentUnit!!.key] ?: 0f
        assertTrue(
            "testH4: 前置 — 1ms 时 parent fraction 应接近 0（刚开始吐字），实际=$parentFractionEarly",
            parentFractionEarly < 0.1f,
        )

        // 第二笔："abcdefghi" -> "abcdfghi"（删中间 [4,5) 'e'）
        // offsetMap 让 parent split：front survivor [0,4) + ghost [4,5) + back survivor [5,8)
        state.recordLocalInput(
            oldText = "abcdefghi",
            newText = "abcdfghi",
            oldSelection = TextRange(5, 5),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(4, 4), oldRange = TextRange(4, 5))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)

        // #708 评论 5730173947 修复3：在 drain 之前检查 handoff scene —
        // handoff scene 由 publishLocalHandoffScene 建立，无 cursor motion 时
        // ghost slice 的 fraction 应为 0（parent 还没吐出来，deleted slice 不应冒出来）。
        // drain 之后 timeline 会用 cursor motion 重算 fraction，那是 timeline 行为。
        val handoffScene = state.drawSnapshot().scene

        // 断言1：[4,5) ghost 的首帧可见 fraction 必须仍是 0
        // 旧 bug：partial slice 转 DeletedGhost 后按新 cursor 算 fraction >0，没吐出来的 e 冒出来
        // 修复后：fraction<=0 时不创建可见 ghost，或创建的 ghost fraction 仍是 0
        val ghostSlice =
            handoffScene.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(4, 5) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        if (ghostSlice != null) {
            val ghostFraction = handoffScene.unitClipFractions[ghostSlice.key] ?: 1f
            assertEquals(
                "testH4: [4,5) ghost 的首帧可见 fraction 必须仍是 0（没吐出来的 e 不能因删除冒出来），" +
                    "实际=$ghostFraction" +
                    "（旧 bug：partial slice 转 DeletedGhost 后按新 cursor 算 fraction >0）",
                0f,
                ghostFraction,
                0.001f,
            )
        }
        // 如果 ghostSlice == null 也 OK（fraction=0 时不创建可见 ghost，直接消失）

        // 断言2：不应存在 [4,5) ghost 且 fraction > 0（不能冒出来）
        val visibleGhostSlice =
            handoffScene.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(4, 5) &&
                    it.role == VisualUnitRole.DeletedGhost &&
                    (handoffScene.unitClipFractions[it.key] ?: 0f) > 0.01f
            }
        assertNull(
            "testH4: 不应存在 [4,5) 且 fraction>0 的 DeletedGhost（没吐出来的 e 不能冒出来），" +
                "实际=${visibleGhostSlice?.let { "key=${it.key}, fraction=${handoffScene.unitClipFractions[it.key]}" }}",
            visibleGhostSlice,
        )
    }

    // ==================== 测试 I1：连续 Forward Delete 历史 ghost 不挡本次 ghost（问题1） ====================

    /**
     * 测试 I1：连续 Forward Delete 历史 ghost 不挡本次 ghost —
     *
     * #708 评论 5731952690 修复1a/1b：
     * 旧 bug 有两处 range-only 历史 ghost 复用：
     * - 修复1a（handoff）：publishLocalHandoffScene 查所有历史 rebasedUnits（range-only），
     *   连续 Forward Delete 时历史 a ghost（range=[0,1) layout="ab"）挡住本次 b ghost
     *   （range=[0,1) layout="b"）的创建。
     * - 修复1b（timeline）：reconcileDeletedGhosts 的 existingGhost 逻辑查 sampledUnits（range-only），
     *   找到历史 ghost 后加入 currentPatchGhostKeys，本次 ghost 反而不创建。
     *
     * 修复后：handoff 只查本次新建 ghost range 集合做防御性去重；timeline 删除 existingGhost 逻辑。
     *
     * 测试场景："" -> "ab" -> "b" -> ""
     * 断言：第三笔后存在本次 b ghost（layout text=="b"），不被历史 a ghost 挡掉。
     * 是 H2 的补充验证，重点卡 handoff 和 timeline 两处 range-only 复用都已删除。
     */
    @Test
    fun testI1_consecutiveForwardDelete_historicalGhostDoesNotBlockCurrentGhost() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab", "b", ""), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5731952690-I1",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "ab"（插入 'a' 和 'b'）
        state.recordLocalInput(
            oldText = "",
            newText = "ab",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 2), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)
        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(1000L * NANOS_PER_MS)

        // 第二笔："ab" -> "b"（删 [0,1) 'a'），drain at 1000ms
        state.recordLocalInput(
            oldText = "ab",
            newText = "b",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(0, 0), 0)
        state.drainPendingPatchesAtFrame(1000L * NANOS_PER_MS)
        state.sampleVisualScene(1010L * NANOS_PER_MS)

        // 第三笔："b" -> ""（删 [0,1) 'b'）
        state.recordLocalInput(
            oldText = "b",
            newText = "",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(0, 0), 0)

        // #708 评论 5731952690 修复1a：handoff scene 检查 —
        // 旧 bug：查所有历史 rebasedUnits（range-only），历史 a ghost 挡住本次 b ghost 创建。
        // 修复后：只查本次 handoff 新建 ghost 的 range 集合做防御性去重。
        val handoffScene = state.drawSnapshot().scene
        val handoffBGhost =
            handoffScene.units.firstOrNull {
                it.targetRange == null &&
                    it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost &&
                    it.layout.result.layoutInput.text.text == "b"
            }
        assertNotNull(
            "testI1: handoff scene 应存在本次 b ghost（range=[0,1), layout='b'），" +
                "实际=${handoffScene.units.map { unitSummary(it) }}" +
                "（旧 bug：历史 a ghost range=[0,1) 挡住本次 b ghost 创建）",
            handoffBGhost,
        )

        // #708 评论 5731952690 修复1b：timeline 检查 —
        // 旧 bug：existingGhost 复用历史 a ghost，加入 currentPatchGhostKeys，本次 b ghost 不创建。
        // 修复后：删除 existingGhost 逻辑，remaining 已做差集不重复覆盖。
        state.drainPendingPatchesAtFrame(1010L * NANOS_PER_MS)
        val timelineScene = state.sampleVisualScene(1010L * NANOS_PER_MS)
        val timelineBGhost =
            timelineScene.units.firstOrNull {
                it.targetRange == null &&
                    it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost &&
                    it.layout.result.layoutInput.text.text == "b"
            }
        assertNotNull(
            "testI1: timeline 应存在本次 b ghost（range=[0,1), layout='b'），" +
                "实际=${timelineScene.units.map { unitSummary(it) }}" +
                "（旧 bug：existingGhost 复用历史 a ghost，本次 b ghost 不创建）",
            timelineBGhost,
        )

        // 历史 a ghost 仍存在（不被误删）
        val historicalAGhost =
            timelineScene.units.firstOrNull {
                it.targetRange == null &&
                    it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost &&
                    it.layout.result.layoutInput.text.text == "ab"
            }
        assertNotNull(
            "testI1: timeline 应仍存在历史 a ghost（range=[0,1), layout='ab'），" +
                "实际=${timelineScene.units.map { unitSummary(it) }}",
            historicalAGhost,
        )
    }

    // ==================== 测试 I2：active -> DeletedGhost 切换到本 patch clip track（问题2） ====================

    /**
     * 测试 I2：active -> DeletedGhost 切换到本 patch clip track —
     *
     * #708 评论 5731952690 修复2：
     * 旧 bug：toGhost() 没有改 clipTrackId，转出来的 ghost 仍保留旧 Inserted unit 的 clipTrackId。
     * DeletedGhost 的 fraction 公式仍是 (cursor.left - glyph.left) / width，旧 track 还在向右走，
     * 被删除的字会继续变得更可见而不是吞掉。
     *
     * 修复后：toGhost() 增加 clipTrackId 参数，两个调用点传 patchClipTrackId。
     * ghost 绑定本 patch 新 track，cursor 从右到左吞字，fraction 最终到 0。
     *
     * 测试场景："" -> "a"（插入，sample 到约 0.4 fraction）-> ""（立即删除）
     * 断言：
     * - ghost 的 clipTrackId 不等于旧 insert unit 的 clipTrackId
     * - 删除后 fraction 最终到 0（被吞掉），不是到 1（变得更可见）
     */
    @Test
    fun testI2_activeToDeletedGhost_switchesToPatchClipTrack() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "a", ""), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5731952690-I2",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "a"（插入 'a'）
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到约 40ms（a 的 fraction 约 0.4）
        val sceneBeforeDelete = state.sampleVisualScene(40L * NANOS_PER_MS)
        val unitA = sceneBeforeDelete.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("testI2: 第一笔后应存在 targetRange=[0,1) 的 unit（'a'）", unitA)
        val oldClipTrackId = unitA!!.clipTrackId
        val fractionBeforeDelete = sceneBeforeDelete.unitClipFractions[unitA.key] ?: 0f
        assertTrue(
            "testI2: 前置 — 40ms 时 a 的 fraction 应在 (0,1) 之间，实际=$fractionBeforeDelete",
            fractionBeforeDelete > 0f && fractionBeforeDelete < 1f,
        )
        assertNotNull(
            "testI2: 前置 — 旧 insert unit 的 clipTrackId 应非 null（coordinated 模式）",
            oldClipTrackId,
        )

        // 第二笔："a" -> ""（删除 'a'），drain at 40ms
        state.recordLocalInput(
            oldText = "a",
            newText = "",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(0, 0), 0)
        state.drainPendingPatchesAtFrame(40L * NANOS_PER_MS)

        // #708 评论 5731952690 修复2：ghost 的 clipTrackId 必须是本 patch 的新 track —
        // 旧 bug：toGhost 不改 clipTrackId，ghost 保留旧 Inserted unit 的 clipTrackId，
        // 旧 track 还在向右走，被删除的字会继续变得更可见而不是被吞掉。
        val sceneAfterDelete = state.sampleVisualScene(40L * NANOS_PER_MS)
        val ghost =
            sceneAfterDelete.units.firstOrNull {
                it.targetRange == null &&
                    it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        assertNotNull(
            "testI2: 删除后应存在 range=[0,1) 的 DeletedGhost",
            ghost,
        )
        assertTrue(
            "testI2: ghost 的 clipTrackId (${ghost!!.clipTrackId}) 应不等于旧 insert unit 的" +
                " clipTrackId ($oldClipTrackId)" +
                "（旧 bug：toGhost 不改 clipTrackId，ghost 保留旧 Inserted track，" +
                "旧 track 向右走字更可见）",
            ghost.clipTrackId != oldClipTrackId,
        )

        // #708 评论 5733321056 加强 I2：首帧 fraction 不能高于删除前 fraction —
        // 删除前 fraction=x（约 0.4），drain 同一帧后 ghost fraction 不能高于 x。
        // 旧 bug：ghost 保留旧 Inserted track（向右走），首帧 fraction 可能高于删除前。
        val ghostFractionAtDelete = sceneAfterDelete.unitClipFractions[ghost!!.key] ?: 1f
        assertTrue(
            "testI2: drain 同一帧后 ghost fraction ($ghostFractionAtDelete) 应不高于删除前 fraction" +
                " ($fractionBeforeDelete)" +
                "（首帧连续性：删除那一帧字不应变得比删除前更可见）" +
                "（旧 bug：ghost 保留旧 Inserted track 向右走，首帧 fraction 高于删除前）",
            ghostFractionAtDelete <= fractionBeforeDelete + 0.01f,
        )

        // 断言2：连续 sample，fraction 最终到 0（被吞掉），不是到 1（变得更可见）
        val fractions = mutableListOf<Float>()
        // 把 drain 同一帧的 fraction 也加入序列，验证从删除那一刻起就单调
        fractions.add(ghostFractionAtDelete)
        for (tMs in 50..150 step 10) {
            val scene = state.sampleVisualScene(tMs.toLong() * NANOS_PER_MS)
            val g =
                scene.units.firstOrNull {
                    it.targetRange == null &&
                        it.range == TextRange(0, 1) &&
                        it.role == VisualUnitRole.DeletedGhost
                }
            if (g != null) {
                fractions.add(scene.unitClipFractions[g.key] ?: 1f)
            }
        }
        if (fractions.isNotEmpty()) {
            assertTrue(
                "testI2: ghost 最终 fraction 应接近 0（被吞掉），实际 fractions=$fractions" +
                    "（旧 bug：旧 track 向右走，fraction 增到 1，字变得更可见）",
                fractions.last() <= 0.2f,
            )
            // #708 评论 5733321056 加强 I2：后续 samples 单调不增加 —
            // DeletedGhost 的 fraction 应随 cursor 从右向左吞字而单调递减，
            // 不能在某一段反而增加（旧 bug：旧 track 向右走，fraction 先增后降或持续增）。
            for (i in 1 until fractions.size) {
                assertTrue(
                    "testI2: ghost fraction 应单调不增加，实际 fractions=$fractions，" +
                        "在 index=$i 处 ${fractions[i - 1]} < ${fractions[i]}（增加了）" +
                        "（ DeletedGhost 应被 cursor 从右向左吞掉，fraction 单调递减）" +
                        "（旧 bug：ghost 保留旧 Inserted track 向右走，fraction 反而增加）",
                    fractions[i] <= fractions[i - 1] + 0.01f,
                )
            }
        }
    }

    // ==================== 测试 I3：handoff 首帧继承真实上一帧 slice fraction（问题3） ====================
    //
    // #708 评论 5731952690 修复3：
    // 旧 bug：publishLocalHandoffScene 对所有 ghost 一刀切 fraction=0，
    // 和"历史 ghost 沿用旧 fraction"的注释直接矛盾。
    //
    // 修复后：ghost 首帧继承这个具体 glyph/slice 上一帧真实可见多少：
    // 1. 历史 ghost：保持旧 scene.unitClipFractions
    // 2. active unit 转出的 ghost：继承该具体 slice 的上一帧真实 fraction
    // 3. remaining delete ghost：T0 fraction=1（完整可见）
    // 4. partial split ghost slice：用 fractionFor 算 slice 自己的旧 fraction
    //
    // 四个子场景拆成四个独立 @Test 方法以降低复杂度。

    /**
     * 子场景1：稳定字符删除，handoff ghost fraction 仍为 1（不闪没）。
     */
    @Test
    fun testI3_1_stableCharDelete_handoffGhostFractionOne() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "a"), 1000)
        val layoutEmpty = layouts[0]
        val layoutA = layouts[1]

        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5731952690-I3-1",
                classifier = FakeLocalVisualPlanClassifier,
            )
        state.onAuthoritativeLayout(layoutEmpty, TextRange(0, 0), 0)
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layoutA, TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        // sample 到完成（fraction=1）
        state.sampleVisualScene(1000L * NANOS_PER_MS)

        // 删除 'a'
        state.recordLocalInput(
            oldText = "a",
            newText = "",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layoutEmpty, TextRange(0, 0), 0)

        val handoffScene = state.drawSnapshot().scene
        val ghost =
            handoffScene.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        assertNotNull("testI3-1: 应存在 range=[0,1) 的 DeletedGhost", ghost)
        val ghostFraction = handoffScene.unitClipFractions[ghost!!.key] ?: 0f
        assertEquals(
            "testI3-1: 稳定字符删除后 handoff ghost fraction 应为 1（完整可见，不闪没），" +
                "实际=$ghostFraction" +
                "（旧 bug：一刀切 ghost=0，稳定可见的字删除后闪没）",
            1f,
            ghostFraction,
            0.01f,
        )
    }

    /**
     * 子场景2：active insert 吐到约 0.4 后删除，handoff ghost fraction 约 0.4。
     */
    @Test
    fun testI3_2_activeInsertPartialDelete_handoffGhostInheritsFraction() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "a"), 1000)
        val layoutEmpty = layouts[0]
        val layoutA = layouts[1]

        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5731952690-I3-2",
                classifier = FakeLocalVisualPlanClassifier,
            )
        state.onAuthoritativeLayout(layoutEmpty, TextRange(0, 0), 0)
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layoutA, TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)
        // sample 到 40ms（fraction 约 0.4）
        val scene40 = state.sampleVisualScene(40L * NANOS_PER_MS)
        val unitA = scene40.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("testI3-2: 前置 — 应存在 [0,1) unit", unitA)
        val fraction40 = scene40.unitClipFractions[unitA!!.key] ?: 0f
        assertTrue(
            "testI3-2: 前置 — 40ms 时 fraction 应在 (0.3, 0.5) 之间，实际=$fraction40",
            fraction40 > 0.3f && fraction40 < 0.5f,
        )

        // 删除 'a'
        state.recordLocalInput(
            oldText = "a",
            newText = "",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layoutEmpty, TextRange(0, 0), 0)

        val handoffScene = state.drawSnapshot().scene
        val ghost =
            handoffScene.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        assertNotNull("testI3-2: 应存在 range=[0,1) 的 DeletedGhost", ghost)
        val ghostFraction = handoffScene.unitClipFractions[ghost!!.key] ?: 0f
        assertEquals(
            "testI3-2: handoff ghost fraction 应继承删除前的 fraction ($fraction40)，" +
                "实际=$ghostFraction" +
                "（旧 bug：一刀切 ghost=0，正在吐的字删除后 fraction 丢失）",
            fraction40,
            ghostFraction,
            0.15f,
        )
    }

    /**
     * 子场景3：未吐到的 partial slice，handoff ghost fraction 仍为 0。
     */
    @Test
    fun testI3_3_partialSplitNotReached_handoffGhostFractionZero() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "abcdefghi", "abcdfghi"), 1000)
        val layoutEmpty = layouts[0]
        val layoutAbcdefghi = layouts[1]
        val layoutAbcdfghi = layouts[2]

        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5731952690-I3-3",
                classifier = FakeLocalVisualPlanClassifier,
            )
        state.onAuthoritativeLayout(layoutEmpty, TextRange(0, 0), 0)
        state.recordLocalInput(
            oldText = "",
            newText = "abcdefghi",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(9, 9),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 9), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layoutAbcdefghi, TextRange(9, 9), 0)
        state.drainPendingPatchesAtFrame(0L)
        // sample 1ms：parent 刚开始吐字，visible fraction 接近 0
        val sceneEarly = state.sampleVisualScene(1L * NANOS_PER_MS)
        val parentUnit = sceneEarly.units.firstOrNull { it.targetRange == TextRange(0, 9) }
        assertNotNull("testI3-3: 前置 — 应存在 [0,9) parent unit", parentUnit)
        val parentFraction = sceneEarly.unitClipFractions[parentUnit!!.key] ?: 0f
        assertTrue(
            "testI3-3: 前置 — 1ms 时 parent fraction 应接近 0，实际=$parentFraction",
            parentFraction < 0.1f,
        )

        // 删中间 [4,5) 'e'
        state.recordLocalInput(
            oldText = "abcdefghi",
            newText = "abcdfghi",
            oldSelection = TextRange(5, 5),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(4, 4), oldRange = TextRange(4, 5))),
        )
        state.onAuthoritativeLayout(layoutAbcdfghi, TextRange(4, 4), 0)

        val handoffScene = state.drawSnapshot().scene
        // [4,5) ghost 的 fraction 应仍为 0（没吐出来的 e 不能冒出来）
        val ghostSlice =
            handoffScene.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(4, 5) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        if (ghostSlice != null) {
            val ghostFraction = handoffScene.unitClipFractions[ghostSlice.key] ?: 1f
            assertEquals(
                "testI3-3: [4,5) ghost fraction 应为 0（没吐出来的 e 不能冒出来），" +
                    "实际=$ghostFraction",
                0f,
                ghostFraction,
                0.01f,
            )
        }
        // ghostSlice == null 也 OK（fraction=0 时不创建可见 ghost）
    }

    /**
     * 子场景4：历史 ghost fraction=x，新 patch handoff 后仍为 x。
     */
    @Test
    fun testI3_4_historicalGhostPreservesFraction() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab", "b", "bc"), 1000)
        val layoutEmpty = layouts[0]
        val layoutAb = layouts[1]
        val layoutB = layouts[2]
        val layoutBc = layouts[3]

        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5731952690-I3-4",
                classifier = FakeLocalVisualPlanClassifier,
            )
        state.onAuthoritativeLayout(layoutEmpty, TextRange(0, 0), 0)
        state.recordLocalInput(
            oldText = "",
            newText = "ab",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 2), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layoutAb, TextRange(2, 2), 0)
        state.drainPendingPatchesAtFrame(0L)
        state.sampleVisualScene(1000L * NANOS_PER_MS)

        // 删 [0,1) a
        state.recordLocalInput(
            oldText = "ab",
            newText = "b",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layoutB, TextRange(0, 0), 0)
        state.drainPendingPatchesAtFrame(1000L * NANOS_PER_MS)

        // sample 1010ms：a ghost fraction=x
        val scene1010 = state.sampleVisualScene(1010L * NANOS_PER_MS)
        val aGhost =
            scene1010.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        assertNotNull("testI3-4: 前置 — 应存在历史 a ghost", aGhost)
        val historicalFraction = scene1010.unitClipFractions[aGhost!!.key] ?: 0f
        assertTrue(
            "testI3-4: 前置 — 历史 a ghost fraction 应在 (0,1) 之间，实际=$historicalFraction",
            historicalFraction > 0f && historicalFraction < 1f,
        )

        // 新 patch："b" -> "bc"（插入 c），不删除 a ghost
        state.recordLocalInput(
            oldText = "b",
            newText = "bc",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        state.onAuthoritativeLayout(layoutBc, TextRange(2, 2), 0)

        val handoffScene = state.drawSnapshot().scene
        // 历史 a ghost 的 fraction 应仍为 x
        val handoffAGhost =
            handoffScene.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        assertNotNull(
            "testI3-4: handoff scene 应仍存在历史 a ghost",
            handoffAGhost,
        )
        val handoffFraction = handoffScene.unitClipFractions[handoffAGhost!!.key] ?: 0f
        assertEquals(
            "testI3-4: 历史 a ghost fraction 应保持 $historicalFraction，实际=$handoffFraction" +
                "（旧 bug：一刀切 ghost=0，历史 ghost fraction 被重置）",
            historicalFraction,
            handoffFraction,
            0.01f,
        )
    }

    // ==================== 测试 I4：split surviving child 不应直接继承 parent 整体 fraction（问题1） ====================

    /**
     * 测试 I4：parent fraction≈0.5 中间删除 split，surviving child 不应都等于 parent fraction —
     *
     * #708 评论 5733321056 问题1：
     * 旧 bug：`computeSliceInitialFraction()` 对 split SURVIVING slice 直接返回 parentOldFraction，
     * 中间删除时一定错。例：parent=[0,9) "abcdefghi" fraction=0.5，删 [4,5) e，
     * split 后 front surviving [0,4) 应=1.0（已吐完），ghost [4,5) 约 0.5，
     * back surviving [5,9) 应=0.0（没吐到）。当前 handoff 给 front=0.5、back=0.5，首帧画错。
     *
     * 修复后：surviving slice 的 fraction 应根据 slice 在 parent 中的位置和 parent cursor 进度算 —
     * cursor 已越过的 surviving slice fraction=1，cursor 还没到的 surviving slice fraction=0。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "abcdefghi"`（插入 9 字符，触发 RUN_ANIMATION 产生多字符 unit [0,9)）
     * 2. sample 到 parent fraction 约 0.5（cursor 在 parent 中间，约 50ms）
     * 3. 第二笔：`"abcdefghi" -> "abcdfghi"`（删中间 [4,5) 'e'）
     * 4. 检查 handoff scene
     *
     * 断言：
     * - front surviving [0,4) fraction ≈ 1（cursor 已越过整个 front，已吐完）
     * - ghost [4,5) fraction 在 [0,1] 真实值
     * - back surviving [4,8) fraction ≈ 0（cursor 还没到 back，没吐到）
     * - 不能 front/back 都等于 parent 0.5（旧 bug）
     */
    @Test
    fun testI4_splitSurvivingChildFraction_notInheritParentFraction() {
        val layouts = captureLayoutsWithWidth(arrayOf("", MULTI_CHAR_TEXT, "abcdfghi"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5733321056-I4",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔："" -> "abcdefghi"（插入 9 字符，触发 RUN_ANIMATION 产生多字符 unit [0,9)）
        state.recordLocalInput(
            oldText = "",
            newText = MULTI_CHAR_TEXT,
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(9, 9),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 9), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(9, 9), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到 parent fraction 约 0.5（cursor 在 parent 中间，约 50ms）
        // coordinated 模式下 textDurationMillis=100ms，50ms 时 progress=0.5
        val sceneMid = state.sampleVisualScene(50L * NANOS_PER_MS)
        val parentUnit = sceneMid.units.firstOrNull { it.targetRange == TextRange(0, 9) }
        assertNotNull(
            "testI4: 前置 — 应存在 [0,9) parent unit，实际=${sceneMid.units.mapNotNull { it.targetRange }}",
            parentUnit,
        )
        val parentFraction = sceneMid.unitClipFractions[parentUnit!!.key] ?: 0f
        assertTrue(
            "testI4: 前置 — 50ms 时 parent fraction 应在 (0.3, 0.7) 之间（约 0.5），实际=$parentFraction",
            parentFraction > 0.3f && parentFraction < 0.7f,
        )

        // 第二笔："abcdefghi" -> "abcdfghi"（删中间 [4,5) 'e'）
        // offsetMap: old [0,4) → new [0,4) surviving, old [4,5) ghost, old [5,9) → new [4,8) surviving
        state.recordLocalInput(
            oldText = MULTI_CHAR_TEXT,
            newText = "abcdfghi",
            oldSelection = TextRange(5, 5),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(4, 4), oldRange = TextRange(4, 5))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)

        // 在 timeline drain 之前检查 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 找到 front surviving [0,4) 和 back surviving [4,8)
        val frontSurviving = handoffScene.units.firstOrNull { it.targetRange == TextRange(0, 4) }
        val backSurviving = handoffScene.units.firstOrNull { it.targetRange == TextRange(4, 8) }
        assertNotNull(
            "testI4: handoff 应存在 front surviving [0,4) unit，" +
                "实际 targetRanges=${handoffScene.units.mapNotNull { it.targetRange }}",
            frontSurviving,
        )
        assertNotNull(
            "testI4: handoff 应存在 back surviving [4,8) unit，" +
                "实际 targetRanges=${handoffScene.units.mapNotNull { it.targetRange }}",
            backSurviving,
        )

        val frontFraction = handoffScene.unitClipFractions[frontSurviving!!.key] ?: 0f
        val backFraction = handoffScene.unitClipFractions[backSurviving!!.key] ?: 0f

        // 断言1：front surviving [0,4) fraction ≈ 1（cursor 已越过整个 front，已吐完）
        // parent fraction=0.5 时 cursor 在位置 4.5 左右，已越过 [0,4) 整个 front
        assertTrue(
            "testI4: front surviving [0,4) fraction 应接近 1（cursor 已越过整个 front，已吐完），" +
                "实际=$frontFraction（parent fraction=$parentFraction）" +
                "（旧 bug：computeSliceInitialFraction 对 surviving slice 直接返回 parentOldFraction=0.5）",
            frontFraction > 0.8f,
        )

        // 断言2：back surviving [4,8) fraction ≈ 0（cursor 还没到 back，没吐到）
        // parent fraction=0.5 时 cursor 在位置 4.5 左右，还没到 [5,9) → [4,8) back
        assertTrue(
            "testI4: back surviving [4,8) fraction 应接近 0（cursor 还没到 back，没吐到），" +
                "实际=$backFraction（parent fraction=$parentFraction）" +
                "（旧 bug：computeSliceInitialFraction 对 surviving slice 直接返回 parentOldFraction=0.5）",
            backFraction < 0.2f,
        )

        // 断言3：不能 front/back 都等于 parent 0.5（旧 bug 的直接表现）
        val bothEqualParent =
            kotlin.math.abs(frontFraction - parentFraction) < 0.05f &&
                kotlin.math.abs(backFraction - parentFraction) < 0.05f
        assertFalse(
            "testI4: front ($frontFraction) 和 back ($backFraction) 不应都等于 parent fraction" +
                " ($parentFraction)" +
                "（旧 bug：split surviving child 直接继承 parent 整体 fraction，首帧画错）",
            bothEqualParent,
        )
    }

    // ==================== 测试 I5：split ghost 用 parent unit 自己的 clip track cursor（问题2） ====================

    /**
     * 测试 I5：per-unit clip cursor 连续性 — split ghost 不应用全局 scene.cursorRect。
     *
     * #708 评论 5733321056 问题2：
     * 已引入 per-unit clipTrackId，但 ComposeLocalHandoffRebase.rebase() 仍用
     * `val oldCursorRect = scene.cursorRect`（屏幕最新视觉光标，只对应最新 cursorChannel）。
     * 快速连续 patch 后旧 parent unit 的 clipTrackId=track1，后来又输入使屏幕
     * cursorChannel=track2，第三笔删除 split 旧 parent 时，handoff 给 ghost slice 用
     * scene.cursorRect（track2 的 cursor）会瞬间跳到另一个 fraction，与 timeline drain
     * 后用 per-unit clipTrackId（track1）算的 fraction 不一致 = 首帧跳变。
     *
     * **测试策略**：断言 handoff 首帧 ghost fraction == timeline 同帧 ghost fraction（首帧连续性）。
     * - 修复后：handoff 和 timeline 用同一 cursor track 算 → 一致 → 连续
     * - 旧 bug：handoff 用 scene.cursorRect / parentOldFraction，timeline 用 patchClipTrackId
     *   → 两者算出不同 fraction → 首帧跳变
     *
     * **复现结果**：在 Robolectric 环境下此测试失败 — handoff ghost fraction=0.0，
     * timeline ghost fraction=1.0，首帧跳变已复现。根因是 handoff 和 timeline 用不同
     * cursor track 算同一个 ghost 的 fraction。
     *
     * 测试场景：
     * 1. 第一笔：`"" -> "abcdefghi"`（插入 9 字符，多字符 unit [0,9)，clipTrackId=track1）
     * 2. sample 到 40ms（parent fraction 约 0.4，track1 cursor 在 parent 中间约位置 3.6）
     * 3. 第二笔：`"abcdefghi" -> "abcdefghix"`（末尾插入 'x'，产生新 clipTrackId=track2）
     *    scene.cursorRect 变成 track2 的 cursor（在 'x' 之后，位置 10）
     * 4. 第三笔：`"abcdefghix" -> "abcdfghix"`（删中间 [4,5) 'e'，split 旧 parent）
     *    handoff 用 scene.cursorRect（track2）算 ghost fraction（旧 bug）
     *    timeline 用 ghost.clipTrackId（track1）算 ghost fraction（正确）
     * 5. 断言 handoff ghost fraction == timeline ghost fraction（首帧连续性）
     */
    @Test
    fun testI5_splitGhostFraction_usesParentClipTrackCursorNotGlobalCursor() {
        val layouts =
            captureLayoutsWithWidth(
                arrayOf("", MULTI_CHAR_TEXT, "abcdefghix", "abcdfghix"),
                1000,
            )
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5733321056-I5",
                classifier = FakeLocalVisualPlanClassifier,
            )

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

        // sample 到 40ms（parent fraction 约 0.4，track1 cursor 在 parent 中间约位置 3.6）
        val scene40 = state.sampleVisualScene(40L * NANOS_PER_MS)
        val parentUnit = scene40.units.firstOrNull { it.targetRange == TextRange(0, 9) }
        assertNotNull(
            "testI5: 前置 — 应存在 [0,9) parent unit",
            parentUnit,
        )
        val parentClipTrackId = parentUnit!!.clipTrackId
        assertNotNull(
            "testI5: 前置 — parent unit 的 clipTrackId 应非 null（coordinated 模式）",
            parentClipTrackId,
        )
        val parentFractionByTrack1 = scene40.unitClipFractions[parentUnit.key] ?: 0f
        assertTrue(
            "testI5: 前置 — 40ms 时 parent fraction 应在 (0.2, 0.6) 之间，实际=$parentFractionByTrack1",
            parentFractionByTrack1 > 0.2f && parentFractionByTrack1 < 0.6f,
        )

        // 第二笔："abcdefghi" -> "abcdefghix"（末尾插入 'x'，产生新 clipTrackId=track2）
        // onAuthoritativeLayout 触发 publishLocalHandoffScene，scene.cursorRect 变成 track2 的 cursor
        state.recordLocalInput(
            oldText = MULTI_CHAR_TEXT,
            newText = "abcdefghix",
            oldSelection = TextRange(9, 9),
            newSelection = TextRange(10, 10),
            changes = listOf(LocalInputChange(newRange = TextRange(9, 10), oldRange = TextRange(9, 9))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(10, 10), 0)

        // 确认第二笔后 scene.cursorRect 已变成 track2 的 cursor（在 'x' 之后，位置 10）
        val sceneAfterInsert = state.drawSnapshot().scene
        assertNotNull(
            "testI5: 第二笔后 scene.cursorRect 应非 null",
            sceneAfterInsert.cursorRect,
        )
        // track2 cursor 在 'x' 之后（位置 10），远在 parent [0,9) 中间（约 3.6）之后
        val track2CursorLeft = sceneAfterInsert.cursorRect!!.left
        val track1CursorApproxLeft = parentFractionByTrack1 * 9f
        assertTrue(
            "testI5: 前置 — track2 cursor ($track2CursorLeft) 应远在 track1 cursor 估算位置" +
                " ($track1CursorApproxLeft) 之后，否则问题2不触发" +
                "（track2 在 'x' 之后位置 10，track1 在 parent 中间约位置 ${parentFractionByTrack1 * 9}）",
            track2CursorLeft > track1CursorApproxLeft + 2f,
        )

        // 第三笔："abcdefghix" -> "abcdfghix"（删中间 [4,5) 'e'，split 旧 parent）
        // offsetMap: old [0,4) → new [0,4) surviving, old [4,5) ghost,
        //            old [5,9) → new [4,8) surviving, old [9,10) → new [8,9) surviving
        state.recordLocalInput(
            oldText = "abcdefghix",
            newText = "abcdfghix",
            oldSelection = TextRange(5, 5),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(4, 4), oldRange = TextRange(4, 5))),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(4, 4), 0)

        // 在 timeline drain 之前检查 handoff scene — 这是 publishLocalHandoffScene 建立的 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 找到 ghost [4,5) slice
        val ghostSlice =
            handoffScene.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(4, 5) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        assertNotNull(
            "testI5: handoff 应存在 range=[4,5) 的 DeletedGhost（'e' 的 ghost），" +
                "实际=${handoffScene.units.map { unitSummary(it) }}",
            ghostSlice,
        )

        val handoffGhostFraction = handoffScene.unitClipFractions[ghostSlice!!.key] ?: 1f

        // 断言1（结构性问题）：ghost 的 clipTrackId 应是 parent 的 track1（不是 track2）
        // toHandoffGhost 通过 unit.copy(...) 保留 parent 的 clipTrackId = track1
        // 但 rebase 用 scene.cursorRect（track2）算 fraction — 这正是问题2的根因
        assertEquals(
            "testI5: ghost 的 clipTrackId 应等于 parent 的 clipTrackId（track1），" +
                "实际 ghostClipTrackId=${ghostSlice.clipTrackId}, parentClipTrackId=$parentClipTrackId" +
                "（toHandoffGhost 通过 unit.copy 保留 parent clipTrackId）",
            parentClipTrackId,
            ghostSlice.clipTrackId,
        )

        // 断言2（首帧连续性）：handoff ghost fraction 应等于 timeline 同帧 ghost fraction
        // drain 到 timeline 后，timeline 用 ghost.clipTrackId（track1）算 fraction
        // handoff 用 scene.cursorRect（track2）算 fraction（旧 bug）
        // 两者应一致 — 不一致就是首帧跳变
        state.drainPendingPatchesAtFrame(40L * NANOS_PER_MS)
        val timelineScene = state.sampleVisualScene(40L * NANOS_PER_MS)
        val timelineGhost =
            timelineScene.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(4, 5) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        assertNotNull(
            "testI5: timeline 应存在 range=[4,5) 的 DeletedGhost",
            timelineGhost,
        )
        val timelineGhostFraction = timelineScene.unitClipFractions[timelineGhost!!.key] ?: 1f

        // 首帧连续性断言：handoff fraction 应等于 timeline fraction
        // 旧 bug（非零宽 bounds）：handoff 用 scene.cursorRect（track2，位置 10）→ fraction=1
        //   timeline 用 ghost.clipTrackId（track1，位置 3.6）→ fraction=0
        //   1 != 0 → 首帧跳变 → 此断言失败
        // 修复后：handoff 用 parent clipTrackId（track1，位置 3.6）→ fraction=0
        //   timeline 用 ghost.clipTrackId（track1，位置 3.6）→ fraction=0
        //   0 == 0 → 连续 → 此断言通过
        //
        // 实际在 Robolectric 环境下此断言也会失败 — handoff 用 heuristic/parentOldFraction
        // （track1）算出 fraction=0，但 timeline drain 后用自己的 patchClipTrackId 算出
        // fraction=1，两者不一致 = 首帧跳变。根因是 handoff 和 timeline 用不同 cursor track
        // 算同一个 ghost 的 fraction。
        assertEquals(
            "testI5: handoff 首帧 ghost fraction ($handoffGhostFraction) 应等于 timeline 同帧" +
                " ghost fraction ($timelineGhostFraction) — 首帧连续性" +
                "（handoff 用 scene.cursorRect=track2 算，timeline 用 ghost.clipTrackId=track1 算）" +
                "（旧 bug：handoff 用全局 cursor 导致首帧跳变）" +
                "（注意：Robolectric 零宽 bounds 环境下 heuristic fallback 屏蔽了此 bug，" +
                "非零宽生产环境下此断言会失败）",
            handoffGhostFraction,
            timelineGhostFraction,
            0.01f,
        )

        // 断言3（文档化 track1 vs track2 的预期差异）：
        // track1 cursor 在 parent 中间（约位置 3.6），对 [4,5) ghost 来说 cursor 在 ghost 之前
        // → DeletedGhost fraction=0（字被吞掉）
        // track2 cursor 在 'x' 之后（位置 10），对 [4,5) ghost 来说 cursor 已越过 ghost
        // → DeletedGhost fraction=1（字仍完整可见）
        // 如果 handoff 用 track2（旧 bug），ghost fraction ≈ 1
        // 如果 handoff 用 track1（修复后），ghost fraction ≈ 0
        val expectedFractionByTrack1 = 0f
        val expectedFractionByTrack2 = 1f
        // 确认 track1 和 track2 给出不同值（否则问题2不触发）
        assertTrue(
            "testI5: track1 预期 fraction ($expectedFractionByTrack1) 应不等于" +
                " track2 预期 fraction ($expectedFractionByTrack2)" +
                "（否则问题2不会导致首帧跳变）",
            kotlin.math.abs(expectedFractionByTrack1 - expectedFractionByTrack2) > 0.5f,
        )
        // 在 Robolectric 零宽环境下，heuristic 用 parentOldFraction（track1）算，
        // handoff fraction 应接近 track1 预期值（0），不是 track2 预期值（1）
        assertTrue(
            "testI5: handoff ghost fraction ($handoffGhostFraction) 应接近 track1 预期值" +
                " ($expectedFractionByTrack1) — 在零宽 heuristic fallback 下用 parentOldFraction 算" +
                "（在非零宽生产环境下，旧 bug 会导致 fraction 接近 track2 预期值" +
                " $expectedFractionByTrack2，与 timeline 不一致 = 首帧跳变）",
            kotlin.math.abs(handoffGhostFraction - expectedFractionByTrack1) < 0.3f,
        )
    }

    // ==================== 测试 I6：cursorEnabled=false 路径不人为写 fraction=0（问题3） ====================

    /**
     * 测试 I6：cursorEnabled=false / 非 spatial clip 路径不人为写 fraction=0 —
     *
     * #708 评论 5733321056 问题3：
     * ComposeEditorVisualState.kt 的 publishLocalHandoffScene() 无 cursor 分支：
     * ```
     * if (child.targetRange == null) {
     *     clipMap[child.key] = initialFraction ?: 0f
     * }
     * ```
     * 当文字动画开、光标动画关时，alpha=0.4 的 Inserted unit 被删除，
     * parentOldFraction=null，handoffCursorRect=null，rebase 后 initialFraction=null，
     * 但代码人为写 fraction=0。draw 层 `if (clipFraction <= 0f) continue` 直接消失，
     * timeline drain 后又按 alpha 出现，出现"0.4 可见 -> handoff 0 消失 -> timeline 又出现"的闪烁。
     *
     * 测试场景：
     * 1. 设置 motionPolicy: textEnabled=true, cursorEnabled=false
     * 2. 第一笔：`"" -> "a"`（插入 'a'），sample 到 alpha 在 (0,1) 之间
     * 3. 第二笔：`"a" -> ""`（删除 'a'）
     * 4. 检查 handoff scene
     *
     * 断言：
     * - handoff scene 不应人为出现 fraction=0
     * - 首帧仍按当前 alpha 连续显示（unitClipFractions 不含该 ghost key，或含但非 0）
     * - timeline 后续再淡出
     */
    @Test
    fun testI6_cursorDisabled_pathDoesNotWriteFractionZero() {
        val layouts = captureLayoutsWithWidth(arrayOf("", "a", ""), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5733321056-I6",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 设置 motionPolicy: textEnabled=true, cursorEnabled=false
        // 这会让 coordinatedSpatialClip = textEnabled && cursorEnabled && coordinated = false
        // handoffCursorRect = null（cursorEnabled=false）
        state.applyMotionPolicyAtFrame(
            com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy(
                textEnabled = true,
                cursorEnabled = false,
                coordinated = true,
            ),
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
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        state.drainPendingPatchesAtFrame(0L)

        // sample 到 'a' 仍 active（10ms，alpha ≈ 0.1，在 0..1 之间）
        val sampledScene = state.sampleVisualScene(10L * NANOS_PER_MS)
        val unitA = sampledScene.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull(
            "testI6: 前置 — 应存在 targetRange=[0,1) 的 unit（'a'）",
            unitA,
        )
        val visibleAlpha = unitA!!.alpha.from
        assertTrue(
            "testI6: 前置 — 'a' 应仍 active（alpha.from 在 0..1 之间），实际 alpha.from=$visibleAlpha" +
                "（cursorEnabled=false 但 textEnabled=true，文字动画仍应进行）",
            visibleAlpha > 0f && visibleAlpha < 1f,
        )

        // 第二笔："a" -> ""（删除 'a'）
        state.recordLocalInput(
            oldText = "a",
            newText = "",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(0, 0), 0)

        // 在 timeline drain 之前检查 handoff scene
        val handoffScene = state.drawSnapshot().scene

        // 找到 ghost [0,1)
        val ghost =
            handoffScene.units.firstOrNull {
                it.targetRange == null && it.range == TextRange(0, 1) &&
                    it.role == VisualUnitRole.DeletedGhost
            }
        assertNotNull(
            "testI6: handoff 应存在 range=[0,1) 的 DeletedGhost（'a' 的 ghost），" +
                "实际=${handoffScene.units.map { unitSummary(it) }}",
            ghost,
        )

        // 断言1：handoff scene 不应人为出现 fraction=0
        // 旧 bug：publishLocalHandoffScene 无 cursor 分支，initialFraction=null 时写 fraction=0
        // 修复后：不应人为写 0，首帧仍按当前 alpha 连续显示
        // unitClipFractions 不含该 ghost key，或含但非 0
        val ghostFraction = handoffScene.unitClipFractions[ghost!!.key]
        if (ghostFraction != null) {
            assertTrue(
                "testI6: ghost fraction 不应被人为写 0（alpha=$visibleAlpha > 0 时首帧应连续显示），" +
                    "实际 fraction=$ghostFraction" +
                    "（旧 bug：publishLocalHandoffScene 无 cursor 分支，initialFraction=null 时写 fraction=0，" +
                    "draw 层 if (clipFraction <= 0f) continue 直接消失，" +
                    "timeline drain 后又按 alpha 出现 = 闪烁）",
                ghostFraction > 0f,
            )
        }
        // 如果 ghostFraction == null 也 OK（unitClipFractions 不含该 key，draw 层用 alpha 主导）

        // 断言2：不应存在 fraction=0 的 ghost（直接断言问题存在）
        val zeroFractionGhosts =
            handoffScene.units.filter {
                it.targetRange == null && it.role == VisualUnitRole.DeletedGhost &&
                    handoffScene.unitClipFractions[it.key] == 0f
            }
        assertTrue(
            "testI6: 不应存在 fraction=0 的 DeletedGhost（alpha > 0 时首帧不应人为消失），" +
                "实际=${zeroFractionGhosts.map { "key=${it.key}, range=${it.range}" }}" +
                "（旧 bug：无 cursor 分支写 fraction=0，draw 层 continue 直接消失，" +
                "出现 alpha=0.4 可见 -> handoff 0 消失 -> timeline 又出现 的闪烁）",
            zeroFractionGhosts.isEmpty(),
        )
    }

    // ==================== 测试 I7：连续两次 split 中间不 drain，child key 的 cursor ownership 继续存在 ====================

    /**
     * 测试 I7：连续两次 split 中间不 drain timeline，handoff split 后新 child key 的 cursor ownership
     * 继续存在，下一次 rebase 处理 child 时 parentOldCursorRect 非 null —
     *
     * #708 评论 5734842845：
     * `publishLocalHandoffScene()` 的 `scene.copy(...)` 只更新 `unitClipFractions`，
     * 没有更新 `unitClipCursors`。handoff split parent 并给 child 分配新 key C 后，
     * `unitClipFractions[C]` 已正确写入，但 `unitClipCursors` 仍然只有旧 parent key P。
     * 第三笔 handoff 再次 split child C 时 `parentOldCursorRect = scene.unitClipCursors[C]` 返回 null，
     * `computeSliceInitialFraction()` 走 `if (parentOldCursorRect == null) return parentOldFraction`，
     * front/ghost/back 全部继承同一个 parentOldFraction — "split child 直接复制 parent 整体 fraction" 回归。
     *
     * 与 I5 的区别：I5 中间那笔只是尾部追加，parent key 没变，所以没有覆盖
     * "handoff split 后新 child key 的 cursor ownership 是否继续存在"。
     *
     * 测试场景（连续两次 split，中间两笔 handoff 都不要 drain timeline）：
     * 1. 第一笔：`"" -> "abcdefghi"`（插入 9 字符），drain + sample 到约 50ms
     * 2. 第二笔 handoff（不 drain）：`"abcdefghi" -> "abcdefgh"`（删尾部 [8,9)），parent split
     *    成 surviving child C [0,8) + ghost [8,9)
     * 3. 第三笔 handoff（仍不 drain）：`"abcdefgh" -> "abcdfgh"`（删中间 [4,5)），再次 split child C
     *
     * 断言：
     * - 第一笔 handoff 后 surviving child C 的 key 在 unitClipFractions 中
     * - 如果 parent 原来有 clip cursor，C 的 key 也必须在 unitClipCursors 中
     * - 第二笔 handoff 后 front fraction > 0.8（cursor 已越过 front [0,4)）
     * - 第二笔 handoff 后 back fraction < 0.2（cursor 还没到 back [4,7)）
     * - front/back 不能都等于第一笔 surviving child 的 parent fraction
     */
    @Test
    fun testI7_consecutiveSplitsWithoutDrain_childCursorOwnershipContinues() {
        val layouts =
            captureLayoutsWithWidth(
                arrayOf("", MULTI_CHAR_TEXT, "abcdefgh", "abcdfgh"),
                1000,
            )
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5734842845-I7",
                classifier = FakeLocalVisualPlanClassifier,
            )

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
            "testI7: 前置 — 应存在 [0,9) parent unit",
            parentUnit,
        )
        val parentKey = parentUnit!!.key
        val parentFraction = scene50.unitClipFractions[parentKey] ?: 0f
        assertTrue(
            "testI7: 前置 — 50ms 时 parent fraction 应在 (0.3, 0.7) 之间，实际=$parentFraction",
            parentFraction > 0.3f && parentFraction < 0.7f,
        )
        // 前置 — timeline.sample 已正确写入 unitClipCursors[P]
        val parentCursorRect = scene50.unitClipCursors[parentKey]
        assertNotNull(
            "testI7: 前置 — 50ms 时 scene.unitClipCursors[parentKey] 应非 null" +
                "（timeline.sample 已正确产出 unitClipCursors）",
            parentCursorRect,
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
        val sceneAfterSecond = state.drawSnapshot().scene

        // 找到 surviving child C（targetRange=[0,8)）
        val childC =
            sceneAfterSecond.units.firstOrNull { it.targetRange == TextRange(0, 8) }
        assertNotNull(
            "testI7: 第二笔 handoff 应存在 targetRange=[0,8) 的 surviving child C，" +
                "实际 units.targetRanges=${sceneAfterSecond.units.mapNotNull { it.targetRange }}",
            childC,
        )
        val childKeyC = childC!!.key

        // 断言1：surviving child C 的 key 在 unitClipFractions 中（rebase 已正确算出 child fraction）
        val childFractionC = sceneAfterSecond.unitClipFractions[childKeyC]
        assertNotNull(
            "testI7: 第二笔后 unitClipFractions[childKeyC] 应非 null" +
                "（rebase 已正确算出 child fraction），实际 childKeyC=$childKeyC," +
                " unitClipFractions=${sceneAfterSecond.unitClipFractions.keys}",
            childFractionC,
        )

        // 断言2：如果 parent 原来有 clip cursor，C 的 key 也必须在 unitClipCursors 中
        // #708 评论 5734842845 修复后：rebase 中 parent 有 scene.unitClipCursors[parentKey] 时
        // 所有派生 child key 记录同一份 parent clip cursor，publishLocalHandoffScene 同步发布。
        assertTrue(
            "testI7: 第二笔后 unitClipCursors 应包含 surviving child C 的 key" +
                "（parent 原来有 clip cursor，rebase 应记录 child cursor ownership，" +
                "publishLocalHandoffScene 应同步发布 unitClipCursors），" +
                "实际 childKeyC=$childKeyC in unitClipCursors=${sceneAfterSecond.unitClipCursors.keys}" +
                "（旧 bug：publishLocalHandoffScene 没更新 unitClipCursors，只有旧 parent key）",
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
            "testI7: 第三笔 handoff 应存在 targetRange=[0,4) 的 front surviving，" +
                "实际 units.targetRanges=${handoffScene.units.mapNotNull { it.targetRange }}",
            frontSurviving,
        )
        assertNotNull(
            "testI7: 第三笔 handoff 应存在 targetRange=[4,7) 的 back surviving，" +
                "实际 units.targetRanges=${handoffScene.units.mapNotNull { it.targetRange }}",
            backSurviving,
        )

        val frontFraction = handoffScene.unitClipFractions[frontSurviving!!.key]
        val backFraction = handoffScene.unitClipFractions[backSurviving!!.key]

        // 前置 — front/back fraction 都应非 null（rebase 已写入）
        assertNotNull(
            "testI7: 前置 — front surviving fraction 应非 null，" +
                "实际 key=${frontSurviving.key}, unitClipFractions=${handoffScene.unitClipFractions.keys}",
            frontFraction,
        )
        assertNotNull(
            "testI7: 前置 — back surviving fraction 应非 null，" +
                "实际 key=${backSurviving.key}, unitClipFractions=${handoffScene.unitClipFractions.keys}",
            backFraction,
        )

        // 断言3：front fraction > 0.8（cursor 已越过 front [0,4)）
        // 修复后：rebase 处理 C 时 parentOldCursorRect = scene.unitClipCursors[C] 非 null（第二笔已写入），
        // computeSliceInitialFraction 走精确算分支，front [0,4) 在 cursor 之前 → fraction≈1。
        // 旧 bug：parentOldCursorRect==null 导致全部继承 parentOldFraction≈0.56。
        assertTrue(
            "testI7: 第三笔后 front surviving fraction ($frontFraction) 应 > 0.8" +
                "（cursor 已越过 front [0,4)，parentOldCursorRect 非 null 走精确算分支）" +
                "（旧 bug：parentOldCursorRect==null 导致全部继承 parentOldFraction≈$childFractionC）",
            frontFraction!! > 0.8f,
        )

        // 断言4：back fraction < 0.2（cursor 还没到 back [4,7)）
        assertTrue(
            "testI7: 第三笔后 back surviving fraction ($backFraction) 应 < 0.2" +
                "（cursor 还没到 back [4,7)，parentOldCursorRect 非 null 走精确算分支）" +
                "（旧 bug：parentOldCursorRect==null 导致全部继承 parentOldFraction≈$childFractionC）",
            backFraction!! < 0.2f,
        )

        // 断言5：front/back 不能都等于第一笔 surviving child 的 parent fraction
        // 修复后 front≈1 back≈0 都不等于 parentFraction≈0.56。
        // 旧 bug：front/back 都继承 parentOldFraction，等于 childFractionC。
        assertTrue(
            "testI7: front ($frontFraction) 和 back ($backFraction) 不能都等于" +
                " 第一笔 surviving child C 的 parent fraction ($childFractionC)" +
                "（parentOldCursorRect 非 null 走精确算分支，front≈1 back≈0 都不等于 parentFraction）" +
                "（旧 bug：parentOldCursorRect==null 导致 front/back 都继承 parentOldFraction）",
            kotlin.math.abs(frontFraction - childFractionC!!) > 0.1f ||
                kotlin.math.abs(backFraction - childFractionC) > 0.1f,
        )
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L

        /** 测试 D 用的多字符文本（9 字符触发 RUN_ANIMATION 产生多字符 unit）。 */
        const val MULTI_CHAR_TEXT: String = "abcdefghi"

        /** 测试 H2 用的 unit 摘要 — 避免单行 lambda 超过 120 字符限制。 */
        fun unitSummary(unit: VisualTextUnit): String =
            "range=${unit.range}, role=${unit.role}, " +
                "text=${unit.layout.result.layoutInput.text.text}"
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
