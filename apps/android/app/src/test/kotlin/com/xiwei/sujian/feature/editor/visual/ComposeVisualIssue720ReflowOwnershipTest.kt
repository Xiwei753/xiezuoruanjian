package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.EditorSoftBreakProjection
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * Issue #720 评论 5746323050：本地 reflow 所有权收口回归测试 —
 * 本地输入只动画本次真正插入/删除的 glyph；
 * 凡是因为自动换行、硬换行删除或前文长度变化而改变自然位置的幸存文字，
 * 都由 BasicTextField 直接画最终位置。
 *
 * 三组场景：
 * 1. 自动换行 — surviving unit 退出 overlay（[autoReflow_survivingUnit_releasedToBasicTextField]）
 * 2. 删除手动换行 — surviving unit 交还 BasicTextField，deleted ghost 只覆盖真正删除内容
 *    （[deleteNewline_survivingUnit_releasedToBasicTextField]）
 * 3. 几何未变化 — active unit 继续原动画（[geometryUnchanged_survivingUnit_continuesOriginalAnimation]）
 *
 * 另给 #703 评论 5710977972 a3 和 #689 deleteNewline 各补一组 projection 版本，
 * 用 [snapshotFromRawText] 生成含 projection + rawText 的 [ComposeLayoutSnapshot]。
 */
@Suppress(
    "LongMethod",
    "MaxLineLength",
    "LargeClass",
    "LongParameterList",
    "StringLiteralDuplication",
    "TooManyFunctions",
)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue720ReflowOwnershipTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 场景1：换行回流 — surviving unit 退出 overlay ====================

    /**
     * 场景1：已有 active inserted unit，后续输入让它的自然位置从上一行变到下一行；
     * 新 patch 后它必须退出 survivor overlay，不能有 position tween。
     *
     * Robolectric 下 rememberTextMeasurer 不做真实字体度量（软换行不可靠），
     * 所以用硬换行 '\n' 触发几何变化（从第一行变到第二行）—
     * naturalGeometryChanged 的判定逻辑对软换行和硬换行一致。
     *
     * - T0 = "ab"（一行），先插入 'c' 创建 active unit [2,3)
     * - T1 = "abc"（一行），再在 'b' 后插入 '\n' → "ab\nc"（'c' 从第一行变到第二行）
     *
     * 用 [ComposeVisualTimeline] 直接操作。patch.intent = null（本地输入）。
     */
    @Test
    fun autoReflow_survivingUnit_releasedToBasicTextField() {
        val snaps = snapshotsFromRawTexts(listOf("ab", "abc", "ab\nc"), maxWidth = 1000)
        val abLayout = snaps[0]
        val abcLayout = snaps[1]
        val abncLayout = snaps[2]

        // 前置：'abc' 一行，'ab\nc' 跨两行（'c' 被挤到第二行）
        assertTrue(
            "场景1: 'abc' 应一行，实际 lineCount=${abcLayout.result.lineCount}",
            abcLayout.result.lineCount == 1,
        )
        assertTrue(
            "场景1: 'ab\\nc' 应跨两行，实际 lineCount=${abncLayout.result.lineCount}",
            abncLayout.result.lineCount >= 2,
        )

        val timeline = ComposeVisualTimeline()
        val motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true)

        // patch1：插入 'c'，"ab" → "abc"，'c' 成为 active unit [2,3)
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = abLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(2, 3)),
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY)),
                motionPolicy = motionPolicy,
            )
        timeline.applyPatch(patch = patch1, frameTimeNanos = 0L)

        // 确认 'c' 的 active unit 存在
        val sceneAfter1 = timeline.sample(0L)
        val cUnitAfter1 = sceneAfter1.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        assertNotNull(
            "场景1: patch1 后 'c' [2,3) 的 active unit 应存在，实际 units=${sceneAfter1.units.map { "tgt=${it.targetRange}" }}",
            cUnitAfter1,
        )

        // patch2：在 'b' 后插入 '\n'，"abc" → "ab\nc"，'c' 从 [2,3) 映到 [3,4)
        // '\n' 在新正文 [2,3)，'c' 在新正文 [3,4)
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abcLayout,
                newLayout = abncLayout,
                insertedUnits = listOf(TextRange(2, 3)),
                offsetMap =
                    listOf(
                        // "ab"
                        VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                        // 'c' [2,3)→[3,4)
                        VisualOffsetMapEntry(2, 3, 1, VisualOffsetMapKind.SHIFTED),
                    ),
                motionPolicy = motionPolicy,
            )
        timeline.applyPatch(patch = patch2, frameTimeNanos = 20L * NANOS_PER_MS)

        val scene = timeline.sample(20L * NANOS_PER_MS)

        // 'c' 在新正文 "ab\nc" 中是 [3,4)，换行后应已释放给 BasicTextField
        val cSurvivingUnit = scene.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        assertNull(
            "换行后 'c' 的 surviving unit 应已释放给 BasicTextField，不在 overlay units 里，" +
                "实际 units=${scene.units.map { "tgt=${it.targetRange}" }}",
            cSurvivingUnit,
        )
    }

    // ==================== 场景2：删除手动换行 — surviving unit 交还 BasicTextField ====================

    /**
     * 场景2：`ab\ncd` 中 'cd' 处于上一笔 active animation；删除 `\n` 后移动的 surviving unit
     * 必须交还 BasicTextField，deleted ghost 只覆盖真正删除的内容（'\n'）。
     *
     * 用 [ComposeEditorVisualState] 走真实路径 recordLocalInput → onAuthoritativeLayout →
     * drainPendingPatchesAtFrame → sampleVisualScene。
     */
    @Test
    fun deleteNewline_survivingUnit_releasedToBasicTextField() {
        val state =
            ComposeEditorVisualState(
                targetId = "test-720-delete-newline-reflow",
                classifier = FakeLocalVisualPlanClassifier,
            )

        val displayLayouts = displayLayoutsFromRawTexts(listOf("ab\n", "ab\ncd", "abcd"), 1000)

        // 初始 layout "ab\n"
        val (initLayout, initProj) = displayLayouts[0]
        state.onAuthoritativeLayout(initLayout, TextRange(3, 3), 0, projection = initProj, rawText = "ab\n")

        // 插入 'cd'："ab\n" → "ab\ncd"（在末尾插入 'cd'，创建 active unit）
        state.recordLocalInput(
            oldText = "ab\n",
            newText = "ab\ncd",
            oldSelection = TextRange(3, 3),
            newSelection = TextRange(5, 5),
            changes = listOf(LocalInputChange(newRange = TextRange(3, 5), oldRange = TextRange(3, 3))),
        )
        val (abCdLayout, abCdProj) = displayLayouts[1]
        state.onAuthoritativeLayout(abCdLayout, TextRange(5, 5), 0, projection = abCdProj, rawText = "ab\ncd")

        // drain patch1（'cd' 成为 active unit）
        state.drainPendingPatchesAtFrame(0L)

        // 删除 '\n'："ab\ncd" → "abcd"（删除 offset [2,3) 的换行符）
        state.recordLocalInput(
            oldText = "ab\ncd",
            newText = "abcd",
            oldSelection = TextRange(5, 5),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 2), oldRange = TextRange(2, 3))),
        )
        val (abcdLayout, abcdProj) = displayLayouts[2]
        state.onAuthoritativeLayout(abcdLayout, TextRange(4, 4), 0, projection = abcdProj, rawText = "abcd")

        // Issue #720 评论 5747339452：handoff 首帧断言 —
        // onAuthoritativeLayout 会触发 publishLocalHandoffScene，drain 前 drawSnapshot() 能拿到 handoff scene。
        // 'cd' 在新正文 "abcd" 中是 [2,4)，删除换行后从第二行变到第一行（自然几何变化），
        // handoff 首帧就应释放给 BasicTextField：
        // - 'cd' 的 surviving unit 不在 scene.units（没有 targetRange 落在 [2,4) 区间的 unit）
        // - 'cd' 对应的 range 不在 scene.hiddenRanges（BasicTextField 首帧不被裁掉）
        val handoffScene = state.drawSnapshot().scene
        val cdSurvivingInHandoff =
            handoffScene.units.firstOrNull {
                it.targetRange != null && it.targetRange!!.start >= 2 && it.targetRange!!.end <= 4
            }
        assertNull(
            "handoff 首帧：'cd' surviving unit 应已释放给 BasicTextField，不在 handoff scene.units 里，" +
                "实际 units=${handoffScene.units.map { "tgt=${it.targetRange} rng=${it.range}" }}",
            cdSurvivingInHandoff,
        )
        val cdHiddenInHandoff =
            handoffScene.hiddenRanges.any { it.start <= 2 && it.end >= 4 }
        assertTrue(
            "handoff 首帧：'cd' [2,4) 不应在 hiddenRanges（BasicTextField 首帧不被裁掉），" +
                "实际 hiddenRanges=${handoffScene.hiddenRanges}",
            !cdHiddenInHandoff,
        )

        // drain patch2（删除 '\n'，'cd' reflow）
        state.drainPendingPatchesAtFrame(20L * NANOS_PER_MS)

        val scene = state.sampleVisualScene(20L * NANOS_PER_MS)

        // 'cd' 在新正文 "abcd" 中是 [2,4)，删除换行后应已释放给 BasicTextField
        // Issue #720 评论 5747339452：timeline 帧断言 — drain 后 survivor 不会重新出现
        val cdSurviving =
            scene.units.firstOrNull {
                it.targetRange != null && it.targetRange!!.start >= 2 && it.targetRange!!.end <= 4
            }
        assertNull(
            "删除换行后 'cd' surviving unit 应已释放给 BasicTextField，" +
                "实际 units=${scene.units.map { "tgt=${it.targetRange} rng=${it.range}" }}",
            cdSurviving,
        )

        // deleted ghost 只覆盖 '\n' [2,3)（旧正文 "ab\ncd" 坐标）
        val ghosts = scene.units.filter { it.targetRange == null }
        for (ghost in ghosts) {
            assertTrue(
                "deleted ghost 应只覆盖 '\\n' [2,3)，实际 range=${ghost.range}",
                ghost.range.start >= 2 && ghost.range.end <= 3,
            )
        }
    }

    // ==================== 场景3：naturalGeometryChanged helper 单元测试 ====================

    /**
     * 场景3：直接验证 [ComposeVisualRebase.naturalGeometryChanged] 的判定逻辑 —
     * Robolectric 下 rememberTextMeasurer 不做真实字体度量，跨文本比较 bounds 不可靠，
     * 所以不通过 timeline 间接验证"几何未变化 → 保留"，而是直接测 helper：
     *
     * 1. 同一 layout + 同一 range → false（几何未变化）
     * 2. 不同行（硬换行）→ true（几何变化，应释放）
     * 3. 取不到 bounds 的空 range → true（安全释放）
     * 4. Issue #720 评论 5747339452：同一 layout 不同 range（同行水平位移，left 变化但 top 不变）→ true
     *    （修复点1 改成比较 left/top/right/bottom 后，同行水平位移也判定为几何变化，不再只判 top）
     */
    @Test
    fun naturalGeometryChanged_correctlyDetectsReflowAndStability() {
        val snaps = snapshotsFromRawTexts(listOf("abc", "ab\nc"))
        val abcLayout = snaps[0]
        val abncLayout = snaps[1]

        // 1. 同一 layout + 同一 range → false（几何未变化）
        val sameGeometry =
            ComposeVisualRebase.naturalGeometryChanged(
                oldLayout = abcLayout,
                oldRange = TextRange(0, 1),
                newLayout = abcLayout,
                newRange = TextRange(0, 1),
            )
        assertTrue(
            "同一 layout + 同一 range 应判定为几何未变化（false），实际=$sameGeometry",
            !sameGeometry,
        )

        // 2. 'c' 从 "abc" 第一行 [2,3) 变到 "ab\nc" 第二行 [3,4) → true（几何变化）
        val cMovedToSecondLine =
            ComposeVisualRebase.naturalGeometryChanged(
                oldLayout = abcLayout,
                oldRange = TextRange(2, 3),
                newLayout = abncLayout,
                newRange = TextRange(3, 4),
            )
        assertTrue(
            "'c' 从第一行变到第二行应判定为几何变化（true），实际=$cMovedToSecondLine",
            cMovedToSecondLine,
        )

        // 3. 空 range → boundsForRawRange 返回 null → true（安全释放）
        val emptyRange =
            ComposeVisualRebase.naturalGeometryChanged(
                oldLayout = abcLayout,
                oldRange = TextRange(0, 0),
                newLayout = abcLayout,
                newRange = TextRange(0, 0),
            )
        assertTrue(
            "空 range（bounds 为 null）应判定为几何变化（true，安全释放），实际=$emptyRange",
            emptyRange,
        )

        // 4. Issue #720 评论 5747339452：同一 layout "abc"，oldRange=[0,1)（'a'），newRange=[1,2)（'b'）
        //    两者 top 相同（同行）但 left 不同（水平位移）→ 应返回 true（不再只判 top）
        val sameLineHorizontalShift =
            ComposeVisualRebase.naturalGeometryChanged(
                oldLayout = abcLayout,
                oldRange = TextRange(0, 1),
                newLayout = abcLayout,
                newRange = TextRange(1, 2),
            )
        assertTrue(
            "同行水平位移（left 变化但 top 不变）应判定为几何变化（true），实际=$sameLineHorizontalShift" +
                "（Issue #720 评论 5747339452：naturalGeometryChanged 比较完整 left/top/right/bottom，不再只判 top）",
            sameLineHorizontalShift,
        )
    }

    // ==================== 场景1b：同行水平位移 reflow — 修复点1 新行为 ====================

    /**
     * 场景1b：Issue #720 评论 5747339452 修复点1 新行为 —
     * naturalGeometryChanged 改成比较完整 left/top/right/bottom 后，
     * 同行水平位移（前文长度变化但不换行）也判定为几何变化，本地 survivor 释放给 BasicTextField。
     *
     * Robolectric 下 rememberTextMeasurer 不做真实字体度量，软换行不可靠
     *（探测确认 maxWidth=20..50 下 "ab".."abcdef" 均一行），
     * 所以用同行水平位移（前文插入字符，后文同行右移）触发 left 变化 —
     * naturalGeometryChanged 的判定逻辑对软换行和同行位移一致（都走 left/top/right/bottom 比较）。
     *
     * - T0 = "a"（一行），先插入 'b' 创建 active unit [1,2)
     * - T1 = "ab"（一行），再在 'a' 前插入 'x' → "xab"（'b' 同行右移，left 变化但 top 不变）
     *
     * 用 [ComposeVisualTimeline] 直接操作。patch.intent = null（本地输入）。
     */
    @Test
    fun autoReflow_softWrap_survivingUnit_releasedToBasicTextField() {
        // Robolectric 软换行不可靠（探测确认 maxWidth=20..50 均一行），
        // 用同行水平位移触发 left 变化验证修复点1 新行为。
        val snaps = snapshotsFromRawTexts(listOf("a", "ab", "xab"), maxWidth = 1000)
        val aLayout = snaps[0]
        val abLayout = snaps[1]
        val xabLayout = snaps[2]

        // 前置：同行水平位移场景成立（均一行，'b' 同行右移）
        assertTrue(
            "场景1b: 'a' 应一行，实际 lineCount=${aLayout.result.lineCount}",
            aLayout.result.lineCount == 1,
        )
        assertTrue(
            "场景1b: 'ab' 应一行，实际 lineCount=${abLayout.result.lineCount}",
            abLayout.result.lineCount == 1,
        )
        assertTrue(
            "场景1b: 'xab' 应一行，实际 lineCount=${xabLayout.result.lineCount}",
            xabLayout.result.lineCount == 1,
        )

        val timeline = ComposeVisualTimeline()
        val motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true)

        // patch1：插入 'b'，"a" → "ab"，'b' 成为 active unit [1,2)
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = aLayout,
                newLayout = abLayout,
                insertedUnits = listOf(TextRange(1, 2)),
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                motionPolicy = motionPolicy,
            )
        timeline.applyPatch(patch = patch1, frameTimeNanos = 0L)

        // 确认 'b' 的 active unit 存在
        val sceneAfter1 = timeline.sample(0L)
        val bUnitAfter1 = sceneAfter1.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        assertNotNull(
            "场景1b: patch1 后 'b' [1,2) 的 active unit 应存在，实际 units=${sceneAfter1.units.map { "tgt=${it.targetRange}" }}",
            bUnitAfter1,
        )

        // patch2：在 'a' 前插入 'x'，"ab" → "xab"，'b' 从 [1,2) 映到 [2,3)（同行水平位移）
        // 'x' 在新正文 [0,1)，'a' 在 [1,2)，'b' 在 [2,3)
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abLayout,
                newLayout = xabLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                offsetMap =
                    listOf(
                        // 'a' [0,1)→[1,2)
                        VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.SHIFTED),
                        // 'b' [1,2)→[2,3)
                        VisualOffsetMapEntry(1, 2, 1, VisualOffsetMapKind.SHIFTED),
                    ),
                motionPolicy = motionPolicy,
            )
        timeline.applyPatch(patch = patch2, frameTimeNanos = 20L * NANOS_PER_MS)

        val scene = timeline.sample(20L * NANOS_PER_MS)

        // 'b' 在新正文 "xab" 中是 [2,3)，同行右移后应已释放给 BasicTextField
        // Issue #720 评论 5747339452 修复点1：同行水平位移（left 变化）也释放，不再只判 top
        val bSurvivingUnit = scene.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        assertNull(
            "同行水平位移后 'b' 的 surviving unit 应已释放给 BasicTextField（修复点1：left 变化也释放），" +
                "实际 units=${scene.units.map { "tgt=${it.targetRange}" }}",
            bSurvivingUnit,
        )
    }

    // ==================== 场景1c：插入换行 handoff 首帧释放 survivor ====================

    /**
     * 场景1c：Issue #720 评论 5747339452 handoff 首帧释放 —
     * 插入换行触发 reflow 时，handoff 首帧就释放 survivor 给 BasicTextField，
     * 不等 timeline 下一帧才释放。
     *
     * Robolectric 下软换行不可靠（探测确认），用硬换行 '\n' 触发几何变化 —
     * naturalGeometryChanged 的判定逻辑对软换行和硬换行一致。
     *
     * 用 [ComposeEditorVisualState] 走真实路径 recordLocalInput → onAuthoritativeLayout。
     * - 第一笔：建立初始 active unit（"" → "ab" → "abc"）
     * - 第二笔：插入换行（"abc" → "ab\nc"），'c' reflow
     * - onAuthoritativeLayout 后、drain 前，读 state.drawSnapshot().scene，断言 survivor 不在 units 且不在 hiddenRanges
     * - drain 后再断言 timeline 帧 survivor 不重新出现
     */
    @Test
    fun softWrap_handoffFirstFrame_releasesSurvivor() {
        // Robolectric 软换行不可靠（探测确认 maxWidth=20..50 均一行），
        // 用硬换行触发几何变化验证 handoff 首帧释放。
        val displayLayouts = displayLayoutsFromRawTexts(listOf("ab", "abc", "ab\nc"), 1000)

        val state =
            ComposeEditorVisualState(
                targetId = "test-720-softwrap-handoff-first-frame",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout "ab"
        val (initLayout, initProj) = displayLayouts[0]
        state.onAuthoritativeLayout(initLayout, TextRange(2, 2), 0, projection = initProj, rawText = "ab")

        // 第一笔：插入 'c'，"ab" → "abc"（创建 active unit [2,3)）
        state.recordLocalInput(
            oldText = "ab",
            newText = "abc",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(3, 3),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 3), oldRange = TextRange(2, 2))),
        )
        val (abcLayout, abcProj) = displayLayouts[1]
        state.onAuthoritativeLayout(abcLayout, TextRange(3, 3), 0, projection = abcProj, rawText = "abc")

        // drain patch1（'c' 成为 active unit）
        state.drainPendingPatchesAtFrame(0L)

        // 第二笔：在 'b' 后插入 '\n'，"abc" → "ab\nc"（'c' 从第一行变到第二行，reflow）
        state.recordLocalInput(
            oldText = "abc",
            newText = "ab\nc",
            oldSelection = TextRange(3, 3),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 3), oldRange = TextRange(2, 2))),
        )
        val (abncLayout, abncProj) = displayLayouts[2]
        state.onAuthoritativeLayout(abncLayout, TextRange(4, 4), 0, projection = abncProj, rawText = "ab\nc")

        // Issue #720 评论 5747339452：handoff 首帧断言 —
        // onAuthoritativeLayout 会触发 publishLocalHandoffScene，drain 前 drawSnapshot() 能拿到 handoff scene。
        // 'c' 在新正文 "ab\nc" 中是 [3,4)，插入换行后从第一行变到第二行（自然几何变化），
        // handoff 首帧就应释放给 BasicTextField：
        // - 'c' 的 surviving unit 不在 scene.units
        // - 'c' 对应的 range 不在 scene.hiddenRanges（BasicTextField 首帧不被裁掉）
        val handoffScene = state.drawSnapshot().scene
        val cSurvivingInHandoff = handoffScene.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        assertNull(
            "handoff 首帧：'c' [3,4) surviving unit 应已释放给 BasicTextField，不在 handoff scene.units 里，" +
                "实际 units=${handoffScene.units.map { "tgt=${it.targetRange} rng=${it.range}" }}",
            cSurvivingInHandoff,
        )
        val cHiddenInHandoff = handoffScene.hiddenRanges.any { it.start <= 3 && it.end >= 4 }
        assertTrue(
            "handoff 首帧：'c' [3,4) 不应在 hiddenRanges（BasicTextField 首帧不被裁掉），" +
                "实际 hiddenRanges=${handoffScene.hiddenRanges}",
            !cHiddenInHandoff,
        )

        // drain patch2（插入 '\n'，'c' reflow）
        state.drainPendingPatchesAtFrame(20L * NANOS_PER_MS)

        // Issue #720 评论 5747339452：timeline 帧断言 — drain 后 survivor 不会重新出现
        val scene = state.sampleVisualScene(20L * NANOS_PER_MS)
        val cSurviving = scene.units.firstOrNull { it.targetRange == TextRange(3, 4) }
        assertNull(
            "timeline 帧：'c' [3,4) surviving unit 应已释放给 BasicTextField，" +
                "实际 units=${scene.units.map { "tgt=${it.targetRange} rng=${it.range}" }}",
            cSurviving,
        )
    }

    // ==================== projection 版本：#703 评论 5710977972 a3 ====================

    /**
     * #703 评论 5710977972 缺陷2 的 projection 版本 —
     * #717 之后 TextLayoutResult 已经是 display 坐标，不再允许只靠 raw-layout 测试证明换行链正确。
     * 本测试用 [snapshotFromRawText] 生成含 projection 的 [ComposeLayoutSnapshot]。
     *
     * Issue #725 评论 5750735497：停止自绘屏幕 caret 后，CursorMotionPath / cursorFromRect /
     * cursorPath / cursorDurationNanos / scene.cursorRect 已删除，本测试方法已移除。
     *
     * [ComposeVisualTransactionRestartReproTest.deleteNewline_geometryUnchangedUnit_noPositionTrack]
     * 的 projection 版本 — 用 [snapshotFromRawText] 生成含 projection 的 layout。
     *
     * 场景：Insert "" → "ab\nc"，再 Delete "ab\nc" → "abc"。
     * 删换行时 "ab" 在 old/new layout 里位置没变（都在第一行开头），
     * 不应出现在 retainedMoves 里。
     */
    @Test
    fun deleteNewline_geometryUnchangedUnit_noPositionTrack_projectionVersion() {
        val state = ComposeEditorVisualState(targetId = "test-720-delete-newline-projection")

        // === 生成 patch A（Insert "" → "ab\nc"）===
        val displayLayouts = displayLayoutsFromRawTexts(listOf("", "ab\nc", "abc"), 1000)
        val (initLayout, initProj) = displayLayouts[0]
        state.onAuthoritativeLayout(initLayout, TextRange(0, 0), 0, projection = initProj, rawText = "")
        state.onVisualIntent(
            makeInsertIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "",
                newText = "ab\nc",
                newRange = TextRange(0, 4),
                replaceBounds = VisualReplaceBounds(0, 0, 0, 4),
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        val (abncLayout, abncProj) = displayLayouts[1]
        state.onAuthoritativeLayout(abncLayout, TextRange(4, 4), 0, projection = abncProj, rawText = "ab\nc")
        val patchA = state.latestPatch.value
        assertNotNull("patch A 应生成", patchA)

        // === 生成 patch B（Delete "ab\nc" → "abc"）===
        state.onVisualIntent(
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
                                // "ab"
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                                // "c"
                                VisualOffsetMapEntry(3, 2, 1, VisualOffsetMapKind.SHIFTED),
                            ),
                    ),
                // "\n"
                oldRanges = listOf(TextRange(2, 3)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(2, 3, 2, 2),
                expectedOldText = "ab\nc",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        val (abcLayout, abcProj) = displayLayouts[2]
        state.onAuthoritativeLayout(abcLayout, TextRange(3, 3), 0, projection = abcProj, rawText = "abc")
        val patchB = state.latestPatch.value
        assertNotNull("patch B 应生成", patchB)

        // 核心断言：B 的 retainedMoves 只包含真正发生位移的 unit。
        // "ab" 在 old layout（"ab\nc" 第一行）和 new layout（"abc" 第一行）里位置没变，
        // 不应出现在 retainedMoves 里。
        val retainedMoves = patchB!!.retainedMoves
        val abMove = retainedMoves.firstOrNull { it.newRange == TextRange(0, 2) }
        assertNull(
            "'ab' 几何没变，不应出现在 retainedMoves 里（不产生 position track）\n" +
                "Issue #720 projection 版本：删换行时几何没变的存活 unit 不产生 position track",
            abMove,
        )
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L
    }

    /**
     * 批量测量：raw texts → display texts → TextLayoutResults → ComposeLayoutSnapshots。
     *
     * composeRule.setContent 只能调一次，所以一个测试里所有文本必须一次性测量。
     * 默认 selection = TextRange(raw.length, raw.length)（光标在末尾）。
     */
    @Suppress("LongParameterList")
    private fun snapshotsFromRawTexts(
        rawTexts: List<String>,
        maxWidth: Int = 1000,
        fontSizeSp: Float = 14f,
    ): List<ComposeLayoutSnapshot> {
        val projections = rawTexts.map { EditorSoftBreakProjection.fromRawText(it) }
        val displayTexts = rawTexts.zip(projections).map { (raw, proj) -> buildDisplayText(raw, proj) }
        val layouts = measureAllLayouts(displayTexts, maxWidth, fontSizeSp)
        return rawTexts.zip(projections).zip(layouts).map { (rawAndProj, layout) ->
            val (raw, proj) = rawAndProj
            ComposeLayoutSnapshot(layout, TextRange(raw.length, raw.length), 0, proj, raw)
        }
    }

    /**
     * 批量测量：raw texts → (display TextLayoutResults, projections) —
     * 供 [ComposeEditorVisualState.onAuthoritativeLayout] 使用（它接收 TextLayoutResult + projection + rawText）。
     */
    @Suppress("LongParameterList")
    private fun displayLayoutsFromRawTexts(
        rawTexts: List<String>,
        maxWidth: Int = 1000,
        fontSizeSp: Float = 14f,
    ): List<Pair<TextLayoutResult, EditorSoftBreakProjection>> {
        val projections = rawTexts.map { EditorSoftBreakProjection.fromRawText(it) }
        val displayTexts = rawTexts.zip(projections).map { (raw, proj) -> buildDisplayText(raw, proj) }
        val layouts = measureAllLayouts(displayTexts, maxWidth, fontSizeSp)
        return layouts.zip(projections)
    }

    /**
     * 按 [projection.insertPoints] 在 raw text 中插入 U+200B 生成 display text。
     */
    private fun buildDisplayText(
        rawText: String,
        projection: EditorSoftBreakProjection,
    ): String {
        if (projection.insertPoints.isEmpty()) return rawText
        val sb = StringBuilder()
        var prev = 0
        for (insertPoint in projection.insertPoints) {
            sb.append(rawText, prev, insertPoint)
            sb.append(EditorSoftBreakProjection.ZERO_WIDTH_SPACE)
            prev = insertPoint
        }
        sb.append(rawText, prev, rawText.length)
        return sb.toString()
    }

    /**
     * 一次性测量所有文本（composeRule.setContent 只能调一次）。
     */
    private fun measureAllLayouts(
        texts: List<String>,
        maxWidth: Int,
        fontSizeSp: Float,
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

    @Suppress("LongParameterList")
    private fun makePatch(
        id: Long,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>? = null,
        insertedUnits: List<TextRange> = emptyList(),
        deletedUnits: List<TextRange> = emptyList(),
        retainedMoves: List<RetainedMove> = emptyList(),
        durationMs: Long = 100L,
        motionPolicy: EditorMotionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
    ): ComposeVisualPatch =
        ComposeVisualPatch(
            id = id,
            coreTransactionIds = listOf(id),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = offsetMap,
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = retainedMoves,
            durationMs = durationMs,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            motionPolicy = motionPolicy,
            // intent = null 表示本地输入 — naturalGeometryChanged 判定生效
        )

    private fun makeInsertIntent(
        coreTxnId: Long,
        baseRev: Long,
        newRev: Long,
        oldText: String,
        newText: String,
        newRange: TextRange,
        replaceBounds: VisualReplaceBounds,
        offsetMap: VisualOffsetMap? = null,
    ): EditorVisualIntent =
        EditorVisualIntent(
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = offsetMap,
            oldRanges = emptyList(),
            newRanges = listOf(newRange),
            textKind = TextVisualKind.Insert,
            cursor = null,
            replaceBounds = replaceBounds,
            expectedOldText = oldText,
            expectedNewText = newText,
        )
}
