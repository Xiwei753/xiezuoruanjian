package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.cursorRect
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
 * #708 评论 5725146968 两个修复的验证测试 —
 *
 * 修复1：纯输入时首帧光标回抽
 * - 旧 bug：`publishLocalHandoffScene()` 只在删除时才把旧 caret 放进 scene.cursorRect，
 *   普通插入时 scene.cursorRect == null，draw 层直接算出新光标位置，
 *   下一帧 timeline 又用旧位置做起点，导致光标回抽。
 * - 修复后：只要 `cursorEnabled && cursorMotionPath != null`，首帧 scene 就用
 *   `originCursorRect` 作为 cursorRect。
 *
 * 修复2：Reflow 部分重叠差集
 * - 旧 bug：`subtractOverlayOwnedRanges()` 对部分重叠直接丢弃整个 reflow，
 *   而不是做差集。
 * - 修复后：使用 `ComposeOverlayOwnership.subtractOwnedRanges()` 做真正的差集，
 *   部分重叠时返回剩余 slice。
 */
@Suppress("LongMethod", "MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue708Comment5725146968ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 修复1：纯输入时首帧光标回抽 ====================

    /**
     * 修复1：纯插入时首帧 scene 的 cursorRect 应使用旧光标位置（originCursorRect），
     * 而不是 null（旧 bug）或新光标位置。
     *
     * #708 评论 5725146968：
     * 旧 bug：`publishLocalHandoffScene()` 只在删除时才把旧 caret 放进 scene.cursorRect，
     * 普通插入时 scene.cursorRect == null。draw 层在 cursorRect == null 时直接算出新光标位置，
     * 下一帧 timeline 又用旧位置做起点，导致光标从新位置"回抽"到旧位置再动画到新位置。
     *
     * 修复后：只要 `cursorEnabled && cursorMotionPath != null`，首帧 scene 就用
     * `originCursorRect` 作为 cursorRect，让 draw 层首帧画旧光标位置，
     * 与 timeline 的 cursorMotionPath.fromRect 保持一致，不出现回抽。
     *
     * 测试场景：
     * 1. 初始 layout "ab"，caret 在 offset 1
     * 2. 本地输入 "ab" -> "axb"（在 offset 1 插入 'x'）
     * 3. 调用 onAuthoritativeLayout 触发 publishLocalHandoffScene
     * 4. 断言：首帧 scene 的 cursorRect 不为 null，且等于旧光标位置（offset 1 在旧 layout "ab" 中的 cursor rect），
     *    而不是新光标位置（offset 2 在新 layout "axb" 中的 cursor rect）
     */
    @Test
    fun fix1_pureInsertFirstFrameCursorUsesOriginCursorRect() {
        val layouts = captureLayoutsWithWidth(arrayOf("ab", "axb"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5725146968-fix1",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 计算旧光标位置（offset 1 在 "ab" 中的 cursor rect）
        val oldLayoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val oldCursorRect = oldLayoutSnapshot.cursorRect(1)

        // 计算新光标位置（offset 2 在 "axb" 中的 cursor rect）
        val newLayoutSnapshot = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)
        val newCursorRect = newLayoutSnapshot.cursorRect(2)

        // 确认旧光标和新光标位置不同（否则测试无意义）
        assertTrue(
            "fix1: 旧光标位置(offset=1 in 'ab')和新光标位置(offset=2 in 'axb')应不同，" +
                "oldCursorRect=$oldCursorRect, newCursorRect=$newCursorRect",
            oldCursorRect != newCursorRect,
        )

        // 初始 layout："ab"，caret 在 offset 1
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 本地输入: "ab" -> "axb"（在 offset 1 插入 'x'）
        state.recordLocalInput(
            oldText = "ab",
            newText = "axb",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )

        // onAuthoritativeLayout 配对生成 localPatch，建立首帧 scene（触发 publishLocalHandoffScene）
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // 断言：首帧 scene 的 cursorRect 不为 null
        val firstFrameScene = state.drawSnapshot().scene
        assertNotNull(
            "fix1: 纯插入时首帧 scene 的 cursorRect 应不为 null" +
                "（旧 bug：publishLocalHandoffScene 只在删除时才设 cursorRect，插入时为 null，" +
                "导致 draw 层直接算新光标位置，下一帧 timeline 用旧位置做起点产生回抽）",
            firstFrameScene.cursorRect,
        )

        // 断言：首帧 scene 的 cursorRect 等于旧光标位置（originCursorRect），
        // 而不是新光标位置
        assertEquals(
            "fix1: 首帧 scene 的 cursorRect 应等于旧光标位置（offset=1 in 'ab'），" +
                "而不是新光标位置（offset=2 in 'axb'），" +
                "actual=${firstFrameScene.cursorRect}, expected(old)=$oldCursorRect, newCursorRect=$newCursorRect" +
                "（旧 bug：cursorRect==null 导致 draw 层首帧画新光标位置，下一帧 timeline 从旧位置开始，光标回抽）",
            oldCursorRect,
            firstFrameScene.cursorRect,
        )
    }

    /**
     * 修复1补充：删除时首帧 scene 的 cursorRect 仍应使用旧光标位置。
     *
     * 这是修复前的已有行为（删除路径一直正确），确保修复没有破坏删除路径。
     */
    @Test
    fun fix1_deleteFirstFrameCursorStillUsesOriginCursorRect() {
        val layouts = captureLayoutsWithWidth(arrayOf("abc", "ac"), 1000)
        val state =
            ComposeEditorVisualState(
                targetId = "test-708-5725146968-fix1-delete",
                classifier = FakeLocalVisualPlanClassifier,
            )

        val oldLayoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(2, 2), 0)
        val oldCursorRect = oldLayoutSnapshot.cursorRect(2)

        // 初始 layout："abc"，caret 在 offset 2
        state.onAuthoritativeLayout(layouts[0], TextRange(2, 2), 0)

        // 本地输入: "abc" -> "ac"（删除 offset 1 的 'b'）
        state.recordLocalInput(
            oldText = "abc",
            newText = "ac",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 1), oldRange = TextRange(1, 2))),
        )

        // onAuthoritativeLayout 配对生成 localPatch，建立首帧 scene
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        val firstFrameScene = state.drawSnapshot().scene
        assertNotNull(
            "fix1-delete: 删除时首帧 scene 的 cursorRect 应不为 null",
            firstFrameScene.cursorRect,
        )
        assertEquals(
            "fix1-delete: 删除时首帧 scene 的 cursorRect 应等于旧光标位置（offset=2 in 'abc'）",
            oldCursorRect,
            firstFrameScene.cursorRect,
        )
    }

    // ==================== 修复2：Reflow 部分重叠差集（timeline 集成测试） ====================

    /**
     * 修复2：applyReflowMoves 使用 subtractOverlayOwnedRanges 做差集，
     * 部分重叠时只创建剩余 slice 的 ReflowMove unit，不会同一段文字被两个 overlay unit 同时画。
     *
     * #708 评论 5725146968：
     * 旧 bug：`subtractOverlayOwnedRanges()` 对部分重叠直接丢弃整个 reflow（返回 emptyList），
     * 而不是做差集。这导致部分重叠的 reflow 被完全丢弃，文字位置变化没有动画。
     * 同时 `applyReflowMoves` 的去重条件只查 `role==ReflowMove`，不查 Inserted/RetainedMove，
     * 可能创建重复 unit。
     *
     * 修复后：使用 `ComposeOverlayOwnership.subtractOwnedRanges()` 做真正的差集，
     * 部分重叠时返回剩余 slice。同时去重条件覆盖所有 role。
     *
     * 测试场景（用 ComposeVisualTimeline 直接操作）：
     * 1. 第一笔：插入 patch，oldLayout="" newLayout="ab"，insertedUnits=[TextRange(1,2)]（'b'）。
     *    创建 Inserted role 的 unit，targetRange=[1,2)。
     * 2. 在 20ms 时 apply 第二笔（unit 还 active）。
     * 3. 第二笔：带非空 reflowMoves 的 patch，oldLayout="ab" newLayout="\nb"（窄宽度让 'b' 从第一行移到第二行），
     *    reflowMoves 包含 'b' 的 [1,2) -> [1,2) 位移。
     * 4. 断言：applyReflowMoves 后，surviving 中 targetRange=[1,2) 的 unit 只有 1 个（不是 2 个），
     *    不会同一段文字被两个 overlay unit 同时画。
     */
    @Test
    fun fix2_applyReflowMovesPartialOverlapDedupBySubtraction() {
        // 用窄宽度 30px："ab" 一行（'b' 在 [1,2) 第一行）；"\nb" 跨两行（'b' 在 [1,2) 第二行）
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab", "\nb"), 30)
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)
        val newlineBLayout = ComposeLayoutSnapshot(layouts[2], TextRange(2, 2), 0)

        // 确认 "ab" 一行，"\nb" 跨两行（确保几何位移场景成立）
        assertTrue(
            "fix2: 'ab' 应一行，实际 lineCount=${layouts[1].lineCount}",
            layouts[1].lineCount == 1,
        )
        assertTrue(
            "fix2: '\\nb' 应跨两行，实际 lineCount=${layouts[2].lineCount}",
            layouts[2].lineCount >= 2,
        )

        // 确认 'b' [1,2) 在 "ab" 和 "\nb" 中位置不同（确保 reflow 触发）
        val bPosInAb = layouts[1].getPathForRange(1, 2).getBounds()
        val bPosInNewlineB = layouts[2].getPathForRange(1, 2).getBounds()
        assertTrue(
            "fix2: 'b' [1,2) 在 'ab' 和 '\\nb' 中位置应不同（确保 reflow 触发），" +
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
            "fix2: 第一笔 applyPatch 后应存在 targetRange=[1,2) 的 unit",
            unitAfter1,
        )
        assertEquals(
            "fix2: 第一笔创建的 unit role 应为 Inserted",
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

        // 断言：同一段文字 [1,2) 应只有一个 overlay unit。
        // 修复后：applyReflowMoves 用 subtractOverlayOwnedRanges 做差集，
        // surviving 里已有 role=Inserted 的 unit（targetRange=[1,2)）时，
        // subtractOwnedRanges 从 move.newRange=[1,2) 中减去 ownedRanges=[1,2) 得到空列表，
        // 不创建第二个 ReflowMove unit。
        // 旧 bug：去重条件只查 role==ReflowMove 不查 Inserted，又创建第二个 role=ReflowMove，
        // 同一段文字被两个 overlay unit 同时画产生重影。
        val sameRangeUnitCount = sameRangeUnits.size
        assertEquals(
            "fix2: 同一段文字 [1,2) 应只有一个 overlay unit（差集去重应覆盖所有 role），" +
                "实际 sameRangeUnitCount=$sameRangeUnitCount, roles=${sameRangeUnits.map { it.role }}" +
                "（旧 bug：applyReflowMoves 去重条件只查 role==ReflowMove 不查 Inserted，" +
                "surviving 里已有 Inserted unit 时又创建 ReflowMove unit，产生重影）",
            1,
            sameRangeUnitCount,
        )
    }

    // ==================== 修复2：Reflow 部分重叠差集（直接单元测试） ====================

    /**
     * 修复2直接测试：ComposeOverlayOwnership.subtractOwnedRanges 部分重叠时返回剩余 slice。
     *
     * #708 评论 5725146968：
     * 旧 bug：`subtractOverlayOwnedRanges()` 对部分重叠直接丢弃整个 reflow（返回 emptyList），
     * 而不是做差集。例如 ReflowMove newRange=[2,6) 已有 Inserted targetRange=[5,6)，
     * 旧代码返回空列表，[2,5) 的 reflow 被丢弃。
     *
     * 修复后：使用 `ComposeOverlayOwnership.subtractOwnedRanges()` 做真正的差集，
     * 部分重叠时返回剩余 slice：[2,5)。
     *
     * 用带换行符的文本确保 oldLayout 和 newLayout 中同一段文字的位置真的不同：
     * - oldLayout: "abcde"（一行），[1,5) 在第一行
     * - newLayout: "a\nbcde"（两行），[2,6) 在第二行
     */
    @Test
    fun fix2_subtractOwnedRangesPartialOverlapReturnsRemainingSlice() {
        val layouts =
            captureLayoutsWithMultipleWidths(
                FIX2_OLD_TEXT to 1000,
                FIX2_NEW_TEXT to 1000,
            )

        val oldLayoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val newLayoutSnapshot = ComposeLayoutSnapshot(layouts[1], TextRange(0, 0), 0)

        // 取 [1,5) 在 oldLayout 和 [2,6) 在 newLayout 的 bounds
        val oldBounds = layouts[0].getPathForRange(1, 5).getBounds()
        val newBounds = layouts[1].getPathForRange(2, 6).getBounds()

        // 确认 oldBounds != newBounds（否则 subtractOwnedRanges 会因位置没变化跳过 slice）
        assertTrue(
            "fix2-direct: [1,5) in oldLayout 和 [2,6) in newLayout 的 bounds 应不同" +
                "（否则位置没变化，subtractOwnedRanges 会跳过 slice），" +
                "oldBounds=$oldBounds, newBounds=$newBounds",
            oldBounds != newBounds,
        )

        val move =
            ComposeReflowMove(
                oldRange = TextRange(1, 5),
                newRange = TextRange(2, 6),
                oldBounds = oldBounds,
                newBounds = newBounds,
            )

        // ownedRanges = [5,6)：部分重叠（只有 newRange 的最后一部分被接管）
        val ownedRanges = listOf(TextRange(5, 6))

        val result =
            ComposeOverlayOwnership.subtractOwnedRanges(
                move = move,
                ownedRanges = ownedRanges,
                oldLayout = oldLayoutSnapshot,
                newLayout = newLayoutSnapshot,
            )

        // 断言：结果不为空（旧 bug：部分重叠直接返回空列表）
        assertTrue(
            "fix2-direct: 部分重叠时 subtractOwnedRanges 应返回非空列表（剩余 slice），" +
                "实际 result.size=${result.size}" +
                "（旧 bug：部分重叠直接丢弃整个 reflow，返回空列表）",
            result.isNotEmpty(),
        )

        // 断言：返回的 slice 的 newRange 应为 [2,5)（从 [2,6) 中减去 [5,6)）
        assertEquals(
            "fix2-direct: 部分重叠时返回的 slice newRange 应为 [2,5)，" +
                "实际 result.newRanges=${result.map { it.newRange }}" +
                "（从 [2,6) 中减去 owned [5,6) 应得到 [2,5)）",
            listOf(TextRange(2, 5)),
            result.map { it.newRange },
        )

        // 断言：返回的 slice 的 oldRange 应为 [1,4)（按相对偏移算）
        assertEquals(
            "fix2-direct: 部分重叠时返回的 slice oldRange 应为 [1,4)，" +
                "实际 result.oldRanges=${result.map { it.oldRange }}" +
                "（按相对偏移算，move.oldRange=[1,5) 的前 3 个字符）",
            listOf(TextRange(1, 4)),
            result.map { it.oldRange },
        )
    }

    /**
     * 修复2直接测试补充：完全覆盖时返回空列表。
     *
     * ReflowMove newRange=[2,6)，已有 ownedRange=[2,6)（完全覆盖），
     * 期望：返回空列表（整个 reflow 被其他 unit 接管，不需要创建 ReflowMove）。
     */
    @Test
    fun fix2_subtractOwnedRangesFullCoverReturnsEmpty() {
        val layouts =
            captureLayoutsWithMultipleWidths(
                FIX2_OLD_TEXT to 1000,
                FIX2_NEW_TEXT to 1000,
            )

        val oldLayoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val newLayoutSnapshot = ComposeLayoutSnapshot(layouts[1], TextRange(0, 0), 0)

        val oldBounds = layouts[0].getPathForRange(1, 5).getBounds()
        val newBounds = layouts[1].getPathForRange(2, 6).getBounds()

        val move =
            ComposeReflowMove(
                oldRange = TextRange(1, 5),
                newRange = TextRange(2, 6),
                oldBounds = oldBounds,
                newBounds = newBounds,
            )

        // ownedRanges = [2,6)：完全覆盖
        val ownedRanges = listOf(TextRange(2, 6))

        val result =
            ComposeOverlayOwnership.subtractOwnedRanges(
                move = move,
                ownedRanges = ownedRanges,
                oldLayout = oldLayoutSnapshot,
                newLayout = newLayoutSnapshot,
            )

        assertTrue(
            "fix2-full: 完全覆盖时 subtractOwnedRanges 应返回空列表" +
                "（整个 reflow 被其他3 unit 接管），实际 result.size=${result.size}",
            result.isEmpty(),
        )
    }

    /**
     * 修复2直接测试补充：无重叠时返回原 move。
     *
     * ReflowMove newRange=[2,6)，已有 ownedRange=[0,1)（无重叠），
     * 期望：返回原 move（没有被其他 unit 接管的部分）。
     */
    @Test
    fun fix2_subtractOwnedRangesNoOverlapReturnsOriginal() {
        val layouts =
            captureLayoutsWithMultipleWidths(
                FIX2_OLD_TEXT to 1000,
                FIX2_NEW_TEXT to 1000,
            )

        val oldLayoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val newLayoutSnapshot = ComposeLayoutSnapshot(layouts[1], TextRange(0, 0), 0)

        val oldBounds = layouts[0].getPathForRange(1, 5).getBounds()
        val newBounds = layouts[1].getPathForRange(2, 6).getBounds()

        val move =
            ComposeReflowMove(
                oldRange = TextRange(1, 5),
                newRange = TextRange(2, 6),
                oldBounds = oldBounds,
                newBounds = newBounds,
            )

        // ownedRanges = [0,1)：无重叠（在 newRange [2,6) 之前）
        val ownedRanges = listOf(TextRange(0, 1))

        val result =
            ComposeOverlayOwnership.subtractOwnedRanges(
                move = move,
                ownedRanges = ownedRanges,
                oldLayout = oldLayoutSnapshot,
                newLayout = newLayoutSnapshot,
            )

        assertEquals(
            "fix2-no-overlap: 无重叠时 subtractOwnedRanges 应返回 1 个 slice（原 move）",
            1,
            result.size,
        )
        assertEquals(
            "fix2-no-overlap: 返回的 slice newRange 应为原 [2,6)",
            TextRange(2, 6),
            result[0].newRange,
        )
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L

        /** fix2 直接单元测试用的旧文本（一行）。 */
        const val FIX2_OLD_TEXT = "abcde"

        /** fix2 直接单元测试用的新文本（两行，'bcde' 在第二行）。 */
        const val FIX2_NEW_TEXT = "a\nbcde"
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
    ): List<TextLayoutResult> =
        captureLayoutsWithMultipleWidths(
            *texts.map { it to maxWidth }.toTypedArray(),
            fontSizeSp = fontSizeSp,
        )

    /**
     * #708 评论 5725146968：一次 setContent 内捕获多种宽度的 layout —
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
