package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
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
 * #708 评论 5724568261 三个缺口的暴露测试 —
 *
 * 缺口1：[ComposeEditorVisualState] 的 `pendingLocalEditHandoff` 实际从来没有被建立。
 * - `pendingLocalEditHandoff` 初始化是 null
 * - 全文件只有 `pendingLocalEditHandoff = handoff.copy(patchId = localPatchId)` 和 `pendingLocalEditHandoff = null`
 * - 没有任何地方真正 `pendingLocalEditHandoff = ComposeLocalEditHandoff(...)`
 * - 所以 `bindLocalPatchHandoff()` 正常输入时实际上只会走 `Log.w(TAG, "local_patch_handoff_missing...")`
 * - `finishCompositionCommit()` 只做 `pendingPatches.addLast(localPatch); _patchVersion.update; bindLocalPatchHandoff(...)`，
 *   **没有发布局部首帧 scene，也没有同步 drawSnapshot**
 *
 * 缺口2：自动换行的 ReflowMove 没进入首帧 handoff。
 * - timeline 里会把 ReflowMove 的 newRange 放进 hiddenRanges，再从 oldBounds -> newBounds 平移
 * - 但 `onAuthoritativeLayout()` 建首帧 scene 时完全没处理 `localPatch.reflowMoves`
 * - 当首帧只做 insert->隐藏新字、delete->画旧字ghost、cursor->放旧caret 时，
 *   BasicTextField 已经使用 newLayout，所以自动换行后幸存文字会先直接出现在新行
 *
 * 缺口3：快速连续输入时，同一段字可能同时存在"旧 active unit + 新 ReflowMove"，会重复绘制。
 * - `applyReflowMoves()` 的去重条件只有
 *   `if (surviving.any { it.targetRange == newRange && it.role == VisualUnitRole.ReflowMove }) continue`
 * - 这不够：surviving 里可能已有一个 role=Inserted 的 unit，applyReflowMoves() 因为它不是 ReflowMove 不会跳过，
 *   又创建第二个 role=ReflowMove
 * - 同一段文字被两个 overlay unit 同时画，产生重影、闪字
 *
 * 当前代码下三个暴露断言都应 **FAIL**（证明 bug 存在）。
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
     * `finishCompositionCommit()` 只做 `pendingPatches.addLast(localPatch); _patchVersion.update; bindLocalPatchHandoff(...)`，
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
        // 旧 bug：finishCompositionCommit 只做 addLast + patchVersion + bindLocalPatchHandoff，
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

    // ==================== 缺口2：ReflowMove 没进入首帧 handoff ====================

    /**
     * 缺口2：onAuthoritativeLayout 建首帧 scene 时没处理 localPatch.reflowMoves。
     *
     * buildLocalInputPatch 会通过 ComposeReflowPlanner.plan 产生 reflowMoves（行 590-595），
     * 但 onAuthoritativeLayout 建首帧 scene 时（行 775-828）只处理：
     * - insertedUnits -> 加进 hiddenRanges
     * - deletedUnits -> 建 ghost
     * - cursor -> 放旧 caret
     * 完全没有处理 localPatch.reflowMoves。
     *
     * 暴露断言：onAuthoritativeLayout 触发自动换行（产生非空 reflowMoves）后，
     * 首帧 scene（drawSnapshotState.scene）应包含 ReflowMove 相关条目
     *（ReflowMove role 的 unit 或 reflow 相关的 hiddenRanges）。
     * 当前 bug：首帧 scene 完全没有处理 reflowMoves，不包含任何 ReflowMove 相关条目。
     */
    @Test
    fun repro_comment5724568261_gap2_reflowMovesNotInFirstFrameHandoff() {
        // 用窄宽度 30px："ab" 一行；"a\nb" 跨两行（'b' 从第一行移到第二行，触发 reflow）
        val layouts = captureLayoutsWithWidth(arrayOf("ab", "a\nb"), 30)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5724568261-gap2",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 确认 "ab" 一行，"a\nb" 跨两行（确保 reflow 场景成立）
        assertTrue(
            "gap2: 'ab' 应一行，实际 lineCount=${layouts[0].lineCount}",
            layouts[0].lineCount == 1,
        )
        assertTrue(
            "gap2: 'a\\nb' 应跨两行，实际 lineCount=${layouts[1].lineCount}",
            layouts[1].lineCount >= 2,
        )

        // 确认 'b' [1,2) 在 "ab" 和 "a\nb" 中位置不同（确保 reflow 触发）
        val bPosInAb = layouts[0].getPathForRange(1, 2).getBounds()
        val bPosInNewlineB = layouts[1].getPathForRange(1, 2).getBounds()
        assertTrue(
            "gap2: 'b' [1,2) 在 'ab' 和 'a\\nb' 中位置应不同（确保 reflow 触发），" +
                "ab bounds=$bPosInAb, newlineB bounds=$bPosInNewlineB",
            kotlin.math.abs(bPosInAb.top - bPosInNewlineB.top) > 1f ||
                kotlin.math.abs(bPosInAb.left - bPosInNewlineB.left) > 1f,
        )

        // 初始 layout："ab"
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 本地输入: "ab" -> "a\nb"（在 offset 1 插入换行符，'b' 被挤到第二行 = reflow）
        state.recordLocalInput(
            oldText = "ab",
            newText = "a\nb",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        // onAuthoritativeLayout 配对生成 localPatch（含非空 reflowMoves），建立首帧 scene
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // drawSnapshot() 是 internal fun，返回 drawSnapshotState（onAuthoritativeLayout 在行 830-835 同步了 drawSnapshotState）
        val firstFrameScene = state.drawSnapshot().scene

        // 反射读取 pendingPatches 确认 localPatch 已入队且 reflowMoves 非空
        val pendingPatchesField = ComposeEditorVisualState::class.java.getDeclaredField("pendingPatches")
        pendingPatchesField.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val pendingPatches = pendingPatchesField.get(state) as ArrayDeque<ComposeVisualPatch>
        assertTrue(
            "gap2: onAuthoritativeLayout 后应有一笔 localPatch 入队，实际 size=${pendingPatches.size}",
            pendingPatches.size == 1,
        )
        val localPatch = pendingPatches.first()
        assertTrue(
            "gap2: localPatch.reflowMoves 应非空（'b' 从第一行 reflow 到第二行），" +
                "实际 reflowMoves=${localPatch.reflowMoves}" +
                "（如果为空说明 ComposeReflowPlanner.plan 未生成 reflow，测试场景需调整）",
            localPatch.reflowMoves.isNotEmpty(),
        )

        // 暴露断言：首帧 scene 应包含 ReflowMove 相关条目。
        // 评论描述：onAuthoritativeLayout 建首帧 scene 时应处理 localPatch.reflowMoves，
        // 把 ReflowMove 的 newRange 放进 hiddenRanges 或建立 ReflowMove role 的 unit，
        // 让 BasicTextField 的新位置暂时不重复画，由 overlay 从 oldBounds -> newBounds 平移。
        // 当前 bug：首帧 scene 完全没有处理 reflowMoves。
        val reflowMoveUnitCount =
            firstFrameScene.units.count { it.role == VisualUnitRole.ReflowMove }
        val hasReflowInHiddenRanges =
            localPatch.reflowMoves.any { move ->
                firstFrameScene.hiddenRanges.any { it.start == move.newRange.start && it.end == move.newRange.end }
            }
        assertTrue(
            "gap2: 首帧 scene 应包含 ReflowMove 相关条目（ReflowMove role 的 unit 或 reflow 的 hiddenRanges），" +
                "实际 reflowMoveUnitCount=$reflowMoveUnitCount, hasReflowInHiddenRanges=$hasReflowInHiddenRanges" +
                "（当前 bug：onAuthoritativeLayout 建首帧 scene 时只处理 insertedUnits/deletedUnits/cursor，" +
                "完全没处理 localPatch.reflowMoves，自动换行后幸存文字会先直接出现在新行）",
            reflowMoveUnitCount > 0 || hasReflowInHiddenRanges,
        )
    }

    // ==================== 缺口3：applyReflowMoves 去重不足 ====================

    /**
     * 缺口3：applyReflowMoves 去重条件过窄。
     *
     * 用 [ComposeVisualTimeline] 直接操作。
     *
     * 场景（用窄宽度 30px 让换行触发几何位移）：
     * - 第一笔：插入 patch，oldLayout="" newLayout="ab"，insertedUnits=[TextRange(1,2)]（'b'）。
     *   创建 Inserted role 的 unit，targetRange=[1,2)，position='b' 在 "ab" 第一行。
     * - 在 20ms 时 apply 第二笔（unit 还 active，alpha=0.02 未收口）。
     * - 第二笔：带非空 reflowMoves 的 patch，oldLayout="ab" newLayout="\nb"，
     *   reflowMoves=[ComposeReflowMove(oldRange=[1,2), newRange=[1,2), oldBounds, newBounds)]。
     *   newRange=[1,2) 命中第一笔 unit 的 targetRange=[1,2)。
     *
     * applyReflowMoves 的去重条件（行 771）：
     * `if (surviving.any { it.targetRange == newRange && it.role == VisualUnitRole.ReflowMove }) continue`
     * 只查 role==ReflowMove，不查 Inserted。
     * surviving 里已有 role=Inserted 的 unit（targetRange=[1,2)），applyReflowMoves 不跳过，
     * 又创建第二个 role=ReflowMove（targetRange=[1,2)）。
     *
     * 暴露断言：`assertEquals(1, sameRangeUnitCount)` — 当前代码下应 **FAIL**（有 2 个 unit）。
     */
    @Test
    fun repro_comment5724568261_gap3_applyReflowMovesDedupInsufficient() {
        // 用窄宽度 30px："ab" 一行（'b' 在 [1,2) 第一行）；"\nb" 跨两行（'b' 在 [1,2) 第二行）
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab", "\nb"), 30)
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)
        val newlineBLayout = ComposeLayoutSnapshot(layouts[2], TextRange(2, 2), 0)

        // 确认 "ab" 一行，"\nb" 跨两行（确保几何位移场景成立）
        assertTrue(
            "gap3: 'ab' 应一行，实际 lineCount=${layouts[1].lineCount}",
            layouts[1].lineCount == 1,
        )
        assertTrue(
            "gap3: '\\nb' 应跨两行，实际 lineCount=${layouts[2].lineCount}",
            layouts[2].lineCount >= 2,
        )

        // 确认 'b' [1,2) 在 "ab" 和 "\nb" 中位置不同（确保 reflow 触发）
        val bPosInAb = layouts[1].getPathForRange(1, 2).getBounds()
        val bPosInNewlineB = layouts[2].getPathForRange(1, 2).getBounds()
        assertTrue(
            "gap3: 'b' [1,2) 在 'ab' 和 '\\nb' 中位置应不同（确保 reflow 触发），" +
                "ab bounds=$bPosInAb, newlineB bounds=$bPosInNewlineB",
            kotlin.math.abs(bPosInAb.top - bPosInNewlineB.top) > 1f ||
                kotlin.math.abs(bPosInAb.left - bPosInNewlineB.left) > 1f,
        )

        val timeline = ComposeVisualTimeline()
        val motionPolicy =
            EditorMotionPolicy(
                textDurationMillis = 1000L,
                cursorEnabled = true,
                coordinated = true,
            )

        // 第一笔：插入 'b'，创建 Inserted role 的 unit，targetRange=[1,2)
        val patch1 =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abLayout,
                insertedUnits = listOf(TextRange(1, 2)),
                durationMs = 1000L,
                motionPolicy = motionPolicy,
            )
        timeline.applyPatch(patch = patch1, frameTimeNanos = 0L)

        // 确认第一笔创建了 Inserted role 的 unit
        val sceneAfter1 = timeline.sample(0L)
        val unitAfter1 = sceneAfter1.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        assertNotNull(
            "gap3: 第一笔 applyPatch 后应存在 targetRange=[1,2) 的 unit",
            unitAfter1,
        )
        assertEquals(
            "gap3: 第一笔创建的 unit role 应为 Inserted",
            VisualUnitRole.Inserted,
            unitAfter1!!.role,
        )

        // 在 20ms 时 apply 第二笔（unit 还 active，alpha=0.02 未收口）
        // 第二笔：reflowMoves 命中 unit（newRange=[1,2)），newLayout="\nb" 让 [1,2) 位置变化
        val reflowMove =
            ComposeReflowMove(
                oldRange = TextRange(1, 2),
                newRange = TextRange(1, 2),
                oldBounds = bPosInAb,
                newBounds = bPosInNewlineB,
            )
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abLayout,
                newLayout = newlineBLayout,
                reflowMoves = listOf(reflowMove),
                durationMs = 1000L,
                motionPolicy = motionPolicy,
            )
        timeline.applyPatch(patch = patch2, frameTimeNanos = 20L * NANOS_PER_MS)

        // 采样检查 surviving units
        val scene = timeline.sample(20L * NANOS_PER_MS)

        // 找所有 targetRange=[1,2) 的 surviving unit
        val sameRangeUnits = scene.units.filter { it.targetRange == TextRange(1, 2) }

        // 暴露断言：同一段文字 [1,2) 应只有一个 overlay unit。
        // applyReflowMoves 的去重条件应覆盖 surviving 中所有可能已存在的同 targetRange unit
        // （包括 Inserted/RetainedMove role），避免同一段文字被两个 overlay unit 同时画。
        // 当前 bug：去重条件只查 role==ReflowMove 不查 Inserted，
        // surviving 里已有 role=Inserted 的 unit 时又创建第二个 role=ReflowMove，
        // 同一段文字被两个 overlay unit 同时画产生重影、闪字。
        val sameRangeUnitCount = sameRangeUnits.size
        assertEquals(
            "gap3: 同一段文字 [1,2) 应只有一个 overlay unit（去重应覆盖所有 role），" +
                "实际 sameRangeUnitCount=$sameRangeUnitCount, roles=${sameRangeUnits.map { it.role }}" +
                "（当前 bug：applyReflowMoves 去重条件只查 role==ReflowMove 不查 Inserted，" +
                "surviving 里已有 Inserted unit 时又创建 ReflowMove unit，产生重影）",
            1,
            sameRangeUnitCount,
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
        reflowMoves: List<ComposeReflowMove> = emptyList(),
        cursorMotionPath: CursorMotionPath? = null,
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
            reflowMoves = reflowMoves,
            cursorMotionPath = cursorMotionPath,
            durationMs = durationMs,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            motionPolicy = motionPolicy,
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
