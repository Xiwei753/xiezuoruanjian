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
 * #703 评论 5712256296 两个缺口的暴露测试 —
 *
 * 缺口1：[ComposeVisualPatchBatch.compose] 无条件重算 retainedMoves。
 * 同一 VSync 内来了两笔以上本地 patch 时，`drainPendingPatchesAtFrame` 走 `compose()`，
 * 这里仍无条件根据 `composedOffsetMap + oldLayout + newLayout` 调
 * `computeRetainedMovesFromComposedMap()`，凭几何重新"发明"出 retainedMoves。
 * 但 `buildLocalInputPatch()` 已经把本地输入的 `retainedMoves = emptyList()`。
 * 本地输入不应产生 retainedMoves（不把整行幸存文字交给 overlay 临时接管）。
 *
 * 缺口2：[ComposeVisualTimeline.redirectExistingMoveUnit] 没把 role 改成 RetainedMove。
 * `createMoveUnitForReflow()` 新建 unit 时写了 `role = VisualUnitRole.RetainedMove`，
 * 但 `redirectExistingMoveUnit()` 命中已存在 unit 时只 `copy(position = ...)`，没更新 role。
 * 于是原本的 `Inserted` unit 被判定为 retained move 后 role 仍是 `Inserted`，
 * `computeUnitClipFractions()` 仍会把它当吐字单元按 cursor 裁切。
 *
 * 当前代码下两个暴露断言都应 **FAIL**（证明 bug 存在）。
 */
@Suppress("LongMethod", "MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue703Comment5712256296ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 缺口1：batch 无条件重算 retainedMoves ====================

    /**
     * 缺口1：快速本地输入 batch 重新生成 retainedMoves。
     *
     * 走真实生产路径 recordLocalInput -> onAuthoritativeLayout -> drainPendingPatchesAtFrame。
     *
     * 场景（用窄宽度 30px 让插入换行触发几何位移）：
     * - T0 = "ab"（一行）
     * - 第一笔本地输入: "ab" -> "a\nb"（在 'a' 后插入换行符，'b' 从第一行移到第二行）
     * - 第二笔本地输入: "a\nb" -> "a\nbc"（在 'b' 后插入 'c'）
     *
     * 两笔 patch 在 pendingPatches 积累（不在中间 drain），一次 drainPendingPatchesAtFrame
     * 触发 ComposeVisualPatchBatch.compose(batch.size=2)。
     *
     * compose 中 composedOffsetMap 非空（'a' 和 'b' 存活），无条件调
     * computeRetainedMovesFromComposedMap(oldLayout="ab", newLayout="a\nbc", map)。
     * 'b' 在 "ab" 第一行，在 "a\nbc" 第二行，位置变了 → retainedMove 非空。
     *
     * 但本地输入的 retainedMoves 本应为空（buildLocalInputPatch 第 531 行已清空）。
     * batch.compose 不看各笔 patch 的 retainedMoves，凭几何重新算 — 这是缺口1。
     *
     * 暴露断言：`assertTrue(framePatch.retainedMoves.isEmpty())` — 当前代码下应 **FAIL**。
     */
    @Test
    fun repro_comment5712256296_gap1_batchRecomputesRetainedMoves() {
        // 用窄宽度 30px：'a'/'b' 约 7px，"ab" 约 14px 一行；"a\nb"/'a\nbc' 跨两行
        val layouts = captureLayoutsWithWidth(arrayOf("ab", "a\nb", "a\nbc"), 30)
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-5712256296-gap1",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 确认 "ab" 一行，"a\nb" / "a\nbc" 跨两行（确保几何位移场景成立）
        assertTrue(
            "gap1: 'ab' 应一行，实际 lineCount=${layouts[0].lineCount}",
            layouts[0].lineCount == 1,
        )
        assertTrue(
            "gap1: 'a\\nb' 应跨两行，实际 lineCount=${layouts[1].lineCount}",
            layouts[1].lineCount >= 2,
        )
        assertTrue(
            "gap1: 'a\\nbc' 应跨两行，实际 lineCount=${layouts[2].lineCount}",
            layouts[2].lineCount >= 2,
        )

        // 初始 layout："ab"，caret 在 'a' 后（offset 1，准备插入换行）
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 第一笔本地输入: "ab" -> "a\nb"（在 offset 1 插入换行符）
        // newRange=[1,2) 是换行符在新文本中的位置；oldRange=[1,1) 空（纯插入）
        state.recordLocalInput(
            oldText = "ab",
            newText = "a\nb",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        // onAuthoritativeLayout 配对生成 patch1 入队 pendingPatches（不 drain）
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // 第二笔本地输入: "a\nb" -> "a\nbc"（在 offset 3 插入 'c'）
        // newRange=[3,4) 是 'c' 在新文本中的位置；oldRange=[3,3) 空（纯插入）
        state.recordLocalInput(
            oldText = "a\nb",
            newText = "a\nbc",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(4, 4),
            changes = listOf(LocalInputChange(newRange = TextRange(3, 4), oldRange = TextRange(3, 3))),
        )
        // onAuthoritativeLayout 配对生成 patch2 入队 pendingPatches（不 drain）
        state.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)

        // 一次 drain：pendingPatches 有两笔 patch，batch.size=2，走 compose 合成
        val framePatches = state.drainPendingPatchesAtFrame(0L)
        assertTrue(
            "gap1: drain 应合成一笔 framePatch，实际 size=${framePatches.size}",
            framePatches.size == 1,
        )
        val framePatch = framePatches.first()

        // 暴露断言：本地输入 batch 合成后 retainedMoves 应为空。
        // buildLocalInputPatch 已把每笔本地 patch 的 retainedMoves 清空（不接管整行幸存文字），
        // batch.compose 应只合成各 stage patch 已携带的 retainedMoves（空），不应凭几何重算。
        // 当前 bug：compose 无条件调 computeRetainedMovesFromComposedMap 凭几何重新发明 retainedMoves，
        // 'b' 从 "ab" 第一行移到 "a\nbc" 第二行，位置变了 → retainedMove 非空。
        assertTrue(
            "gap1: 本地输入 batch 合成后 retainedMoves 应为空（本地输入不接管整行幸存文字），" +
                "实际=${framePatch.retainedMoves.map { "${it.oldRange}->${it.newRange}" }}" +
                "（当前 bug：compose 无条件重算 retainedMoves，凭几何发明出非空 retainedMoves）",
            framePatch.retainedMoves.isEmpty(),
        )

        // 同时检查 scene 不因本地 batch 新建 RetainedMove role 的 unit。
        // 本地输入不应把幸存文字交给 overlay 接管。
        val scene = state.sampleVisualScene(0L)
        val retainedMoveUnitCount =
            scene.units.count { it.role == VisualUnitRole.RetainedMove && it.targetRange != null }
        assertTrue(
            "gap1: 本地输入 batch 不应新建 RetainedMove role 的存活 unit，" +
                "实际 retainedMoveUnitCount=$retainedMoveUnitCount",
            retainedMoveUnitCount == 0,
        )
    }

    // ==================== 缺口2：redirect 不改 role ====================

    /**
     * 缺口2：existing-unit redirect 不改 role。
     *
     * 用 [ComposeVisualTimeline] 直接操作。
     *
     * 场景（用窄宽度 30px 让换行触发几何位移）：
     * - 第一笔：插入 patch，oldLayout="" newLayout="ab"，insertedUnits=[TextRange(1,2)]（'b'）。
     *   创建 Inserted role 的 unit，targetRange=[1,2)，position='b' 在 "ab" 第一行。
     * - 在 20ms 时 apply 第二笔（unit 还 active，alpha=0.02 未收口）。
     * - 第二笔：带非空 retainedMoves 的 patch，oldLayout="ab" newLayout="\nb"，
     *   retainedMoves=[RetainedMove([1,2), [1,2))]。
     *   newRange=[1,2) 命中第一笔 unit 的 targetRange=[1,2)。
     *   newPosition='b' 在 "\nb" 第二行 != oldPosition='b' 在 "ab" 第一行 → 触发 redirect。
     *
     * redirectExistingMoveUnit 只 copy(position=...)，没更新 role。
     * 于是原本 Inserted 的 unit 被判定为 retained move 后 role 仍是 Inserted。
     *
     * 暴露断言：`assertEquals(VisualUnitRole.RetainedMove, unit.role)` — 当前代码下应 **FAIL**。
     * 同时检查 coordinated spatial clip 下该 unit 的 clipFraction 始终为 1
     *（RetainedMove 不应被 cursor 裁切）。
     */
    @Test
    fun repro_comment5712256296_gap2_redirectDoesNotUpdateRole() {
        // 用窄宽度 30px："ab" 一行（'b' 在 [1,2) 第一行）；"\nb" 跨两行（'b' 在 [1,2) 第二行）
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab", "\nb"), 30)
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)
        val newlineBLayout = ComposeLayoutSnapshot(layouts[2], TextRange(2, 2), 0)

        // 确认 "ab" 一行，"\nb" 跨两行（确保几何位移场景成立）
        assertTrue(
            "gap2: 'ab' 应一行，实际 lineCount=${layouts[1].lineCount}",
            layouts[1].lineCount == 1,
        )
        assertTrue(
            "gap2: '\\nb' 应跨两行，实际 lineCount=${layouts[2].lineCount}",
            layouts[2].lineCount >= 2,
        )

        // 确认 'b' [1,2) 在 "ab" 和 "\nb" 中位置不同（确保 redirect 触发）
        val bPosInAb = layouts[1].getPathForRange(1, 2).getBounds()
        val bPosInNewlineB = layouts[2].getPathForRange(1, 2).getBounds()
        assertTrue(
            "gap2: 'b' [1,2) 在 'ab' 和 '\\nb' 中位置应不同（确保 redirect 触发），" +
                "ab bounds=$bPosInAb, newlineB bounds=$bPosInNewlineB",
            kotlin.math.abs(bPosInAb.top - bPosInNewlineB.top) > 1f ||
                kotlin.math.abs(bPosInAb.left - bPosInNewlineB.left) > 1f,
        )

        val timeline = ComposeVisualTimeline()
        val motionPolicy =
            EditorMotionPolicy(
                textDurationMillis = 1000L,
                coordinated = true,
            )

        // 第一笔：插入 'b'，创建 Inserted role 的 unit，targetRange=[1,2)
        // oldLayout="" newLayout="ab"，insertedUnits=[TextRange(1,2)]（只 'b'，简化）
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
            "gap2: 第一笔 applyPatch 后应存在 targetRange=[1,2) 的 unit",
            unitAfter1,
        )
        assertEquals(
            "gap2: 第一笔创建的 unit role 应为 Inserted",
            VisualUnitRole.Inserted,
            unitAfter1!!.role,
        )

        // 在 20ms 时 apply 第二笔（unit 还 active，alpha=0.02 未收口）
        // 第二笔：retainedMoves 命中 unit（newRange=[1,2)），newLayout="\nb" 让 [1,2) 位置变化
        val patch2 =
            makePatch(
                id = 2L,
                oldLayout = abLayout,
                newLayout = newlineBLayout,
                retainedMoves =
                    listOf(
                        RetainedMove(oldRange = TextRange(1, 2), newRange = TextRange(1, 2)),
                    ),
                durationMs = 1000L,
                motionPolicy = motionPolicy,
            )
        timeline.applyPatch(patch = patch2, frameTimeNanos = 20L * NANOS_PER_MS)

        // 采样检查 surviving unit 的 role
        val scene = timeline.sample(20L * NANOS_PER_MS)

        // 找 targetRange=[1,2) 的 surviving unit
        val unit = scene.units.firstOrNull { it.targetRange == TextRange(1, 2) }
        assertNotNull(
            "gap2: 第二笔 applyPatch 后 targetRange=[1,2) 的 surviving unit 应存在" +
                "（retainedMove 命中已存在 unit，redirect 重定向 position）",
            unit,
        )

        // 暴露断言：被判定为 retained move 的 unit role 应是 RetainedMove。
        // createMoveUnitForReflow 新建 unit 时写了 role=RetainedMove，
        // 但 redirectExistingMoveUnit 命中已存在 unit 时只 copy(position=...)，没更新 role。
        // 当前 bug：unit.role 仍是 Inserted（原插入 unit 的 role 被保留）。
        assertEquals(
            "gap2: redirect 后 unit.role 应为 RetainedMove（被判定为 retained move 的幸存回流文字），" +
                "实际=${unit!!.role}" +
                "（当前 bug：redirectExistingMoveUnit 只 copy(position) 没改 role，仍是 Inserted）",
            VisualUnitRole.RetainedMove,
            unit.role,
        )

        // 同时检查 coordinated spatial clip 下该 unit 的 clipFraction 始终为 1
        // （RetainedMove 不应被 cursor 裁切）。
        // 当前 bug：role=Inserted，computeUnitClipFractions 把它当吐字单元按 cursor 裁切，
        // clipFraction 可能 != 1。
        val clipFraction = scene.unitClipFractions[unit.key] ?: 1f
        assertTrue(
            "gap2: coordinated spatial clip 下 retained move unit 的 clipFraction 应始终为 1" +
                "（RetainedMove 不被 cursor 裁切），实际 clipFraction=$clipFraction" +
                "（当前 bug：role=Inserted 被当吐字单元裁切）",
            clipFraction >= 0.99f,
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
            originCaretRect = Rect.Zero,
            targetCaretRect = Rect.Zero,
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
