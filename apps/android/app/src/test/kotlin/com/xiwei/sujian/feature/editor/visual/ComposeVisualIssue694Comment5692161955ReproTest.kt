package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.input.EditorInputSnapshot
import com.xiwei.sujian.feature.editor.input.InputSnapshotOutcome
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
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
 * #694 评论 5692161955 复现测试 — 上一轮提交 ab91aaffb 修了 3 个问题后还剩的 3 个
 * 会直接影响真实输入/删除动画的问题：
 *
 * 问题1：一次 InputTransformation 提交多个字，仍然会整块吐出。
 *   ComposeLocalVisualRebase.composeLocalChainInsertedUnits()/composeLocalChainDeletedUnits()
 *   把 changedRangesFromOffsetMap 的 newRanges/oldRanges 直接当动画 unit，
 *   不能处理"一笔里多个 grapheme"。例如 "" -> "我们" 得到 1 个 [0,2) unit，timeline 整体吐出。
 *
 * 问题2：buildLocalInputPatch() 把所有本地输入硬编码成 CLUSTER_ANIMATION。
 *   ComposeEditorVisualState.kt 中 animationMode = AnimationModeDto.CLUSTER_ANIMATION，
 *   丢弃 Core 已做好的视觉分类。Core 规则：0 cluster -> SYSTEM_SUPPRESSED；
 *   含换行 -> LINE_REFLOW_ANIMATION；复杂 grapheme -> CLUSTER_ANIMATION；
 *   <= 8 cluster -> GLYPH_ANIMATION；> 8 cluster -> RUN_ANIMATION。
 *
 * 问题3：composition 活跃时不应该把 Core/external coordinator 基线推进到 preedit。
 *   onAuthoritativeLayout composition 分支调用 frameCoordinator.observePresentedLayout(snapshot)，
 *   把 coordinator.lastConsumed 推到未提交给 Core 的 preedit，导致 Undo 后 external patch 卡死。
 *   另外 observePresentedLayout 只要 pending != null 就不推进，在
 *   "本地输入完成 -> external intent 先到 -> 本地 layout 后到"顺序下会再次卡住。
 *
 * 本测试在当前实现下应 **FAIL**，体现 3 个 bug。修复后应 PASS。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("StringLiteralDuplication", "MaxLineLength")
class ComposeVisualIssue694Comment5692161955ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 问题1：一笔多 grapheme 整块吐出 ====================

    /**
     * 问题1 核心复现：IME 一次提交 "" -> "我们"（一笔含 2 个 grapheme cluster）。
     *
     * 期望：insertedUnits 按 grapheme cluster 拆成 2 个 unit（[0,1) 和 [1,2)），
     * timeline 逐字吐出 "我" -> "们"。
     *
     * 当前实现：composeLocalChainInsertedUnits 对单笔 chain 用 changedRangesFromOffsetMap
     * 算 newRanges = [TextRange(0,2)]（一个连续范围），insertedUnits = 1 个 [0,2)，
     * timeline 把 "我们" 整体吐出。FAIL。
     */
    @Test
    fun singleInputTransformationMultipleGraphemes_shouldSplitByGraphemeCluster() {
        val layouts = captureLayouts("", "我们")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5692161955-p1",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 设置基线：lastPresentedLayout = ""
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 一次 InputTransformation 提交 "" -> "我们"（一笔含 2 个 grapheme）
        // changes 来自 TextFieldBuffer.forEachChange：newRange=[0,2), oldRange=[0,0)
        state.recordLocalInput(
            oldText = "",
            newText = "我们",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 2), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)

        val patch = state.latestPatch.value
        assertNotNull(
            "本地输入 patch 应生成（\"\" -> \"我们\"）",
            patch,
        )
        assertTrue(
            "问题1：一笔 \"\" -> \"我们\" 应按 grapheme cluster 拆成 2 个 insertedUnits，" +
                "实际=${patch!!.insertedUnits}\n" +
                "当前实现：composeLocalChainInsertedUnits 把 newRanges 连续范围直接当 unit，" +
                "得到 1 个 [0,2) 整块，timeline 把 \"我们\" 整体吐出，不是 \"我\" -> \"们\"。\n" +
                "Issue #694 评论 5692161955 问题1：一笔多 grapheme 应按 cluster 拆分",
            patch.insertedUnits.size == 2,
        )
        assertEquals(
            "第 1 个 unit 应是 [0,1)（\"我\"）",
            TextRange(0, 1),
            patch.insertedUnits[0],
        )
        assertEquals(
            "第 2 个 unit 应是 [1,2)（\"们\"）",
            TextRange(1, 2),
            patch.insertedUnits[1],
        )
    }

    // ==================== 问题2：buildLocalInputPatch 硬编码 CLUSTER_ANIMATION ====================

    /**
     * 问题2 核心复现 a：单笔 "" -> "abc" 应使用 GLYPH_ANIMATION（3 cluster <= 8），
     * 不是硬编码 CLUSTER_ANIMATION。
     *
     * 当前实现：buildLocalInputPatch 中 animationMode = AnimationModeDto.CLUSTER_ANIMATION 硬编码，
     * 丢弃 Core 视觉分类。FAIL。
     */
    @Test
    fun singleInputAbc_shouldUseGlyphAnimationNotClusterAnimation() {
        val layouts = captureLayouts("", "abc")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5692161955-p2a",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.recordLocalInput(
            oldText = "",
            newText = "abc",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(3, 3),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 3), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0, compositionActive = false)

        val patch = state.latestPatch.value
        assertNotNull("本地输入 patch 应生成", patch)
        assertEquals(
            "问题2：单笔 \"\" -> \"abc\"（3 cluster <= 8）应使用 GLYPH_ANIMATION，" +
                "实际=${patch!!.animationMode}\n" +
                "当前实现：buildLocalInputPatch 硬编码 animationMode = CLUSTER_ANIMATION，" +
                "丢弃 Core 已做好的视觉分类。\n" +
                "Issue #694 评论 5692161955 问题2：应使用 Core plan 返回的 animationMode",
            AnimationModeDto.GLYPH_ANIMATION,
            patch.animationMode,
        )
    }

    /**
     * 问题2 核心复现 b：单笔 "" -> "abc" 应拆成 3 个 cluster/glyph unit，不是 1 个 [0,3)。
     *
     * 当前实现：单笔多字符没按 grapheme 拆，insertedUnits = 1 个 [0,3)。FAIL。
     */
    @Test
    fun singleInputAbc_shouldSplitIntoThreeClusterUnits() {
        val layouts = captureLayouts("", "abc")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5692161955-p2b",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.recordLocalInput(
            oldText = "",
            newText = "abc",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(3, 3),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 3), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0, compositionActive = false)

        val patch = state.latestPatch.value
        assertNotNull("本地输入 patch 应生成", patch)
        assertTrue(
            "问题2：单笔 \"\" -> \"abc\" 期望 3 个 cluster/glyph unit，" +
                "实际=${patch!!.insertedUnits}（size=${patch.insertedUnits.size}）\n" +
                "当前实现：单笔多字符没按 grapheme 拆，insertedUnits = 1 个 [0,3)。\n" +
                "Issue #694 评论 5692161955 问题2 回归期望：不是一个 [0,3) unit",
            patch.insertedUnits.size == 3,
        )
    }

    /**
     * 问题2 核心复现 c：单笔 "" -> "你好"（IME 一次提交两个汉字）应是 2 个吐字单元。
     *
     * 当前实现：insertedUnits = 1 个 [0,2) 整块。FAIL。
     */
    @Test
    fun singleInputNiHao_shouldSplitIntoTwoClusterUnits() {
        val layouts = captureLayouts("", "你好")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5692161955-p2c",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.recordLocalInput(
            oldText = "",
            newText = "你好",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 2), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)

        val patch = state.latestPatch.value
        assertNotNull("本地输入 patch 应生成", patch)
        assertTrue(
            "问题2：单笔 \"\" -> \"你好\" 期望 2 个吐字单元，" +
                "实际=${patch!!.insertedUnits}（size=${patch.insertedUnits.size}）\n" +
                "当前实现：insertedUnits = 1 个 [0,2) 整块。\n" +
                "Issue #694 评论 5692161955 问题2 回归期望：IME 一次提交两个汉字仍然是两个吐字单元",
            patch.insertedUnits.size == 2,
        )
    }

    /**
     * 问题2 核心复现 d（评论 5692161955 明确要求的回归）：单笔 "" -> "👨‍👩‍👧‍👦"
     * （emoji family：man + ZWJ + woman + ZWJ + girl + ZWJ + boy）应是 **1 个 grapheme unit**。
     *
     * 评论原文："单笔 `"" -> "👨‍👩‍👧‍👦"`：期望整个 emoji family 是 1 个 grapheme unit，不能按 UTF-16 拆"
     *
     * emoji family 在 UTF-16 中是 11 个 char（4 个 emoji 各 2 个 surrogate + 3 个 ZWJ），
     * 不能按 UTF-16 code unit 拆成 11 个或按 UTF-16 char 拆成多个 unit。
     * Unicode grapheme cluster 规则把整个 ZWJ sequence 视为 1 个用户感知字符。
     *
     * 期望：insertedUnits.size == 1（整个 emoji family 是 1 个 grapheme unit）。
     *       animationMode == CLUSTER_ANIMATION（含复杂 grapheme，1 cluster 但 containsComplexGrapheme=true）。
     *
     * 当前实现（修复前）：按 UTF-16 拆会得到多个 unit，或硬编码 CLUSTER_ANIMATION 但 unit 拆分错误。FAIL。
     * 修复后：Core classify_local_visual_plan 用 unicode_segmentation 切 grapheme cluster，
     *         整个 emoji family 是 1 个 cluster，得到 1 个 unit。PASS。
     */
    @Test
    fun singleInputEmojiFamily_shouldBeOneGraphemeUnit() {
        val emojiFamily = "👨‍👩‍👧‍👦" // man + ZWJ + woman + ZWJ + girl + ZWJ + boy
        val emojiUtf16Length = emojiFamily.length // 11 个 UTF-16 code unit
        assertTrue(
            "测试前置：emoji family UTF-16 length 应是 11（4 surrogate pair + 3 ZWJ），实际=$emojiUtf16Length",
            emojiUtf16Length == 11,
        )

        val layouts = captureLayouts("", emojiFamily)
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5692161955-p2d-emoji",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.recordLocalInput(
            oldText = "",
            newText = emojiFamily,
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(emojiUtf16Length, emojiUtf16Length),
            changes =
                listOf(
                    LocalInputChange(
                        newRange = TextRange(0, emojiUtf16Length),
                        oldRange = TextRange(0, 0),
                    ),
                ),
        )
        state.onAuthoritativeLayout(
            layouts[1],
            TextRange(emojiUtf16Length, emojiUtf16Length),
            0,
            compositionActive = false,
        )

        val patch = state.latestPatch.value
        assertNotNull(
            "本地输入 patch 应生成（\"\" -> emoji family）",
            patch,
        )
        assertTrue(
            "问题2 回归：单笔 \"\" -> \"👨‍👩‍👧‍👦\" 期望整个 emoji family 是 1 个 grapheme unit，" +
                "实际=${patch!!.insertedUnits}（size=${patch.insertedUnits.size}）\n" +
                "emoji family = man + ZWJ + woman + ZWJ + girl + ZWJ + boy 是 1 个 Unicode grapheme cluster，" +
                "不能按 UTF-16 拆成 $emojiUtf16Length 个 unit。\n" +
                "Issue #694 评论 5692161955 明确要求：期望整个 emoji family 是 1 个 grapheme unit，不能按 UTF-16 拆",
            patch.insertedUnits.size == 1,
        )
        // 整个 emoji family 应是 1 个 unit 覆盖整个 [0, 11) UTF-16 range
        assertEquals(
            "emoji family 的 1 个 grapheme unit 应覆盖整个 UTF-16 range [0, 11)",
            TextRange(0, emojiUtf16Length),
            patch.insertedUnits[0],
        )
        // 含复杂 grapheme（ZWJ + emoji）-> CLUSTER_ANIMATION
        assertEquals(
            "问题2 回归：emoji family 含复杂 grapheme（ZWJ）应使用 CLUSTER_ANIMATION",
            AnimationModeDto.CLUSTER_ANIMATION,
            patch.animationMode,
        )
    }

    // ==================== 问题3：composition 活跃时误推 coordinator 基线 ====================

    /**
     * 问题3 核心复现 a：composition preedit 期间触发 Undo，external patch 最终应能生成。
     *
     * 场景：
     * 1. 基线 ""
     * 2. 本地输入 "a" + onAuthoritativeLayout("a", compositionActive=false) — 本地 patch 生成
     * 3. composition preedit "an" + onAuthoritativeLayout("an", compositionActive=true)
     *    当前实现：frameCoordinator.observePresentedLayout("an") 把 lastConsumed 推到 "an"
     * 4. Undo intent (baseText="a", targetText="")
     * 5. composition 结束 onAuthoritativeLayout("", compositionActive=false)
     *
     * 期望：Undo patch 能生成（coreTransactionIds 含 Undo id）。
     * 当前实现：lastConsumed="an" != pending.baseText="a"，tryBuildPatch 返回 Empty，Undo patch 卡死。FAIL。
     */
    @Test
    fun compositionPreeditThenUndo_externalPatchShouldNotStall() {
        val layouts = captureLayouts("", "a", "an", "")
        val state =
            ComposeEditorVisualState(
                targetId = "issue694-c5692161955-p3a",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 1. 基线 ""
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 2. 本地输入 "a"
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0, compositionActive = false)
        val localPatch = state.latestPatch.value
        assertNotNull("本地输入 patch 应生成", localPatch)

        // 3. composition preedit "an"（IME 正在 composition，未提交给 Core）
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0, compositionActive = true)

        // 4. Undo: "a" -> ""（Core 驱动，Core 已提交正文 = "a"）
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 200L,
                baseRevision = 1L,
                newRevision = 0L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(0, 1)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 1, newStart = 0, newEnd = 0),
                expectedOldText = "a",
                expectedNewText = "",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // 4.5 bridge outcome: AuthoritativeApplied — Undo 在 composition 期间到达，
        // IME cancel composition，Core 应用了 Undo。
        // #694 评论 5695660885：compositionVisualPhase 必须由 onInputSnapshotResolved 收口，
        // 否则 onAuthoritativeLayout 会因 phase == Composing 进入 AwaitingBridgeResolution 分支直接 return，
        // Undo patch 永远不会生成。真实运行中 bridge outcome 会在 composition cancel 后到达。
        val snapshotEnd =
            EditorInputSnapshot(
                text = "an",
                selection = TextRange(2, 2),
                composition = null,
            )
        state.onInputSnapshotResolved(snapshotEnd, InputSnapshotOutcome.AuthoritativeApplied)

        // 5. composition 结束，权威正文回到 ""
        state.onAuthoritativeLayout(layouts[3], TextRange(0, 0), 0, compositionActive = false)

        // 收集所有已生成 patch 的 coreTransactionIds，判断 Undo patch 是否生成
        val undoPatch = findPatchWithCoreTxnId(state, 200L)
        assertNotNull(
            "问题3：composition preedit 期间触发 Undo，external patch 最终应能生成" +
                "（coreTransactionId=200）\n" +
                "当前实现：composition 活跃时 frameCoordinator.observePresentedLayout 把 lastConsumed " +
                "推到 preedit \"an\"，Undo intent pending.baseText=\"a\" != lastConsumed.text=\"an\"，" +
                "tryBuildPatch 返回 Empty，external patch 卡死。\n" +
                "Issue #694 评论 5692161955 问题3：composition 活跃分支不应调用 observePresentedLayout",
            undoPatch,
        )
    }

    /**
     * 问题3 核心复现 b：observePresentedLayout 在 pending != null 但
     * pending.baseText == presented.text 时应推进 lastConsumed，否则
     * "本地输入完成 -> external intent 先到 -> 本地 layout 后到"顺序会卡住。
     *
     * 场景（直接测 ComposeVisualFrameCoordinator）：
     * 1. onLayout("A") — lastConsumed = "A"
     * 2. onVisualIntent(baseText="AB", targetText="ABC") — pending != null
     * 3. observePresentedLayout("AB") — pending.baseText="AB" == presented.text="AB"
     *    当前实现：pending != null 时不推进，lastConsumed 保持 "A"
     * 4. onLayout("ABC") — tryBuildPatch
     *    当前实现：lastConsumed="A" != pending.baseText="AB"，返回 Empty，卡住
     *    期望：返回 NewPatch
     *
     * 期望：第 4 步 onLayout("ABC") 返回 FrameUpdate.NewPatch。
     * 当前实现：返回 FrameUpdate.Empty，卡住。FAIL。
     */
    @Test
    fun observePresentedLayout_pendingBaseTextMatchesPresented_shouldAdvanceAndGeneratePatch() {
        val layouts = captureLayouts("A", "AB", "ABC")
        val coordinator = ComposeVisualFrameCoordinator(targetId = "issue694-c5692161955-p3b")

        // 1. onLayout("A") — 设基线 lastConsumed = "A"
        coordinator.onLayout(ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0))

        // 2. external intent 先到: "AB" -> "ABC"（pending != null）
        coordinator.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 300L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.GLYPH_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(1, 2)),
                newRanges = listOf(TextRange(1, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 1, oldEnd = 2, newStart = 1, newEnd = 3),
                expectedOldText = "AB",
                expectedNewText = "ABC",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // 3. 本地 layout 后到: observePresentedLayout("AB")
        //    pending.baseText="AB" == presented.text="AB"
        coordinator.observePresentedLayout(ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0))

        // 4. target layout "ABC" 到达: onLayout("ABC")
        val update = coordinator.onLayout(ComposeLayoutSnapshot(layouts[2], TextRange(3, 3), 0))

        assertTrue(
            "问题3：observePresentedLayout 在 pending.baseText == presented.text 时应推进 lastConsumed，" +
                "后续 target layout 到达后 patch 应生成（FrameUpdate.NewPatch）\n" +
                "当前实现：observePresentedLayout 只要 pending != null 就不推进，lastConsumed 保持 \"A\"，" +
                "onLayout(\"ABC\") 时 pending.baseText=\"AB\" != lastConsumed.text=\"A\"，" +
                "tryBuildPatch 返回 Empty，patch 卡死。实际 update=$update\n" +
                "Issue #694 评论 5692161955 问题3：observePresentedLayout 应补并发顺序 " +
                "pending?.baseText == presented.text 时也推进 lastConsumed",
            update is FrameUpdate.NewPatch,
        )
    }

    // ==================== 辅助方法 ====================

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = 14f.sp),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }

    /**
     * 通过反射访问 ComposeEditorVisualState 的 private pendingPatches 队列，
     * 查找包含指定 coreTransactionId 的 patch。
     *
     * Issue #723 评论 5750100004：pendingPatches 元素类型从 ComposeVisualPatch
     * 改为 PendingPatch（携带 sequence），需要通过反射读 .patch 字段。
     */
    private fun findPatchWithCoreTxnId(
        state: ComposeEditorVisualState,
        coreTxnId: Long,
    ): ComposeVisualPatch? {
        val field = ComposeEditorVisualState::class.java.getDeclaredField("pendingPatches")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val deque = field.get(state) as kotlin.collections.ArrayDeque<*>
        // 先在 pendingPatches 队列里找 — 元素是 PendingPatch，需要反射读 .patch
        for (item in deque) {
            val patchField = item?.javaClass?.getDeclaredField("patch")
            if (patchField != null) {
                patchField.isAccessible = true
                val patch = patchField.get(item) as ComposeVisualPatch
                if (patch.coreTransactionIds.contains(coreTxnId)) return patch
            }
        }
        // 再看 latestPatch（可能已被 drain 或就是最新）
        val latest = state.latestPatch.value
        if (latest != null && latest.coreTransactionIds.contains(coreTxnId)) return latest
        return null
    }
}
