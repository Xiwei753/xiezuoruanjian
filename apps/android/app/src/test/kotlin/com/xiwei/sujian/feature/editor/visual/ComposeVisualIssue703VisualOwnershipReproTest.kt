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
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #703 视觉所有权 / 吞字吐字协同动画问题重现测试基线。
 *
 * 基于 Issue #703 评论的根因分析，建立三个根因的可重现测试：
 *
 * - **R1 裸帧窗口**：BasicTextField 已更新真实正文（latestLayout 已是新 layout），
 *   但视觉动画要等后续 drainPendingPatchesAtFrame / sampleVisualScene 才接管，
 *   留下"裸帧"窗口——visualScene 仍为旧/空，hiddenRanges 未覆盖新插入区域。
 *
 * - **R2 alpha 超车无空间约束**：新插入文字在最终位置做 alpha 0→1，
 *   光标走独立 CursorTrack 从左追过去。动画中间时刻字已在最终位置出现（alpha>0），
 *   但光标尚未到达该位置（cursor.left < unit 位置），违反"光标经过哪里文字才出现到哪里"。
 *
 * - **R3 跨行 retainedMoves 整行接管**：跨行删除时 retainedMoves 把整行幸存文字
 *   一起交给 overlay 临时接管（oldRange 跨整行，dy 非零），
 *   BasicTextField→overlay→BasicTextField 任一环晚一帧即整行闪烁。
 *
 * 这些测试体现当前实现中违反的不变式。测试本身能编译运行；
 * 断言会失败（体现 bug 存在）或记录当前实现违反的不变式。
 *
 * 关键文件链：
 * - WritingEditorSurface.kt（InputTransformation, BasicTextField, onTextLayoutResult）
 * - ComposeEditorVisualState.kt（recordLocalInput, onAuthoritativeLayout, pendingPatches, drainPendingPatchesAtFrame）
 * - ComposeVisualTimeline.kt（inserted unit/deleted ghost, cursor track, retainedMoves）
 * - EditorTextFieldDrawLayer.kt（BasicTextField 裁切, visual text/cursor 绘制, 同一帧视觉所有权）
 * - ComposeCursorMotion.kt / ComposeLocalVisualRebase.kt / ComposeVisualRebase.kt
 */
@Suppress("StringLiteralDuplication", "MaxLineLength", "LongMethod")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue703VisualOwnershipReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== R1: 裸帧窗口 ====================

    /**
     * R1 裸帧窗口重现：BasicTextField 已更新真实正文，但视觉动画还没接管。
     *
     * 场景："" → "a" 插入。
     * 1. onAuthoritativeLayout(旧 layout, "") 建立基线
     * 2. onVisualIntent(insert "a") 生成 patch
     * 3. onAuthoritativeLayout(新 layout, "a") — BasicTextField 已画新字
     * 4. **不 drain** — 模拟 EditorTextFieldDrawLayer 的 LaunchedEffect(withFrameNanos) 还没执行到下一帧
     * 5. sampleVisualScene(now) — 视觉层仍是旧/空 scene
     *
     * 不变式（应成立但被违反）：同一帧中 BasicTextField 的真实正文（latestLayout）与
     * 视觉层接管状态（visualScene.hiddenRanges / units）必须同步——
     * 要么都旧要么都新，不能 BasicTextField 已新而 visualScene 仍旧（裸帧）。
     *
     * 当前实现：latestLayout 已是 "a"（新），但 visualScene.units 为空、hiddenRanges 为空
     * （视觉层未接管），体现裸帧窗口。
     */
    @Test
    fun r1_bareFrameWindow_visualOwnershipLagsBehindBasicTextField() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-703-r1-bare-frame")

        // 1. 建立旧 layout（空文本）
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 2. 生成插入 patch（onVisualIntent 路径，不调 classifier）
        state.onVisualIntent(
            makeInsertIntent(1L, 0L, 1L, "", "a", TextRange(0, 1)),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
        )

        // 3. 新 layout 到达 — BasicTextField 已画新字 "a"
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        // 4. patch 已入队，尚未 drain（模拟下一帧 withFrameNanos 还没执行）
        assertTrue("R1: patch 应已入队（hasPendingPatches=true）", state.hasPendingPatches())

        // 5. 采样当前视觉 scene（drain 前视觉层未接管）
        val sceneBeforeDrain = state.sampleVisualScene(0L)

        // === 裸帧窗口证据 ===
        // latestLayout 已是新 layout（"a"）— BasicTextField 已画新字
        val latestLayout = state.latestLayout.value
        assertNotNull("R1: latestLayout 应存在", latestLayout)
        val latestText = latestLayout!!.result.layoutInput.text.text
        assertEquals(
            "R1 裸帧: BasicTextField 已更新真实正文为 'a'，实际='$latestText'",
            "a",
            latestText,
        )

        // visualScene 仍未接管 — hiddenRanges 为空（未裁切新插入区域）、units 为空（无动画 unit）
        // 这就是裸帧窗口：BasicTextField 已画新字，但视觉层没接管，新字"裸"出来
        assertTrue(
            "R1 裸帧: visualScene.hiddenRanges 应为空（视觉层未接管新插入区域），" +
                "实际 size=${sceneBeforeDrain.hiddenRanges.size}；" +
                "BasicTextField 已画 'a' 但 hiddenRanges 未覆盖 [0,1)，新字裸出来",
            sceneBeforeDrain.hiddenRanges.isEmpty(),
        )
        assertTrue(
            "R1 裸帧: visualScene.units 应为空（视觉层未创建动画 unit），" +
                "实际 size=${sceneBeforeDrain.units.size}；" +
                "BasicTextField 已画 'a' 但视觉层无 inserted unit 接管",
            sceneBeforeDrain.units.isEmpty(),
        )

        // 对比：drain 后视觉层才接管
        state.drainPendingPatchesAtFrame(0L)
        val sceneAfterDrain = state.sampleVisualScene(0L)
        // drain 后视觉层接管 — hiddenRanges 非空或 units 非空
        val visualOwnedAfterDrain =
            sceneAfterDrain.hiddenRanges.isNotEmpty() || sceneAfterDrain.units.isNotEmpty()
        assertTrue(
            "R1 对比: drain 后视觉层应接管（hiddenRanges 或 units 非空），" +
                "hiddenRanges=${sceneAfterDrain.hiddenRanges.size}, units=${sceneAfterDrain.units.size}；" +
                "这证明 drain 前视觉所有权确实晚一拍（裸帧窗口存在）",
            visualOwnedAfterDrain,
        )
    }

    /**
     * R1 强化：快速连续输入 3 步，每步都有裸帧窗口。
     *
     * 场景："" → "a" → "ab" → "abc"，每步 onAuthoritativeLayout 后立即采样（不 drain）。
     * 每步都应观察到 latestLayout 已更新但 visualScene 未接管。
     */
    @Test
    fun r1_rapidInput_eachStepHasBareFrameWindow() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val state = ComposeEditorVisualState(targetId = "test-703-r1-rapid")

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        for (i in 1..3) {
            val prevText = "a".repeat(i - 1)
            val currText = "a".repeat(i)

            state.onAuthoritativeLayout(layouts[i - 1], TextRange(i - 1, i - 1), 0)
            state.onVisualIntent(
                makeInsertIntent(
                    i.toLong(),
                    (i - 1).toLong(),
                    i.toLong(),
                    prevText,
                    currText,
                    TextRange(i - 1, i),
                ),
                EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
            )
            state.onAuthoritativeLayout(layouts[i], TextRange(i, i), 0)

            // 不 drain — 采样裸帧
            val sceneBeforeDrain = state.sampleVisualScene(0L)
            val latestText = state.latestLayout.value?.result?.layoutInput?.text?.text ?: ""

            // 裸帧证据：latestLayout 已是 currText，但 visualScene 未接管
            assertEquals(
                "R1 快速输入 step $i: latestLayout 应为 '$currText'，实际='$latestText'",
                currText,
                latestText,
            )
            assertTrue(
                "R1 快速输入 step $i: 裸帧窗口 — visualScene.units 应为空（视觉层未接管），" +
                    "实际 size=${sceneBeforeDrain.units.size}",
                sceneBeforeDrain.units.isEmpty(),
            )

            // drain 后才接管
            state.drainPendingPatchesAtFrame(0L)
        }
    }

    // ==================== R2: alpha 超车无空间约束 ====================

    /**
     * R2 alpha 超车重现：新插入文字在最终位置做 alpha 0→1，光标从左追过去。
     *
     * 场景：一次提交 "abc"（3 个 insertedUnits），cursor path 3 个 point
     * （left=10, 20, 30），duration=300ms。
     *
     * 在 50ms（cursor 在 fromRect→point0 中间，约 left=5）采样：
     * - unit 'a'（targetRange=[0,1)）的 alpha 已 > 0（字已在最终位置出现）
     * - unit 'a' 的 position.from 已是最终位置（≈ point0.rect.left=10）
     * - cursor.left 还在中间（≈5，尚未到达 unit 'a' 的位置 10）
     *
     * 这违反"光标经过哪里，文字才出现到哪里"的空间约束——
     * 字先在最终位置出现（alpha 超车），光标才从左追过去。
     *
     * 真正的吞字吐字：字应该随光标扫过才出现，而非在最终位置独立 alpha 淡入。
     */
    @Test
    fun r2_alphaOvertake_insertedUnitAtFinalPositionWhileCursorStillCatchingUp() {
        val layouts = captureLayouts("", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()

        // 3 个 point 在不同水平位置（left=10, 20, 30）
        val point0 = CursorMotionPoint(rect = Rect(10f, 0f, 12f, 14f), endFraction = 1f / 3f)
        val point1 = CursorMotionPoint(rect = Rect(20f, 0f, 22f, 14f), endFraction = 2f / 3f)
        val point2 = CursorMotionPoint(rect = Rect(30f, 0f, 32f, 14f), endFraction = 1f)
        val cursorPath = CursorMotionPath(points = listOf(point0, point1, point2))

        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath,
                durationMs = 300L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 300L, cursorEnabled = true, coordinated = true),
            )

        // fromRect 在起点（left=0）
        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // 在 50ms 采样（cursor 在 fromRect→point0 中间，progress=50/100=0.5 对第一段）
        // cursor endFraction=1/3 对应 100ms，50ms 时 segmentProgress=0.5
        // cursor.left ≈ fromRect.left + (point0.left - fromRect.left) * 0.5 = 0 + 10*0.5 = 5
        val scene50 = timeline.sample(50L * NANOS_PER_MS)

        // === R2 alpha 超车证据 ===
        // unit 'a'（targetRange=[0,1)）：startedAt=0, duration=100ms，50ms 时 alpha≈0.5
        val unitA = scene50.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("R2: unit 'a' 应存在", unitA)
        val unitAlpha = unitA!!.alpha.from
        assertTrue(
            "R2 alpha 超车: unit 'a' alpha 应已 > 0（字已在最终位置出现），实际=$unitAlpha；" +
                "50ms 时字已开始淡入（alpha≈0.5），但光标尚未到达字的位置",
            unitAlpha > 0.1f,
        )

        // unit 'a' 的 position.from 已是最终位置（≈ point0.rect.left=10）
        val unitPosition = unitA.position.from
        assertTrue(
            "R2 alpha 超车: unit 'a' position 应在最终位置（x≈10），实际 x=${unitPosition.x}；" +
                "字直接在最终位置 alpha 淡入，而非随光标扫过才出现",
            kotlin.math.abs(unitPosition.x - 10f) < 2f,
        )

        // cursor 还在中间（left≈5），尚未到达 unit 'a' 的位置（10）
        val cursor50 = scene50.cursorRect
        assertNotNull("R2: cursor rect 应存在", cursor50)
        val cursorLeft = cursor50!!.left
        assertTrue(
            "R2 alpha 超车: cursor 应还在中间（left≈5），实际=$cursorLeft；" +
                "光标尚未到达 unit 'a' 位置（10），但字已在该位置出现（alpha=$unitAlpha）——" +
                "违反'光标经过哪里文字才出现到哪里'空间约束",
            cursorLeft < 8f,
        )

        // 核心违反：unit alpha > 0（字已出现）但 cursor.left < unit 位置（光标还没到）
        assertTrue(
            "R2 核心违反: 字已在最终位置出现（alpha=$unitAlpha, pos.x=${unitPosition.x}）" +
                "但光标尚未到达（cursor.left=$cursorLeft < ${unitPosition.x}）。" +
                "真正吞字吐字应满足空间约束：cursor.left >= unit 位置 时 unit alpha 才 > 0。",
            unitAlpha > 0.1f && cursorLeft < unitPosition.x - 1f,
        )
    }

    /**
     * R2 强化：删除时 ghost 在旧位置淡出，光标已往回移动但 ghost 还在淡出。
     *
     * 场景："a" → "" 删除。ghost unit 在旧位置 alpha→0，cursor 从旧位置往回移到起点。
     * 在中间时刻：ghost alpha 仍 > 0（字还在淡出），但 cursor 已往回移动（cursor.left < ghost 位置）。
     * 违反"光标经过哪里文字才消失到哪里"——光标已移回但 ghost 还在原地淡出。
     */
    @Test
    fun r2_alphaOvertake_delete_ghostFadesWhileCursorAlreadyMovedBack() {
        val layouts = captureLayouts("a", "")
        val aLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val emptyLayout = ComposeLayoutSnapshot(layouts[1], TextRange(0, 0), 0)

        val timeline = ComposeVisualTimeline()

        // cursor 从 'a' 末尾（left≈7）移到空文本起点（left=0）
        val oldCursorRect = Rect(7f, 0f, 9f, 14f)
        val newCursorRect = Rect(0f, 0f, 2f, 14f)
        val cursorPath =
            CursorMotionPath(
                points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)),
            )

        val patch =
            makePatch(
                id = 1L,
                oldLayout = aLayout,
                newLayout = emptyLayout,
                deletedUnits = listOf(TextRange(0, 1)),
                cursorMotionPath = cursorPath,
                durationMs = 100L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
            )

        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = oldCursorRect,
            cursorPath = cursorPath.points,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // 在 50ms 采样（动画中间）
        val scene50 = timeline.sample(50L * NANOS_PER_MS)

        // ghost unit（targetRange=null，range=[0,1)）：alpha 从 1→0，50ms 时 alpha≈0.5
        val ghost = scene50.units.firstOrNull { it.targetRange == null && it.range == TextRange(0, 1) }
        assertNotNull("R2 删除: ghost unit 应存在", ghost)
        val ghostAlpha = ghost!!.alpha.from
        assertTrue(
            "R2 删除 alpha 超车: ghost alpha 应仍 > 0（字还在淡出），实际=$ghostAlpha",
            ghostAlpha > 0.1f,
        )

        // cursor 已往回移动（50ms 时 cursor.left ≈ 3.5，在 oldRect(7)→newRect(0) 中间）
        val cursor50 = scene50.cursorRect
        assertNotNull("R2 删除: cursor rect 应存在", cursor50)
        val cursorLeft = cursor50!!.left
        // ghost 位置（旧位置，x≈7）vs cursor 位置（已往回，x<5）
        val ghostPosition = ghost.position.from
        assertTrue(
            "R2 删除 alpha 超车: cursor 已往回移动（left=$cursorLeft）但 ghost 仍在旧位置淡出" +
                "（pos.x=${ghostPosition.x}, alpha=$ghostAlpha）。" +
                "违反'光标经过哪里文字才消失到哪里'——光标已移回但 ghost 还在原地淡出。",
            cursorLeft < ghostPosition.x - 1f && ghostAlpha > 0.1f,
        )
    }

    // ==================== R3: 跨行 retainedMoves 整行接管 ====================

    /**
     * R3 跨行 retainedMoves 整行接管重现。
     *
     * 场景：oldText = "abc\ndef"（两行），删除换行符（index 3），newText = "abcdef"。
     * 'def' 从第 1 行移到第 0 行尾，整行位移（dy 非零）。
     *
     * 用 ComposeVisualRebase.computeRetainedMoves(oldLayout, newLayout, chain) 计算。
     * 断言：retainedMoves 包含 'def' 整行的 RetainedMove（oldRange 跨整行 'def'，dy 非零），
     * 体现"整行幸存文字被交给 overlay 临时接管"。
     *
     * 根因：overlay 临时接管整行 'def'，经历 BasicTextField→overlay→BasicTextField，
     * 任一环晚一帧就整行闪一下。
     */
    @Test
    fun r3_crossLineDelete_retainedMovesTakesOverEntireSurvivingLine() {
        // 用窄布局让 "abc\ndef" 跨两行（maxWidth=50，fontSize=14sp）
        val layouts = captureLayoutsWithWidth(arrayOf("abc\ndef", "abcdef"), 50)
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(3, 3), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)

        // 构造删除换行符的 intent：oldText="abc\ndef", newText="abcdef", 删除 [3,4)（换行符）
        val deleteIntent = makeDeleteIntent(
            coreTxnId = 1L,
            baseRev = 0L,
            newRev = 1L,
            oldText = "abc\ndef",
            newText = "abcdef",
            deletedRange = TextRange(3, 4),
        )

        // 计算 retainedMoves（第二个 overload：oldLayout, newLayout, chain）
        val retainedMoves =
            ComposeVisualRebase.computeRetainedMoves(
                oldLayout,
                newLayout,
                listOf(deleteIntent),
            )

        // === R3 跨行整行接管证据 ===
        // 'def' 在旧正文中是 [4,7)（第 1 行），删除换行符后移到 [3,6)（第 0 行尾）
        // retainedMoves 应包含覆盖 'def' 的 RetainedMove
        val defMove =
            retainedMoves.firstOrNull { move ->
                // oldRange 覆盖 'def' 的一部分或全部
                move.oldRange.start <= 4 && move.oldRange.end >= 7
            }

        // 如果没找到完全覆盖 [4,7) 的，找与 'def' 区域 [4,7) 有重叠的
        val defOverlappingMove =
            defMove ?: retainedMoves.firstOrNull { move ->
                move.oldRange.start < 7 && move.oldRange.end > 4
            }

        assertNotNull(
            "R3 跨行: retainedMoves 应包含覆盖 'def' 的 move，" +
                "实际 retainedMoves=${retainedMoves.map { "${it.oldRange}->${it.newRange}" }}；" +
                "若为空说明跨行删除未生成 retainedMoves（可能被其他路径处理）",
            defOverlappingMove,
        )

        // 检查是否有跨行位移（dy 非零）的 move
        // RetainedMove 只有 oldRange/newRange，dy 需要从 layout 算
        val move = defOverlappingMove!!
        val oldBounds = ComposeVisualRebase.safePathBounds(oldLayout.result, move.oldRange)
        val newBounds = ComposeVisualRebase.safePathBounds(newLayout.result, move.newRange)
        assertNotNull("R3: oldBounds 应存在", oldBounds)
        assertNotNull("R3: newBounds 应存在", newBounds)

        val dy = newBounds!!.top - oldBounds!!.top
        val dx = newBounds.left - oldBounds.left

        // 'def' 从第 1 行移到第 0 行，dy 应非零（跨行位移）
        assertTrue(
            "R3 跨行整行接管: 'def' 应有跨行位移（dy 非零），实际 dy=$dy, dx=$dx；" +
                "oldRange=${move.oldRange}(${oldBounds.top}..${oldBounds.bottom}), " +
                "newRange=${move.newRange}(${newBounds.top}..${newBounds.bottom})。" +
                "retainedMoves 把整行幸存文字交给 overlay 临时接管，" +
                "BasicTextField→overlay→BasicTextField 任一环晚一帧即整行闪烁。",
            kotlin.math.abs(dy) > 1f,
        )

        // 证据：retainedMoves 覆盖了整行幸存文字 'def'（oldRange 跨越多行或整行）
        val moveLength = move.oldRange.end - move.oldRange.start
        assertTrue(
            "R3 跨行整行接管: retainedMove 覆盖长度=$moveLength（oldRange=${move.oldRange}），" +
                "'def' 长度=3。整行幸存文字被交给 overlay 接管。",
            moveLength >= 1,
        )
    }

    /**
     * R3 强化：通过 ComposeEditorVisualState 完整路径验证跨行删除 retainedMoves。
     *
     * 用 onAuthoritativeLayout + onVisualIntent(delete) + onAuthoritativeLayout + drain，
     * 检查 latestPatch.value.retainedMoves 包含跨行位移。
     */
    @Test
    fun r3_crossLineDelete_fullPathRetainedMovesTakesOverSurvivingLine() {
        val layouts = captureLayoutsWithWidth(arrayOf("abc\ndef", "abcdef"), 50)
        val state = ComposeEditorVisualState(targetId = "test-703-r3-full-path")

        // 建立旧 layout
        state.onAuthoritativeLayout(layouts[0], TextRange(3, 3), 0)

        // 删除换行符
        state.onVisualIntent(
            makeDeleteIntent(
                coreTxnId = 1L,
                baseRev = 0L,
                newRev = 1L,
                oldText = "abc\ndef",
                newText = "abcdef",
                deletedRange = TextRange(3, 4),
            ),
            EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
        )

        // 新 layout 到达
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)

        // drain
        state.drainPendingPatchesAtFrame(0L)

        // 检查 latestPatch 的 retainedMoves
        val latestPatch = state.latestPatch.value
        assertNotNull("R3 完整路径: latestPatch 应存在", latestPatch)
        val retainedMoves = latestPatch!!.retainedMoves

        // 找与 'def' 区域 [4,7) 有重叠的 move
        val defMove =
            retainedMoves.firstOrNull { move ->
                move.oldRange.start < 7 && move.oldRange.end > 4
            }

        // 跨行删除应生成 retainedMoves 接管 'def'
        assertNotNull(
            "R3 完整路径: latestPatch.retainedMoves 应包含覆盖 'def' 的 move，" +
                "实际=${retainedMoves.map { "${it.oldRange}->${it.newRange}" }}；" +
                "跨行删除时整行幸存文字被交给 overlay 临时接管",
            defMove,
        )

        // 采样验证 scene 中有 retained reflow 的 unit（'def' 被 overlay 接管）
        val scene = state.sampleVisualScene(0L)
        // retainedMoves 非空时，scene 应有 unit（overlay 接管的幸存文字）
        if (retainedMoves.isNotEmpty()) {
            assertTrue(
                "R3 完整路径: retainedMoves 非空时 scene 应有 unit（overlay 接管幸存文字），" +
                    "实际 units.size=${scene.units.size}；" +
                    "这些 unit 经历 BasicTextField→overlay→BasicTextField，任一环晚一帧即整行闪烁",
                scene.units.isNotEmpty() || scene.hiddenRanges.isNotEmpty(),
            )
        }
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
            cursorMotionPath = cursorMotionPath,
            durationMs = durationMs,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            motionPolicy = motionPolicy,
        )

    private fun makeInsertIntent(
        coreTxnId: Long,
        baseRev: Long,
        newRev: Long,
        oldText: String,
        newText: String,
        newRange: TextRange,
        offsetMap: VisualOffsetMap? = null,
        durationMs: Long = 100L,
    ): EditorVisualIntent =
        EditorVisualIntent(
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = durationMs,
            offsetMap = offsetMap,
            oldRanges = emptyList(),
            newRanges = listOf(newRange),
            textKind = TextVisualKind.Insert,
            cursor =
                CursorVisualIntent(
                    oldEndUtf16 = oldText.length,
                    newEndUtf16 = newText.length,
                    animate = true,
                ),
            replaceBounds =
                VisualReplaceBounds(
                    oldStart = oldText.length,
                    oldEnd = oldText.length,
                    newStart = newRange.start,
                    newEnd = newRange.end,
                ),
            expectedOldText = oldText,
            expectedNewText = newText,
        )

    @Suppress("LongParameterList")
    private fun makeDeleteIntent(
        coreTxnId: Long,
        baseRev: Long,
        newRev: Long,
        oldText: String,
        newText: String,
        deletedRange: TextRange,
        offsetMap: VisualOffsetMap? = null,
    ): EditorVisualIntent =
        EditorVisualIntent(
            coreTransactionId = coreTxnId,
            baseRevision = baseRev,
            newRevision = newRev,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = offsetMap,
            oldRanges = listOf(deletedRange),
            newRanges = emptyList(),
            textKind = TextVisualKind.Delete,
            cursor =
                CursorVisualIntent(
                    oldEndUtf16 = oldText.length,
                    newEndUtf16 = newText.length,
                    animate = true,
                ),
            replaceBounds =
                VisualReplaceBounds(
                    oldStart = deletedRange.start,
                    oldEnd = deletedRange.end,
                    newStart = newText.length,
                    newEnd = newText.length,
                ),
            expectedOldText = oldText,
            expectedNewText = newText,
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
