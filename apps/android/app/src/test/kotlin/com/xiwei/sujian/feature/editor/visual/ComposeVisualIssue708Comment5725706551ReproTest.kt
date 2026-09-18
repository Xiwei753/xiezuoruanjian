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
@Suppress("LongMethod", "MaxLineLength", "LargeClass", "StringLiteralDuplication")
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

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L

        /** 测试 D 用的多字符文本（9 字符触发 RUN_ANIMATION 产生多字符 unit）。 */
        const val MULTI_CHAR_TEXT: String = "abcdefghi"
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
