package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #708 评论 5724568261 缺口1 的暴露测试 —
 *
 * 缺口1：[ComposeEditorVisualState] 的 `pendingLocalEditHandoff` 实际从来没有被建立。
 * - `pendingLocalEditHandoff` 初始化是 null
 * - 全文件只有 `pendingLocalEditHandoff = handoff.copy(patchId = localPatchId)` 和 `pendingLocalEditHandoff = null`
 * - 没有任何地方真正 `pendingLocalEditHandoff = ComposeLocalEditHandoff(...)`
 * - 所以 `bindLocalPatchHandoff()` 正常输入时实际上只会走 `Log.w(TAG, "local_patch_handoff_missing...")`
 * - `finishCompositionCommit()` 只做 `pendingPatches.addLast(localPatch); _frameRequestVersion.update; bindLocalPatchHandoff(...)`，
 *   **没有发布局部首帧 scene，也没有同步 drawSnapshot**
 *
 * #711 评论 5738906634：删除 ReflowMove 路线后，原缺口2/缺口3（自动换行必须创建 ReflowMove /
 * 部分重叠 ReflowMove 去重）已不再适用 — 幸存正文不再由 overlay 接管。
 * 本文件保留缺口1 测试，并新增 `reflow_surviving_text_not_in_hiddenRanges` 断言：
 * 输入触发软换行时，幸存正文不进入 hiddenRanges，直接由 BasicTextField 画最终位置。
 */
@Suppress("LongMethod", "MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue708Comment5724568261ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 缺口1：finishCompositionCommit 路径没有发布首帧 scene ====================

    /**
     * 缺口1：中文 composition 最终提交没有首帧交接。
     *
     * 旧 bug：`pendingLocalEditHandoff` 初始化为 null，全文件只有：
     * - `pendingLocalEditHandoff = handoff.copy(patchId = localPatchId)`（在 bindLocalPatchHandoff，但需要 handoff != null）
     * - `pendingLocalEditHandoff = null`（在 cancelCompositionLocalVisualState）
     * 没有任何地方真正 `pendingLocalEditHandoff = ComposeLocalEditHandoff(...)`。
     * 所以 bindLocalPatchHandoff() 正常输入时只会走 Log.w("local_patch_handoff_missing")。
     * `finishCompositionCommit()` 只做 `pendingPatches.addLast(localPatch); _frameRequestVersion.update; bindLocalPatchHandoff(...)`，
     * **没有发布局部首帧 scene，也没有同步 drawSnapshot**。
     *
     * 修复后（#708 评论 5724568261 缺口1）：`pendingLocalEditHandoff` / `ComposeLocalEditHandoff` 已删除，
     * `finishCompositionCommit` 统一调 `publishLocalHandoffScene` 发布首帧 scene。
     *
     * 暴露断言：通过反射直接调用 `finishCompositionCommit`（模拟 composition 最终提交）后，
     * `drawSnapshot().scene` 应包含 handoff 条目（hiddenRanges 非空 或 units 非空），
     * 证明首帧 scene 已发布。旧 bug 下 `finishCompositionCommit` 不发布 scene，scene 保持空。
     */
    @Test
    fun repro_comment5724568261_gap1_pendingLocalEditHandoffNeverEstablished() {
        // 用窄宽度 30px 让插入换行触发几何位移
        val layouts = captureLayoutsWithWidth(arrayOf("ab", "a\nb"), 30)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5724568261-gap1",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 确认 "ab" 一行，"a\nb" 跨两行（确保几何位移场景成立）
        assertTrue(
            "gap1: 'ab' 应一行，实际 lineCount=${layouts[0].lineCount}",
            layouts[0].lineCount == 1,
        )
        assertTrue(
            "gap1: 'a\\nb' 应跨两行，实际 lineCount=${layouts[1].lineCount}",
            layouts[1].lineCount >= 2,
        )

        // 初始 layout："ab"，caret 在 'a' 后（offset 1，准备插入换行）
        // 这会设置 lastPresentedLayout = layouts[0]
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 本地输入: "ab" -> "a\nb"（在 offset 1 插入换行符）
        state.recordLocalInput(
            oldText = "ab",
            newText = "a\nb",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )

        // 通过反射设置 compositionBaseLayout = lastPresentedLayout（模拟 composition 开始时保存 base），
        // 然后直接调用 finishCompositionCommit（模拟 composition 最终提交）。
        // finishCompositionCommit 是 private，需要反射调用。
        val stateClass = ComposeEditorVisualState::class.java
        val baseLayoutField = stateClass.getDeclaredField("compositionBaseLayout")
        baseLayoutField.isAccessible = true
        val lastPresentedField = stateClass.getDeclaredField("lastPresentedLayout")
        lastPresentedField.isAccessible = true
        val baseLayout = lastPresentedField.get(state) as ComposeLayoutSnapshot
        baseLayoutField.set(state, baseLayout)

        val finishMethod =
            stateClass.getDeclaredMethod(
                "finishCompositionCommit",
                String::class.java,
                ComposeLayoutSnapshot::class.java,
            )
        finishMethod.isAccessible = true
        val finalSnapshot = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)
        finishMethod.invoke(state, "a\nb", finalSnapshot)

        // 暴露断言：finishCompositionCommit 后首帧 scene 应包含 handoff 条目。
        // 评论描述：finishCompositionCommit 应发布局部首帧 scene 并同步 drawSnapshot，
        // 使 local timeline 立即接管，不出现"最终字先裸画一帧 -> 动画再接手"的窗口。
        // 旧 bug：finishCompositionCommit 只做 addLast + frameRequestVersion + bindLocalPatchHandoff，
        // 不发布首帧 scene，drawSnapshot().scene 保持空（hiddenRanges 空、units 空）。
        // 修复后：finishCompositionCommit 调 publishLocalHandoffScene，scene 包含 handoff 条目。
        val firstFrameScene = state.drawSnapshot().scene
        val hasHandoffEntries =
            firstFrameScene.hiddenRanges.isNotEmpty() || firstFrameScene.units.isNotEmpty()
        assertTrue(
            "gap1: finishCompositionCommit 后首帧 scene 应包含 handoff 条目" +
                "（hiddenRanges 非空 或 units 非空），" +
                "实际 hiddenRanges=${firstFrameScene.hiddenRanges}, units=${firstFrameScene.units}" +
                "（旧 bug：finishCompositionCommit 不发布首帧 scene，scene 保持空）",
            hasHandoffEntries,
        )
    }

    // ==================== #711：ReflowMove 路线删除后的新语义断言 ====================

    /**
     * #711 评论 5738906634：删除 ReflowMove 路线后 —
     * 输入触发软换行时，幸存正文不进入 hiddenRanges，直接由 BasicTextField 画最终位置。
     *
     * 场景："ab" -> "a\nb"（在 offset 1 插入换行符，'b' 被挤到第二行 = 软换行）。
     * - 文本 offset："a\nb" 中 'a'=[0,1)，'\n'=[1,2)，'b'=[2,3)。
     * - '\n' [1,2) 是新插入字符（changes.newRange=[1,2)），应进 hiddenRanges 由动画层吐字。
     * - 'b' [2,3) 是幸存正文（没被插入、没被删除，只是因为软换行从第一行移到第二行）。
     * - 旧 ReflowMove 路线会把幸存正文 'b' 的 newRange 加进 hiddenRanges，让 BasicTextField 裁掉，
     *   再由 overlay 从 oldBounds 平移到 newBounds，导致闪烁/软换行失配。
     * - 删除 ReflowMove 路线后，'b' 不应进 hiddenRanges，直接由 BasicTextField 画最终位置。
     *
     * 暴露断言：
     * 1. 新插入的换行符 '\n' [1,2) 应在 hiddenRanges 中（新插入字符由动画层吐字）；
     * 2. 幸存正文 'b' [2,3) 不应在 hiddenRanges 中（直接由 BasicTextField 画最终位置）。
     */
    @Test
    fun reflow_surviving_text_not_in_hiddenRanges() {
        // 用窄宽度 30px："ab" 一行；"a\nb" 跨两行（'b' 从第一行移到第二行，触发软换行）
        val layouts = captureLayoutsWithWidth(arrayOf("ab", "a\nb"), 30)
        val state =
            ComposeEditorVisualState(
                targetId = "test-711-5724568261-reflow",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 确认 "ab" 一行，"a\nb" 跨两行（确保软换行场景成立）
        assertTrue(
            "reflow: 'ab' 应一行，实际 lineCount=${layouts[0].lineCount}",
            layouts[0].lineCount == 1,
        )
        assertTrue(
            "reflow: 'a\\nb' 应跨两行，实际 lineCount=${layouts[1].lineCount}",
            layouts[1].lineCount >= 2,
        )

        // 初始 layout："ab"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 本地输入: "ab" -> "a\nb"（在 offset 1 插入换行符，'b' 被挤到第二行 = 软换行）
        state.recordLocalInput(
            oldText = "ab",
            newText = "a\nb",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        // onAuthoritativeLayout 配对生成 localPatch，建立首帧 scene
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        val firstFrameScene = state.drawSnapshot().scene

        // 换行符 '\n' 的 newRange 是 [1,2)（新插入字符）。
        // 新插入字符应进 hiddenRanges，由动画层吐字。
        val newlineNewRange = TextRange(1, 2)
        val newlineInHiddenRanges =
            firstFrameScene.hiddenRanges.any { it.start == newlineNewRange.start && it.end == newlineNewRange.end }
        assertTrue(
            "reflow: 新插入的换行符 '\\n' [1,2) 应在 hiddenRanges 中（新插入字符由动画层吐字），" +
                "实际 hiddenRanges=${firstFrameScene.hiddenRanges}",
            newlineInHiddenRanges,
        )

        // 'b' 的 newRange 是 [2,3)（在 "a\nb" 中 'b' 在 offset 2-3，因为 '\n' 占了 offset 1）。
        // 幸存正文不应进 hiddenRanges，直接由 BasicTextField 画最终位置。
        val bNewRange = TextRange(2, 3)
        val bInHiddenRanges =
            firstFrameScene.hiddenRanges.any { it.start == bNewRange.start && it.end == bNewRange.end }
        assertTrue(
            "reflow: 软换行后幸存正文 'b' [2,3) 不应进 hiddenRanges，" +
                "实际 hiddenRanges=${firstFrameScene.hiddenRanges}" +
                "（旧 ReflowMove 路线会把幸存正文 newRange 加进 hiddenRanges 让 BasicTextField 裁掉，" +
                "导致闪烁/软换行失配；删除 ReflowMove 路线后应直接由 BasicTextField 画最终位置）",
            !bInHiddenRanges,
        )
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L
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
            originCaretRect = Rect.Zero,
            targetCaretRect = Rect.Zero,
            durationMs = durationMs,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
        )

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> = captureLayoutsWithWidth(texts, 1000)

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
