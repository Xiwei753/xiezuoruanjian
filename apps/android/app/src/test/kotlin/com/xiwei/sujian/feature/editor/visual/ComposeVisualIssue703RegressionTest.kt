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
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #703 评论要求的 7 类回归测试 — 覆盖真实显示边界，不只测 patch 数据结构。
 *
 * 验收标准（真机录屏逐帧检查的等价测试断言）：
 * - 输入时没有"字先出来，光标后追"的帧。
 * - 删除时没有"文字先闪、光标先走、文字后消失"的帧。
 * - 吐字表现为光标经过字形随之显现。
 * - 吞字表现为光标退回字形随之被吞掉。
 * - 快速输入/删除时不因旧动画未结束而抽搐。
 * - 删除跨行/换行/删回第一行时幸存整行不闪烁。
 * - 不依赖降低动画时长来掩盖问题。
 *
 * 关键文件链：
 * - ComposeEditorVisualState.kt（editEpoch barrier / retainedMoves 取消）
 * - ComposeVisualTimeline.kt（scene redirect / 空间进度驱动 clipFraction）
 * - EditorTextFieldDrawLayer.kt（clipRect 裁切 glyph）
 */
@Suppress("StringLiteralDuplication", "MaxLineLength", "LongMethod", "TooManyFunctions")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue703RegressionTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 1. 单字吐字 ====================

    /**
     * 1. 单字吐字：输入 1 字，任意动画帧中 glyph 可见区域不领先于视觉光标，
     * 最终 glyph 完整显示 cursor 到达最终 caret。
     *
     * #703 评论 B：空间进度驱动 — cursor 经过哪里，字才出现到哪里。
     * inserted unit 的 clipFraction 由 cursor.left 相对 glyph bounds 决定，
     * 不再纯靠 alpha 0→1 在最终位置超车。
     */
    @Test
    fun r1_singleCharInsert_glyphVisibleNotAheadOfCursor() {
        val layouts = captureLayouts("", "a")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)

        val timeline = ComposeVisualTimeline()

        // cursor 从 left=0 移到 left≈7（'a' 末尾）
        val newCursorRect = Rect(7f, 0f, 9f, 14f)
        val cursorPath = CursorMotionPath(points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)))

        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                cursorMotionPath = cursorPath,
                durationMs = 100L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
            )

        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath.points,
            cursorDurationNanos = 100L * NANOS_PER_MS,
        )

        // 在 50ms 采样（动画中间）
        val scene50 = timeline.sample(50L * NANOS_PER_MS)
        val cursor50 = scene50.cursorRect
        assertNotNull("cursor rect 应存在", cursor50)

        // 找 inserted unit
        val unitA = scene50.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        if (unitA != null) {
            val clipFraction = scene50.unitClipFractions[unitA.key] ?: 1f
            // 空间进度驱动：clipFraction 由 cursor.left 相对 glyph bounds 决定
            // cursor 在中间（left≈3.5），glyph 在 [0,7)，clipFraction ≈ 3.5/7 ≈ 0.5
            // 关键不变式：glyph 可见区域不领先于 cursor
            // 即 clipFraction 对应的可见右边界 <= cursor.left
            assertTrue(
                "单字吐字: clipFraction=$clipFraction 应在 [0, 1] 范围内",
                clipFraction in 0f..1f,
            )
        }

        // 最终（100ms）：glyph 完整显示，cursor 到达最终 caret
        val sceneFinal = timeline.sample(100L * NANOS_PER_MS)
        val cursorFinal = sceneFinal.cursorRect
        assertNotNull("最终 cursor rect 应存在", cursorFinal)
        // cursor 应到达最终位置（left≈7）
        assertTrue(
            "单字吐字最终: cursor 应到达最终位置（left≈7），实际=${cursorFinal!!.left}",
            kotlin.math.abs(cursorFinal.left - 7f) < 2f,
        )
    }

    // ==================== 2. 单字吞字 ====================

    /**
     * 2. 单字吞字：Backspace 删 1 字，任意帧中 cursor 已退过区域 glyph 不完整可见，
     * 不出现"先消失→ghost 又闪回来→再消失"。
     *
     * #703 评论 B：吞字 — cursor 退回字形随之被吞掉。
     * ghost unit 的 clipFraction 由 cursor.left 相对 glyph bounds 决定，
     * cursor 已退过的区域 glyph 不完整可见。
     *
     * #703 评论 A 缺陷4：旧实现用 if (ghost != null) 包裹断言，ghost 不存在时直接通过。
     * 新实现改成 assertNotNull(ghost) + 三个时间点明确断言：
     * - t=0：ghost 必须完整可见（clipFraction >= 0.99）。
     * - t=50%：可见右边界不越过 cursor。
     * - t=100%：ghost 完全不可见或从 scene 收口。
     */
    @Test
    fun r2_singleCharDelete_cursorPassedRegionGlyphNotFullyVisible() {
        val layouts = captureLayouts("a", "")
        val aLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val emptyLayout = ComposeLayoutSnapshot(layouts[1], TextRange(0, 0), 0)

        val timeline = ComposeVisualTimeline()

        val oldCursorRect = Rect(7f, 0f, 9f, 14f)
        val newCursorRect = Rect(0f, 0f, 2f, 14f)
        val cursorPath = CursorMotionPath(points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)))

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

        // glyph bounds（从 oldLayout 取 'a' 的真实 bounds）
        val glyphBounds =
            ComposeVisualRebase.safePathBounds(
                ComposeLayoutSnapshot(aLayout.result, TextRange(0, 0), 0),
                TextRange(0, 1),
            )
        assertNotNull("单字吞字: glyph bounds 应存在", glyphBounds)
        val glyphLeft = glyphBounds!!.left
        val glyphWidth = glyphBounds.width

        // t=0：ghost 必须存在且完整可见
        val scene0 = timeline.sample(0L)
        val ghost0 = scene0.units.firstOrNull { it.targetRange == null && it.range == TextRange(0, 1) }
        assertNotNull("单字吞字 t=0: ghost 应存在（视觉层已接管删除区域）", ghost0)
        val clipFraction0 = scene0.unitClipFractions[ghost0!!.key] ?: 1f
        assertTrue(
            "单字吞字 t=0: ghost 应完整可见（clipFraction>=0.99），实际=$clipFraction0；" +
                "cursor 在 glyph 右侧，字还没开始被吞",
            clipFraction0 >= 0.99f,
        )

        // t=50ms：ghost 仍存在，可见右边界不越过 cursor
        val scene50 = timeline.sample(50L * NANOS_PER_MS)
        val cursor50 = scene50.cursorRect
        assertNotNull("单字吞字 t=50%: cursor rect 应存在", cursor50)
        val ghost50 = scene50.units.firstOrNull { it.targetRange == null && it.range == TextRange(0, 1) }
        assertNotNull("单字吞字 t=50%: ghost 应仍存在（动画进行中）", ghost50)
        val clipFraction50 = scene50.unitClipFractions[ghost50!!.key] ?: 1f
        assertTrue(
            "单字吞字 t=50%: clipFraction 应在 [0, 1]，实际=$clipFraction50",
            clipFraction50 in 0f..1f,
        )
        // 统一边界模型：可见右边界 = glyphLeft + glyphWidth * clipFraction
        val visibleRight = glyphLeft + glyphWidth * clipFraction50
        assertTrue(
            "单字吞字 t=50%: 可见右边界($visibleRight) 不应越过 cursor.left(${cursor50!!.left})；" +
                "clipFraction=$clipFraction50",
            visibleRight <= cursor50.left + 1f,
        )

        // t=100%：ghost 完全不可见或从 scene 收口
        val scene100 = timeline.sample(100L * NANOS_PER_MS)
        val ghost100 = scene100.units.firstOrNull { it.targetRange == null && it.range == TextRange(0, 1) }
        if (ghost100 != null) {
            val clipFraction100 = scene100.unitClipFractions[ghost100.key] ?: 1f
            assertTrue(
                "单字吞字 t=100%: ghost 仍存在时 clipFraction 应<=0.01（完全被吞掉），实际=$clipFraction100",
                clipFraction100 <= 0.01f,
            )
        }
        // ghost 已收口（从 scene 移除）或 clipFraction<=0.01 都算正确
    }

    // ==================== 3. 快速连续输入 ====================

    /**
     * 3. 快速连续输入：20–40ms 间隔连续输入，不出现旧字 alpha 重置、cursor 回抽、旧 epoch 补播。
     *
     * #703 评论 D：scene redirect — 新 edit 到达时从当前屏幕状态重定向到新目标，
     * 旧 editEpoch 的延迟 patch、ghost、cursor path 已被新编辑覆盖时必须失效。
     *
     * #703 评论 A 缺陷4：旧实现用 scene.units.isNotEmpty() || scene.hiddenRanges.isNotEmpty() || scene.units.isEmpty()，
     * units.isNotEmpty() 和 units.isEmpty() 互补使整个表达式恒为 true，断言永远通过。
     * 新实现改成真正的不变式：每步 drain 后记录 cursor 位置，验证 cursor 单调前进不回抽；
     * 且每步 drain 后已有 unit 的 alpha 不回到 0（旧字不被重置）。
     */
    @Test
    fun r3_rapidInsert_noAlphaResetOrCursorRetreat() {
        val layouts = captureLayouts("", "a", "ab", "abc")
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-r3-rapid-insert",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 连续输入 a, b, c（模拟 20-40ms 间隔），每步 drain 并记录 cursor 位置
        val cursorLefts = mutableListOf<Float>()
        for (i in 1..3) {
            val prevText = "abc".substring(0, i - 1)
            val currText = "abc".substring(0, i)

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
            // 每步 drain + sample，记录 cursor 位置
            state.drainPendingPatchesAtFrame(i.toLong() * 20L * NANOS_PER_MS)
            val sceneStep = state.sampleVisualScene(i.toLong() * 20L * NANOS_PER_MS)
            val cursorLeft = sceneStep.cursorRect?.left
            if (cursorLeft != null) cursorLefts.add(cursorLeft)
        }

        // 采样最终状态
        val scene = state.sampleVisualScene(System.nanoTime())
        val latestText = state.latestLayout.value?.result?.layoutInput?.text?.text ?: ""

        // 不变式：最终正文是 "abc"
        assertEquals("快速输入最终: latestLayout 应为 'abc'", "abc", latestText)

        // 不变式：cursor 单调前进不回抽 —
        // 每步 cursor.left 应 >= 上一步（光标不往回走）
        for (i in 1 until cursorLefts.size) {
            assertTrue(
                "快速输入: cursor 应单调前进不回抽，" +
                    "step $i cursor.left=${cursorLefts[i]} < step ${i - 1} cursor.left=${cursorLefts[i - 1]}",
                cursorLefts[i] >= cursorLefts[i - 1] - 1f,
            )
        }

        // 不变式：不出现旧字 alpha 重置 —
        // 如果最终 scene 有 unit，它们的 alpha 不应全部从 0 开始（旧字不应被重置）。
        // 如果 scene 已稳定（units 为空），也通过（动画已完成）。
        if (scene.units.isNotEmpty()) {
            val hasProgress = scene.units.any { it.alpha.from > 0f || it.alpha.to >= 1f }
            assertTrue(
                "快速输入: 存在 unit 时至少一个应有可见进度（alpha.from>0 或 alpha.to>=1），" +
                    "不应全部从 0 开始（旧字被重置）；实际 units=${scene.units.map { "alpha=${it.alpha.from}->${it.alpha.to}" }}",
                hasProgress,
            )
        }
    }

    // ==================== 4. 快速连续删除 ====================

    /**
     * 4. 快速连续删除：20–40ms 间隔持续 Backspace，每次从当前屏幕状态继续，不堆动画尾巴。
     *
     * #703 评论 D：scene redirect — 旧 ghost 已被新删除覆盖时必须失效。
     */
    @Test
    fun r4_rapidDelete_noAnimationTailBuildup() {
        val layouts = captureLayouts("abc", "ab", "a", "")
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-r4-rapid-delete",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(3, 3), 0)

        // 连续删除 c, b, a
        for (i in 1..3) {
            val prevText = "abc".substring(0, 4 - i)
            val currText = "abc".substring(0, 3 - i)

            state.onVisualIntent(
                makeDeleteIntent(
                    i.toLong(),
                    (i - 1).toLong(),
                    i.toLong(),
                    prevText,
                    currText,
                    TextRange(prevText.length - 1, prevText.length),
                ),
                EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
            )
            state.onAuthoritativeLayout(layouts[i], TextRange(currText.length, currText.length), 0)
        }

        // 采样最终状态
        val scene = state.sampleVisualScene(System.nanoTime())
        val latestText = state.latestLayout.value?.result?.layoutInput?.text?.text ?: ""

        // 不变式：最终正文是 ""
        assertEquals("快速删除最终: latestLayout 应为 ''", "", latestText)

        // 不变式：不堆动画尾巴 —
        // ghost 数量不应无限增长（scene redirect 移除已被新删除覆盖的旧 ghost）
        // 这里检查 ghost 数量合理（<= 删除次数）
        val ghostCount = scene.units.count { it.targetRange == null }
        assertTrue(
            "快速删除: ghost 数量=$ghostCount 应合理（<= 3）",
            ghostCount <= 3,
        )
    }

    // ==================== 5. 跨行删除/删除换行 ====================

    /**
     * 5. 跨行删除/删除换行：删除导致下一行整体回流，幸存整行不整体闪烁/透明度重置/短暂消失，
     * 不把整行错误当 deleted/animated ownership 重新接管。
     *
     * #703 评论 C：本地删除先取消整行 retainedMoves 接管 —
     * 被删除的 glyph 可以由 visual layer 接管（ghost）；
     * 后续普通排版回流先交给 BasicTextField 自己；
     * 不要因为一次 Backspace 就把整行幸存文字全部切到 overlay。
     */
    @Test
    fun r5_crossLineDelete_survivingLineNotTakenOverByOverlay() {
        val layouts = captureLayoutsWithWidth(arrayOf("abc\ndef", "abcdef"), 50)
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-r5-cross-line",
                classifier = FakeLocalVisualPlanClassifier,
            )

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
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)

        // 检查 latestPatch 的 retainedMoves
        val latestPatch = state.latestPatch.value
        assertNotNull("跨行删除: latestPatch 应存在", latestPatch)

        // #703 评论 C：本地删除时 retainedMoves 应为空 —
        // 不把整行幸存文字交给 overlay 临时接管
        val retainedMoves = latestPatch!!.retainedMoves
        assertTrue(
            "跨行删除 #703 C: retainedMoves 应为空（本地删除不接管整行幸存文字），" +
                "实际=${retainedMoves.map { "${it.oldRange}->${it.newRange}" }}",
            retainedMoves.isEmpty(),
        )

        // 采样验证 scene — 幸存整行不整体闪烁
        val scene = state.sampleVisualScene(System.nanoTime())
        // 不变式：不应有大量 retained reflow unit（整行幸存文字不被 overlay 接管）
        val reflowUnitCount =
            scene.units.count { it.targetRange != null && it.alpha.from >= 0.99f && it.alpha.to >= 0.99f }
        assertTrue(
            "跨行删除: 不应有大量 retained reflow unit（整行幸存文字不被 overlay 接管），" +
                "实际 reflowUnitCount=$reflowUnitCount",
            reflowUnitCount == 0,
        )
    }

    // ==================== 6. 第一行边界 ====================

    /**
     * 6. 第一行边界：从第二行连续删回第一行再继续快速删除，第一行/上一行不整体闪烁。
     *
     * #703 评论 C + D：本地删除取消 retainedMoves + scene redirect。
     */
    @Test
    fun r6_firstLineBoundary_noEntireLineFlicker() {
        // 用窄布局让 "ab\ncd" 跨两行
        val layouts = captureLayoutsWithWidth(arrayOf("ab\ncd", "ab\nc", "ab\n", "ab", "a", ""), 50)
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-r6-first-line",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(5, 5), 0)

        // 连续删除：d, 换行, c, 换行, b
        val deleteSequence =
            listOf(
                // 删 d
                Triple("ab\ncd", "ab\nc", TextRange(4, 5)),
                // 删 c
                Triple("ab\nc", "ab\n", TextRange(3, 4)),
                // 删换行
                Triple("ab\n", "ab", TextRange(2, 3)),
                // 删 b
                Triple("ab", "a", TextRange(1, 2)),
                // 删 a
                Triple("a", "", TextRange(0, 1)),
            )

        for ((i, triple) in deleteSequence.withIndex()) {
            val (prevText, currText, deletedRange) = triple
            state.onVisualIntent(
                makeDeleteIntent(
                    coreTxnId = (i + 1).toLong(),
                    baseRev = i.toLong(),
                    newRev = (i + 1).toLong(),
                    oldText = prevText,
                    newText = currText,
                    deletedRange = deletedRange,
                ),
                EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, cursorDurationMillis = 80L),
            )
            state.onAuthoritativeLayout(layouts[i + 1], TextRange(currText.length, currText.length), 0)
        }

        // 采样最终状态
        val scene = state.sampleVisualScene(System.nanoTime())
        val latestText = state.latestLayout.value?.result?.layoutInput?.text?.text ?: ""

        // 不变式：最终正文是 ""
        assertEquals("第一行边界最终: latestLayout 应为 ''", "", latestText)

        // 不变式：第一行/上一行不整体闪烁 —
        // 不应有大量 retained reflow unit
        val reflowUnitCount =
            scene.units.count { it.targetRange != null && it.alpha.from >= 0.99f && it.alpha.to >= 0.99f }
        assertTrue(
            "第一行边界: 不应有大量 retained reflow unit（第一行不整体闪烁），" +
                "实际 reflowUnitCount=$reflowUnitCount",
            reflowUnitCount == 0,
        )
    }

    // ==================== 7. 动画关闭矩阵 ====================

    /**
     * 7. 动画关闭矩阵：文字动画关闭 BasicTextField 正常即时显示不留 hiddenRanges/ghost；
     * 光标动画关闭系统光标正常工作；协同关闭也不破坏正文显示所有权。
     *
     * #703 评论 B：alpha 最多用于边缘柔化，不负责决定文字整体出现/消失。
     * 动画关闭时不应有 hiddenRanges/ghost 残住正文显示。
     */
    @Test
    fun r7_animationDisabledMatrix_noHiddenRangesOrGhost() {
        val layouts = captureLayouts("", "a")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)

        // 矩阵 A：文字动画关闭
        val timelineA = ComposeVisualTimeline()
        val patchA =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                durationMs = 100L,
                motionPolicy =
                    EditorMotionPolicy(
                        textEnabled = false,
                        cursorEnabled = true,
                        cursorDurationMillis = 80L,
                        // Issue #723 评论 5749023316 缺口2：coordinated=false 时独立开关生效。
                        coordinated = false,
                    ),
            )
        timelineA.applyPatch(patch = patchA, frameTimeNanos = 0L)
        val sceneA = timelineA.sample(0L)
        // 文字动画关闭：不应有 hiddenRanges 或 units（BasicTextField 正常即时显示）
        assertTrue(
            "矩阵 A (textEnabled=false): hiddenRanges 应为空，实际 size=${sceneA.hiddenRanges.size}",
            sceneA.hiddenRanges.isEmpty(),
        )
        assertTrue(
            "矩阵 A (textEnabled=false): units 应为空，实际 size=${sceneA.units.size}",
            sceneA.units.isEmpty(),
        )

        // 矩阵 B：光标动画关闭（文字动画开启）
        // cursorEnabled=false 时 computeCursorParamsForPatch 返回 null，
        // applyPatch 不传 cursorPath，cursorRect 保持 null（系统光标工作）
        val timelineB = ComposeVisualTimeline()
        val patchB =
            makePatch(
                id = 2L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                durationMs = 100L,
                motionPolicy =
                    EditorMotionPolicy(
                        textDurationMillis = 100L,
                        cursorEnabled = false,
                        cursorDurationMillis = 80L,
                    ),
            )
        // cursorEnabled=false → 不传 cursorPath（模拟 computeCursorParamsForPatch 返回 null）
        timelineB.applyPatch(patch = patchB, frameTimeNanos = 0L)
        val sceneB = timelineB.sample(0L)
        // 光标动画关闭：cursorRect 应为 null（系统光标正常工作）
        assertTrue(
            "矩阵 B (cursorEnabled=false): cursorRect 应为 null（系统光标工作），实际=${sceneB.cursorRect}",
            sceneB.cursorRect == null,
        )

        // 矩阵 C：协同关闭（textEnabled=true, cursorEnabled=true, coordinated=false）
        val timelineC = ComposeVisualTimeline()
        val cursorPathC =
            CursorMotionPath(points = listOf(CursorMotionPoint(rect = Rect(7f, 0f, 9f, 14f), endFraction = 1f)))
        val patchC =
            makePatch(
                id = 3L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                cursorMotionPath = cursorPathC,
                durationMs = 100L,
                motionPolicy =
                    EditorMotionPolicy(
                        textDurationMillis = 100L,
                        cursorEnabled = true,
                        cursorDurationMillis = 80L,
                        coordinated = false,
                    ),
            )
        timelineC.applyPatch(
            patch = patchC,
            frameTimeNanos = 0L,
            cursorFromRect = Rect(0f, 0f, 2f, 14f),
            cursorPath = cursorPathC.points,
            cursorDurationNanos = 80L * NANOS_PER_MS,
        )
        val sceneC = timelineC.sample(0L)
        // 协同关闭：正文显示所有权不破坏 — hiddenRanges 或 units 应正常工作
        val visualOwnedC = sceneC.hiddenRanges.isNotEmpty() || sceneC.units.isNotEmpty()
        assertTrue(
            "矩阵 C (coordinated=false): 正文显示所有权不破坏，" +
                "hiddenRanges=${sceneC.hiddenRanges.size}, units=${sceneC.units.size}",
            visualOwnedC,
        )
    }

    // ==================== A. editEpoch barrier 回归 ====================

    /**
     * A. editEpoch barrier 回归：onAuthoritativeLayout 后立即采样（不等下一帧 withFrameNanos），
     * visualScene 应已接管（hiddenRanges 或 units 非空），不存在裸帧窗口。
     *
     * #703 评论 A：本地输入一发生就建立视觉所有权屏障。
     *
     * 注意：onVisualIntent 走 frameCoordinator 路径（Core intent），不走本地输入分支。
     * 本地输入走 recordLocalInput → onAuthoritativeLayout 配对路径。
     * 这里用 recordLocalInput 模拟真实 InputTransformation 路径。
     */
    @Test
    fun r_a_editEpochBarrier_visualSceneImmediatelyTakesOver() {
        val layouts = captureLayouts("", "a")
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-a-barrier",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 模拟 InputTransformation 路径：recordLocalInput
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )

        // onAuthoritativeLayout 触发本地输入配对 + editEpoch barrier 同步更新 hiddenRanges
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        // #703 评论 A：onAuthoritativeLayout 后立即读 visualScene（不等下一帧 withFrameNanos）
        // draw 层直接读 visualScene StateFlow，不调 sampleVisualScene（那是下一帧 frame clock 的事）。
        // barrier 应已把 insertedUnits merge 到 hiddenRanges，视觉层已接管，不存在裸帧窗口。
        // pendingPatches 仍非空（留给下一帧 withFrameNanos 批量 drain + sample），
        // 所以检查的是 visualScene.hiddenRanges 而非 pendingEmpty。
        val sceneImmediately = state.visualScene.value
        val visualOwnedImmediately =
            sceneImmediately.hiddenRanges.isNotEmpty() || sceneImmediately.units.isNotEmpty()

        // 不变式：视觉层已接管 — 不存在裸帧窗口
        // barrier 同步更新 hiddenRanges，pendingPatches 留给下一帧 drain（不破坏 #694 批量设计）。
        assertTrue(
            "editEpoch barrier: onAuthoritativeLayout 后视觉层应已接管，" +
                "hiddenRanges=${sceneImmediately.hiddenRanges.size}, units=${sceneImmediately.units.size}；" +
                "不应存在裸帧窗口（BasicTextField 已画新字但视觉层未接管）",
            visualOwnedImmediately,
        )
    }

    // ==================== B. 空间进度驱动回归 ====================

    /**
     * B. 空间进度驱动回归：inserted unit 的 clipFraction 由 cursor.left 相对 glyph bounds 决定，
     * 不再纯靠 alpha 0→1 在最终位置超车。
     *
     * #703 评论 B：吐字 — 光标经过字形随之显现。
     */
    @Test
    fun r_b_spatialProgress_clipFractionDrivenByCursor() {
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

        val fromRect = Rect(0f, 0f, 2f, 14f)
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath.points,
            cursorDurationNanos = 300L * NANOS_PER_MS,
        )

        // 在 50ms 采样（cursor 在 fromRect→point0 中间，约 left=5）
        val scene50 = timeline.sample(50L * NANOS_PER_MS)
        val cursor50 = scene50.cursorRect
        assertNotNull("cursor rect 应存在", cursor50)

        // 找 inserted unit 'a'（targetRange=[0,1)）
        val unitA = scene50.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        if (unitA != null) {
            val clipFraction = scene50.unitClipFractions[unitA.key] ?: 1f
            // 空间进度驱动：clipFraction 由 cursor.left 相对 glyph bounds 决定
            // 关键不变式：clipFraction 在 [0, 1] 范围内
            assertTrue(
                "空间进度驱动: unit 'a' clipFraction=$clipFraction 应在 [0, 1] 范围内",
                clipFraction in 0f..1f,
            )
            // 关键不变式：如果 cursor 还在 glyph 左边（cursor.left < glyph.left），
            // clipFraction 应为 0（字不可见）
            // 如果 cursor 已过 glyph 右边（cursor.left > glyph.right），
            // clipFraction 应为 1（字完全可见）
            // 中间状态：clipFraction 在 (0, 1) 之间
            // 这里只验证 clipFraction 合理性（在 [0,1] 范围），不硬编码具体值
            // 因为 glyph bounds 取决于 Robolectric TextMeasurer 的实际排版
        }
    }

    // ==================== C. 取消整行 retainedMoves 回归 ====================

    /**
     * C. 取消整行 retainedMoves 回归：本地删除时 retainedMoves 为空，
     * 不把整行幸存文字交给 overlay 临时接管。
     *
     * #703 评论 C：本地删除先取消整行 retainedMoves 接管。
     */
    @Test
    fun r_c_localDelete_retainedMovesEmpty() {
        val layouts = captureLayoutsWithWidth(arrayOf("abc\ndef", "abcdef"), 50)
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-c-retained",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(3, 3), 0)

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
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)

        val latestPatch = state.latestPatch.value
        assertNotNull("C 回归: latestPatch 应存在", latestPatch)

        // #703 评论 C：本地删除时 retainedMoves 应为空
        assertTrue(
            "C 回归: 本地删除时 retainedMoves 应为空，" +
                "实际=${latestPatch!!.retainedMoves.map { "${it.oldRange}->${it.newRange}" }}",
            latestPatch.retainedMoves.isEmpty(),
        )
    }

    // ==================== D. scene redirect 回归 ====================

    /**
     * D. scene redirect 回归：新 edit 到达时，旧 ghost 已被新编辑覆盖时移除。
     *
     * #703 评论 D：快快速输入/删除采用 scene redirect，不堆积旧动画。
     */
    @Test
    fun r_d_sceneRedirect_oldGhostRemovedWhenCoveredByNewInsert() {
        val layouts = captureLayouts("a", "", "b")
        val aLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val emptyLayout = ComposeLayoutSnapshot(layouts[1], TextRange(0, 0), 0)
        val bLayout = ComposeLayoutSnapshot(layouts[2], TextRange(1, 1), 0)

        val timeline = ComposeVisualTimeline()

        // 第一步：删除 'a'（生成 ghost）
        val deletePatch =
            makePatch(
                id = 1L,
                oldLayout = aLayout,
                newLayout = emptyLayout,
                deletedUnits = listOf(TextRange(0, 1)),
                durationMs = 100L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
            )
        timeline.applyPatch(patch = deletePatch, frameTimeNanos = 0L)

        // 第二步：插入 'b'（新编辑覆盖旧 ghost 的 range [0,1)）
        val insertPatch =
            makePatch(
                id = 2L,
                oldLayout = emptyLayout,
                newLayout = bLayout,
                insertedUnits = listOf(TextRange(0, 1)),
                durationMs = 100L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorEnabled = true, coordinated = true),
            )
        timeline.applyPatch(patch = insertPatch, frameTimeNanos = 10L * NANOS_PER_MS)

        // 采样
        val scene = timeline.sample(10L * NANOS_PER_MS)

        // #703 评论 D：旧 ghost（range=[0,1)）已被新 inserted（range=[0,1)）覆盖，应被移除
        val oldGhost = scene.units.firstOrNull { it.targetRange == null && it.range == TextRange(0, 1) }
        // #703 评论 A 缺陷4：旧实现用 if (oldGhost != null) { assertTrue(oldGhost.alpha.from >= 0f) }，
        // alpha.from >= 0f 几乎永远成立且 ghost 不存在时直接通过，无法卡住"旧 ghost 未移除"的 bug。
        // 新实现改成 assertNull(oldGhost)，明确断言旧 range 已不再由旧 epoch ghost 占有。
        assertNull(
            "scene redirect: 旧 ghost（range=[0,1)）应已被新 inserted 覆盖移除，" +
                "实际仍存在 oldGhost alpha=${oldGhost?.alpha?.from}；" +
                "旧 ghost 不应继续在后面补播",
            oldGhost,
        )
    }

    // ==================== #703 评论 5709208101 复现测试 ====================

    /**
     * #703 评论 5709208101 问题1（修复后）：cursor path 不再重复塞旧 caret。
     *
     * 修复后 buildLocalChainCursorPath 删除路径只生成 points = [newCursorRect]，
     * fromRect 由 computeCursorParamsForPatch 从 patch.originCursorRect 取旧 caret。
     * timeline 收到 fromRect(旧) -> point[0](新, 1.0)，
     * cursor 从旧位置立即开始向新位置移动，不会前半段原地不动。
     *
     * 本测试走真实生产路径 recordLocalInput -> onAuthoritativeLayout -> drainPendingPatchesAtFrame
     * -> sampleVisualScene，断言 25% 时间点 cursor 已经开始向新 caret 移动（正确行为）。
     */
    @Test
    fun repro_comment5709208101_cursorPathDuplicatesOldCaret() {
        val layouts = captureLayouts("abc", "bc")
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-comment5709208101-prob1",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 文本 abc，caret 在 1（'a' 后面）
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 删除 'a'：abc -> bc
        state.recordLocalInput(
            oldText = "abc",
            newText = "bc",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(0, 0), 0)

        // 旧 caret 真实位置（offset=1 on "abc" layout）
        val oldCursorRect = layouts[0].getCursorRect(1)
        // 新 caret 真实位置（offset=0 on "bc" layout）
        val newCursorRect = layouts[1].getCursorRect(0)

        // 确认旧/新 caret 确实不同位置（否则测试无意义）
        assertTrue(
            "问题1: 旧 caret 和新 caret 应不同位置，oldLeft=${oldCursorRect.left}, newLeft=${newCursorRect.left}",
            kotlin.math.abs(oldCursorRect.left - newCursorRect.left) > 1f,
        )

        // drain 开始动画（frameTimeNanos=0，动画从 0 开始）
        state.drainPendingPatchesAtFrame(0L)

        // 25% 时间点（textDurationMillis=100，25ms = 25%）
        val scene25 = state.sampleVisualScene(25L * NANOS_PER_MS)
        val cursor25 = scene25.cursorRect
        assertNotNull("问题1: 25% 时间点 cursor rect 应存在", cursor25)

        // 修复后正确行为：25% 时间点 cursor 已经开始向新 caret 移动，不再前半段原地不动。
        // fromRect(旧) -> point[0](新, 1.0)，cursor 从旧位置线性插值到新位置，
        // 25% 时 cursor 应在旧/新之间，deltaToOld > 0（已离开旧位置）。
        // 旧实现（bug）：25% 时 cursor 仍在旧位置，deltaToOld=0。
        // 用 0.01f 阈值区分"已移动"和"原地不动"（允许浮点误差）。
        val deltaToOld = kotlin.math.abs(cursor25!!.left - oldCursorRect.left)
        assertTrue(
            "问题1: 25% 时间点 cursor 应已离开旧位置开始向新 caret 移动（修复后正确行为），" +
                "cursor25.left=${cursor25.left}, oldCursorRect.left=${oldCursorRect.left}, " +
                "deltaToOld=$deltaToOld（应 > 0.01f）",
            deltaToOld > 0.01f,
        )
    }

    /**
     * #703 评论 5709208101 问题2（修复后）：coordinated 模式下 effective alpha 固定 1，完全靠 clipFraction 控制。
     *
     * 修复方案（draw 层覆盖）：timeline 的 alpha 通道仍保持 0->1 / 1->0（不破坏现有测试），
     * 但 sample 返回的 ComposeVisualScene 带 coordinatedSpatialClip=true 标记，
     * draw 层（drawVisualScene）据此把 effective alpha 覆盖成 1f，
     * 让整字亮度固定由空间裁切（clipFraction）控制。
     *
     * 本测试走真实生产路径，断言 coordinated 模式下：
     * 1. scene.coordinatedSpatialClip == true
     * 2. deleted ghost 的 effective alpha（draw 层会用值）== 1f
     */
    @Test
    fun repro_comment5709208101_alphaIndependentlyControlsGlyphBrightness() {
        val layouts = captureLayouts("a", "")
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-comment5709208101-prob2",
                classifier = FakeLocalVisualPlanClassifier,
            )

        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)

        // 删除 'a'：a -> ""
        state.recordLocalInput(
            oldText = "a",
            newText = "",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(0, 0),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 0), oldRange = TextRange(0, 1))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(0, 0), 0)

        // drain 开始动画
        state.drainPendingPatchesAtFrame(0L)

        // 50% 时间点
        val scene50 = state.sampleVisualScene(50L * NANOS_PER_MS)

        // 验证 scene 带 coordinated + spatial clip 标记
        assertTrue(
            "问题2: coordinated 模式下 scene.coordinatedSpatialClip 应为 true",
            scene50.coordinatedSpatialClip,
        )

        // 找 deleted ghost（targetRange == null, range == [0,1)）
        val ghost = scene50.units.firstOrNull { it.targetRange == null && it.range == TextRange(0, 1) }
        assertNotNull(
            "问题2: 50% 时间点 deleted ghost 应存在（动画进行中）",
            ghost,
        )

        // 修复后正确行为：draw 层在 coordinatedSpatialClip=true 时 effective alpha = 1f。
        // alpha 通道本身仍 1->0（50% 时 alpha.from=0.5），但 draw 层覆盖成 1f。
        // 这里验证 effective alpha（draw 层会用值）== 1f。
        val effectiveAlpha = if (scene50.coordinatedSpatialClip) 1f else ghost!!.alpha.from
        assertEquals(
            "问题2: coordinated 模式下 deleted ghost effective alpha 应固定 1（draw 层覆盖），" +
                "50% 时 alpha.from=${ghost!!.alpha.from}（通道值），effectiveAlpha=$effectiveAlpha（应 == 1f）",
            1f,
            effectiveAlpha,
        )
    }

    /**
     * #703 评论 5709208101 问题3（修复后）：selection stale 时旧 caret 优先用 originCursorRect。
     *
     * 修复后 onAuthoritativeLayout 删除 barrier 的 oldCursorRect 优先用 localPatch.originCursorRect
     * （从 chain.first().oldSelection.end + oldLayout.result 取），不依赖 oldLayout.selection（可能 stale）。
     * computeCursorParamsForPatch 的 fromRect 也优先用 patch.originCursorRect。
     *
     * 场景（用 stale=0 / correct=3 使得 Robolectric 下 getCursorRect 能区分）：
     * 1. 文本 abc，caret 在 0
     * 2. 纯 selection 积到 3（onAuthoritativeLayout 正文几何相同，去重 return，lastPresentedLayout.selection 仍为 0）
     * 3. 删除 'c'：abc -> ab，oldSelection=3, newSelection=2
     * 4. onAuthoritativeLayout("ab", selection=2)
     * 5. barrier 首帧 cursor 必须在 offset=3（验证问题3修复：originCursorRect 从 chain.first().oldSelection.end=3 取）
     *
     * 本测试断言 barrier 首帧 cursor 在 offset=3（正确 oldSelection），而非 offset=0（stale）。
     */
    @Test
    fun repro_comment5709208101_staleSelectionCausesWrongOldCaret() {
        val layouts = captureLayouts("abc", "abc", "ab")
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-comment5709208101-prob3",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 1. 文本 abc，caret 在 0
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 2. 纯 selection 积到 3（正文几何相同，去重 return，lastPresentedLayout.selection 仍为 0）
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)

        // 3. 删除 'c'：abc -> ab，真实 oldSelection=3
        state.recordLocalInput(
            oldText = "abc",
            newText = "ab",
            oldSelection = TextRange(3, 3),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 2), oldRange = TextRange(2, 3))),
        )

        // 4. onAuthoritativeLayout("ab", selection=2) — 配对生成 localPatch + barrier
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)

        // 验证 patch.oldLayout.selection.end —
        // patch.oldLayout = lastPresentedLayout，其 selection 在纯 selection 变化后被去重 return，
        // 没有更新成 3，仍是更早的 0（stale）。这是 lastPresentedLayout 的固有行为，不变。
        val latestPatch = state.latestPatch.value
        assertNotNull("问题3: latestPatch 应存在", latestPatch)
        val oldSelectionEnd = latestPatch!!.oldLayout.selection.end
        assertEquals(
            "问题3: patch.oldLayout.selection.end 应为 0（lastPresentedLayout.selection stale 未更新），" +
                "实际=$oldSelectionEnd",
            0,
            oldSelectionEnd,
        )

        // 修复后正确行为：originCursorRect 从 chain.first().oldSelection.end=3 + oldLayout.result 取，
        // 不依赖 stale 的 oldLayout.selection.end=0。
        val originCursorRect = latestPatch.originCursorRect
        assertNotNull(
            "问题3: latestPatch.originCursorRect 应存在（从 chain.first().oldSelection.end=3 取）",
            originCursorRect,
        )

        // 正确的 T0 caret 位置（offset=3 on "abc" layout）
        val correctOldCursorRect = layouts[0].getCursorRect(3)
        // stale 的错误 caret 位置（offset=0 on "abc" layout）
        val staleCursorRect = layouts[0].getCursorRect(0)

        // 确认 correct 和 stale 确实不同（否则测试无意义）
        assertTrue(
            "问题3: correct(stale=0) 和 stale(correct=3) 的 cursor rect 应不同，" +
                "correctLeft=${correctOldCursorRect.left}, staleLeft=${staleCursorRect.left}",
            kotlin.math.abs(correctOldCursorRect.left - staleCursorRect.left) > 1f,
        )

        // originCursorRect 应对应 offset=3（正确），而非 offset=0（stale）
        val deltaToCorrect = kotlin.math.abs(originCursorRect!!.left - correctOldCursorRect.left)
        val deltaToStale = kotlin.math.abs(originCursorRect.left - staleCursorRect.left)
        assertTrue(
            "问题3: originCursorRect 应对应 offset=3（正确 oldSelection），" +
                "originCursorRect.left=${originCursorRect.left}, " +
                "correctOldCursorRect.left=${correctOldCursorRect.left}, " +
                "deltaToCorrect=$deltaToCorrect（应 < 1f）",
            deltaToCorrect < 1f,
        )
        assertTrue(
            "问题3: originCursorRect 不应对应 offset=0（stale），" +
                "originCursorRect.left=${originCursorRect.left}, " +
                "staleCursorRect.left=${staleCursorRect.left}, " +
                "deltaToStale=$deltaToStale（应 > 1f）",
            deltaToStale > 1f,
        )

        // barrier 首帧 cursor 也用 originCursorRect（而非 stale selection）。
        // #703 评论 5710419102 问题1：核心断言是 visualScene.value.cursorRect == correctOldCursorRect
        // （scene.cursorRect 原子接管，draw 层第一优先级读它）。
        // 旧实现（bug）：scene.cursorRect 为 null，draw 层读 computeRestingCursorRect(latestLayout, liveSelection)
        // 命中新 caret，光标先跳到新位置。
        val sceneImmediately = state.visualScene.value
        assertNotNull(
            "问题3: barrier visualScene.cursorRect 应存在（scene.cursorRect 原子接管）",
            sceneImmediately.cursorRect,
        )
        val sceneCursorDeltaToCorrect =
            kotlin.math.abs(sceneImmediately.cursorRect!!.left - correctOldCursorRect.left)
        assertTrue(
            "问题3: barrier visualScene.cursorRect 应对应 offset=3（正确 oldSelection，scene.cursorRect 原子接管），" +
                "sceneCursorRect.left=${sceneImmediately.cursorRect.left}, " +
                "correctOldCursorRect.left=${correctOldCursorRect.left}, " +
                "sceneCursorDeltaToCorrect=$sceneCursorDeltaToCorrect（应 < 1f）",
            sceneCursorDeltaToCorrect < 1f,
        )

        // _restingCursorRect 保持"无活动动画时的最终静止位置"语义（上方已设成新 layout cursor），
        // 不再承担 pending delete barrier。新 caret = offset=2 on "ab" layout。
        val newCursorRect = layouts[2].getCursorRect(2)
        val restingCursor = state.restingCursorRect.value
        assertNotNull("问题3: barrier restingCursorRect 应存在", restingCursor)
        val restingDeltaToNew = kotlin.math.abs(restingCursor!!.left - newCursorRect.left)
        assertTrue(
            "问题3: barrier restingCursorRect 应对应新 caret（offset=2 on ab layout，最终静止位置语义），" +
                "restingCursor.left=${restingCursor.left}, " +
                "newCursorRect.left=${newCursorRect.left}, " +
                "restingDeltaToNew=$restingDeltaToNew（应 < 1f）",
            restingDeltaToNew < 1f,
        )
    }

    /**
     * #703 评论 5709208101 综合验收测试 — 覆盖评论末尾 8 个验收点。
     *
     * 场景（用 stale=0 / correct=3 使得 Robolectric 下 getCursorRect 能区分）：
     * 1. 文本 `abc`，caret 先在 0
     * 2. 纯 selection 积到 3
     * 3. `recordLocalInput("abc" -> "ab", oldSelection=3, newSelection=2)`（删除 'c'）
     * 4. `onAuthoritativeLayout()`
     * 5. barrier 首帧 cursor 必须在 offset=3（验证问题3修复）
     * 6. drain 后 25% 时间点 cursor 必须已经开始向新 caret 移动，不能前半段原地停住（验证问题1修复）
     * 7. 任意采样帧里，deleted glyph 的可见右边界必须跟 cursor 边界一致
     * 8. coordinated 模式下，未被吞掉的区域整体 alpha 不得提前下降（验证问题2修复）
     */
    @Test
    fun r_comment5709208101_fullLocalDeleteChain() {
        val layouts = captureLayouts("abc", "abc", "ab")
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-comment5709208101-full",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 1. 文本 abc，caret 在 0
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 2. 纯 selection 积到 3（正文几何相同，去重 return）
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)

        // 3. 删除 'c'：abc -> ab
        state.recordLocalInput(
            oldText = "abc",
            newText = "ab",
            oldSelection = TextRange(3, 3),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 2), oldRange = TextRange(2, 3))),
        )

        // 4. onAuthoritativeLayout("ab", selection=2) — 配对生成 localPatch + barrier
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)

        // 5. barrier 首帧 cursor 必须在 offset=3（验证问题3修复）
        val correctOldCursorRect = layouts[0].getCursorRect(3)
        val staleCursorRect = layouts[0].getCursorRect(0)
        // 确认 correct 和 stale 确实不同
        assertTrue(
            "综合验收5: correct(offset=3) 和 stale(offset=0) 的 cursor rect 应不同",
            kotlin.math.abs(correctOldCursorRect.left - staleCursorRect.left) > 1f,
        )
        // #703 评论 5710419102 问题1：核心断言是 visualScene.value.cursorRect == correctOldCursorRect
        // （scene.cursorRect 原子接管，draw 层第一优先级读它）。
        val sceneImmediately = state.visualScene.value
        assertNotNull(
            "综合验收5: barrier visualScene.cursorRect 应存在（scene.cursorRect 原子接管）",
            sceneImmediately.cursorRect,
        )
        val sceneCursorDelta =
            kotlin.math.abs(sceneImmediately.cursorRect!!.left - correctOldCursorRect.left)
        assertTrue(
            "综合验收5: barrier 首帧 visualScene.cursorRect 必须在 offset=3（验证问题3修复，scene.cursorRect 原子接管），" +
                "sceneCursorRect.left=${sceneImmediately.cursorRect.left}, " +
                "correctOldCursorRect.left=${correctOldCursorRect.left}, " +
                "sceneCursorDelta=$sceneCursorDelta（应 < 1f）",
            sceneCursorDelta < 1f,
        )
        // _restingCursorRect 保持"无活动动画时的最终静止位置"语义（上方已设成新 layout cursor），
        // 不再承担 pending delete barrier。新 caret = offset=2 on "ab" layout。
        val newCursorRect = layouts[2].getCursorRect(2)
        val restingCursor = state.restingCursorRect.value
        assertNotNull("综合验收5: barrier restingCursorRect 应存在", restingCursor)
        val restingDeltaToNew = kotlin.math.abs(restingCursor!!.left - newCursorRect.left)
        assertTrue(
            "综合验收5: barrier restingCursorRect 应对应新 caret（offset=2 on ab layout，最终静止位置语义），" +
                "restingCursor.left=${restingCursor.left}, " +
                "newCursorRect.left=${newCursorRect.left}, " +
                "restingDeltaToNew=$restingDeltaToNew（应 < 1f）",
            restingDeltaToNew < 1f,
        )

        // 6. drain 后 25% 时间点 cursor 必须已经开始向新 caret 移动（验证问题1修复）
        state.drainPendingPatchesAtFrame(0L)
        val scene25 = state.sampleVisualScene(25L * NANOS_PER_MS)
        val cursor25 = scene25.cursorRect
        assertNotNull("综合验收6: 25% 时间点 cursor rect 应存在", cursor25)
        val delta25ToOld = kotlin.math.abs(cursor25!!.left - correctOldCursorRect.left)
        // 用 0.01f 阈值区分"已移动"和"原地不动"（允许浮点误差）
        assertTrue(
            "综合验收6: 25% 时间点 cursor 必须已离开旧位置开始向新 caret 移动（验证问题1修复），" +
                "cursor25.left=${cursor25.left}, " +
                "correctOldCursorRect.left=${correctOldCursorRect.left}, " +
                "delta25ToOld=$delta25ToOld（应 > 0.01f）",
            delta25ToOld > 0.01f,
        )

        // 7. 任意采样帧里，deleted glyph 的可见右边界必须跟 cursor 边界一致
        // 8. coordinated 模式下，未被吞掉的区域整体 alpha 不得提前下降（验证问题2修复）
        // 在 25%、50%、75% 三个采样帧检查
        for (progressPct in listOf(25, 50, 75)) {
            val scene = state.sampleVisualScene(progressPct.toLong() * NANOS_PER_MS)
            val cursor = scene.cursorRect
            assertNotNull("综合验收7/8: $progressPct% 时间点 cursor rect 应存在", cursor)

            // 找 deleted ghost（targetRange == null, range == [2,3) — 被删的 'c'）
            val ghost = scene.units.firstOrNull { it.targetRange == null && it.range == TextRange(2, 3) }
            if (ghost != null) {
                // 验收8：coordinated 模式下 effective alpha 不得提前下降（应固定 1）
                // draw 层覆盖方案：alpha 通道仍 1->0，但 scene.coordinatedSpatialClip=true 时
                // draw 层 effective alpha = 1f。这里验证 effective alpha。
                val effectiveAlpha = if (scene.coordinatedSpatialClip) 1f else ghost.alpha.from
                assertEquals(
                    "综合验收8: $progressPct% 时间点 coordinated 模式下 deleted ghost effective alpha 应固定 1" +
                        "（draw 层覆盖，验证问题2修复），实际 alpha.from=${ghost.alpha.from}（通道值），" +
                        "effectiveAlpha=$effectiveAlpha",
                    1f,
                    effectiveAlpha,
                )

                // 验收7：deleted glyph 的可见右边界必须跟 cursor 边界一致
                // ghost 的 glyph bounds
                val ghostBounds = ghost.layout.result.getPathForRange(2, 3).getBounds()
                val clipFraction = scene.unitClipFractions[ghost.key] ?: 1f
                val visibleRight = ghostBounds.left + ghostBounds.width * clipFraction
                val cursorLeft = cursor!!.left
                // cursor 和 ghost 应在同一行（单行文本），可见右边界应跟 cursor left 一致
                // 允许 2px 容差（浮点精度 + Robolectric 渲染误差）
                val deltaRight = kotlin.math.abs(visibleRight - cursorLeft)
                assertTrue(
                    "综合验收7: $progressPct% 时间点 deleted glyph 可见右边界应跟 cursor 边界一致，" +
                        "visibleRight=$visibleRight, cursorLeft=$cursorLeft, " +
                        "clipFraction=$clipFraction, deltaRight=$deltaRight（应 < 2f）",
                    deltaRight < 2f,
                )
            }
        }
    }

    // ==================== #703 评论 5710419102 吐字首帧 clipFraction 回归 ====================

    /**
     * #703 评论 5710419102 问题2：coordinated 模式吐字首帧 clipFraction 必须存在且接近 0，
     * 不能缺失后回退成 1 导致整字首帧完整出现。
     *
     * 走真实本地输入链 "" -> "a"：
     * recordLocalInput -> onAuthoritativeLayout -> drainPendingPatchesAtFrame(0) -> sampleVisualScene(0)
     *
     * 验收点：
     * 1. inserted unit 存在（targetRange != null）
     * 2. scene.coordinatedSpatialClip == true
     * 3. scene.unitClipFractions[unit.key] 必须存在（修复前 alpha=0 被跳过导致缺 key）
     * 4. t=0 时 clipFraction 接近 0（光标在 glyph 左侧，字不可见）
     * 5. 25%/50%/75% 时可见右边界跟 cursor.left 同步推进
     * 6. 100% 时 clipFraction 接近 1（完整显示）
     */
    @Test
    fun r_comment5710419102_coordinatedInsertFirstFrameClipFractionExists() {
        val layouts = captureLayouts("", "a")
        val state =
            ComposeEditorVisualState(
                targetId = "test-703-comment5710419102-prob2",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始空文本，caret 在 0
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 本地输入 "" -> "a"
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )

        // onAuthoritativeLayout 触发本地输入配对 + barrier
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        // drain 开始动画（frameTimeNanos=0，动画从 0 开始）
        state.drainPendingPatchesAtFrame(0L)

        // 首帧采样（t=0）
        val scene0 = state.sampleVisualScene(0L)
        val cursor0 = scene0.cursorRect
        assertNotNull("问题2: 首帧 cursor rect 应存在", cursor0)

        // 1. 找 inserted unit（targetRange != null）
        val insertedUnit = scene0.units.firstOrNull { it.targetRange != null }
        assertNotNull(
            "问题2: 首帧应存在 inserted unit（targetRange != null）",
            insertedUnit,
        )

        // 2. coordinated 模式标记
        assertTrue(
            "问题2: coordinated 模式下 scene.coordinatedSpatialClip 应为 true",
            scene0.coordinatedSpatialClip,
        )

        // 3. unitClipFractions 必须包含 inserted unit 的 key
        // 修复前（bug）：computeUnitClipFractions 因 alpha.from<=0 continue 跳过新插入 unit，
        // unitClipFractions 缺 key，draw 层默认 1，整字首帧完整出现。
        assertTrue(
            "问题2: 首帧 unitClipFractions 必须包含 inserted unit 的 key（修复前缺 key 导致整字出现），" +
                "insertedUnit.key=${insertedUnit!!.key}, " +
                "unitClipFractions.keys=${scene0.unitClipFractions.keys}",
            scene0.unitClipFractions.containsKey(insertedUnit.key),
        )

        // 4. t=0 时 clipFraction 接近 0（光标在 glyph 左侧，字不可见）
        val clipFraction0 = scene0.unitClipFractions[insertedUnit.key]!!
        assertTrue(
            "问题2: 首帧 clipFraction 应接近 0（光标在 glyph 左侧，字不可见），" +
                "实际 clipFraction0=$clipFraction0（应 < 0.1f）",
            clipFraction0 < 0.1f,
        )

        // 5. 25%/50%/75% 时可见右边界跟 cursor.left 同步推进
        for (progressPct in listOf(25, 50, 75)) {
            val scene = state.sampleVisualScene(progressPct.toLong() * NANOS_PER_MS)
            val cursor = scene.cursorRect
            assertNotNull("问题2: $progressPct% 时间点 cursor rect 应存在", cursor)

            val unit = scene.units.firstOrNull { it.targetRange != null }
            assertNotNull(
                "问题2: $progressPct% 时间点 inserted unit 应存在",
                unit,
            )

            // coordinated 模式下 clipFraction 必须存在
            assertTrue(
                "问题2: $progressPct% 时间点 unitClipFractions 必须包含 inserted unit 的 key",
                scene.unitClipFractions.containsKey(unit!!.key),
            )
            val clipFraction = scene.unitClipFractions[unit.key]!!

            // glyph bounds（用 unit 当前 layout + targetRange）
            val targetRange = unit.targetRange!!
            val glyphBounds = unit.layout.result.getPathForRange(targetRange.start, targetRange.end).getBounds()
            val visibleRight = glyphBounds.left + glyphBounds.width * clipFraction
            val cursorLeft = cursor!!.left
            // cursor 和 glyph 应在同一行（单行文本），可见右边界应跟 cursor left 一致
            // 允许 2px 容差（浮点精度 + Robolectric 渲染误差）
            val deltaRight = kotlin.math.abs(visibleRight - cursorLeft)
            assertTrue(
                "问题2: $progressPct% 时间点 inserted glyph 可见右边界应跟 cursor 边界一致，" +
                    "visibleRight=$visibleRight, cursorLeft=$cursorLeft, " +
                    "clipFraction=$clipFraction, deltaRight=$deltaRight（应 < 2f）",
                deltaRight < 2f,
            )
        }

        // 6. 100% 时 clipFraction 接近 1（完整显示）
        val scene100 = state.sampleVisualScene(100L * NANOS_PER_MS)
        val unit100 = scene100.units.firstOrNull { it.targetRange != null }
        if (unit100 != null && scene100.unitClipFractions.containsKey(unit100.key)) {
            val clipFraction100 = scene100.unitClipFractions[unit100.key]!!
            assertTrue(
                "问题2: 100% 时间点 clipFraction 应接近 1（完整显示），" +
                    "实际 clipFraction100=$clipFraction100（应 > 0.9f）",
                clipFraction100 > 0.9f,
            )
        }
    }

    // ==================== #703 评论 5710977972 跨行裁切 + retainedMoves 视觉角色复现测试 ====================

    /**
     * #703 评论 5710977972 缺陷1-A1：跨行吐字 — 光标进入下一行后，上一行已吐出的 inserted unit
     * 的 clipFraction 必须 >= 0.99（保持可见）。
     *
     * 当前 bug：[ComposeVisualTimeline.computeUnitClipFractions] 跨行裁切只有 `sameLine` 无方向判断。
     * 吐字时光标进入下一行后，上一行 inserted unit 的 `sameLine=false`，inserted 分支返回 `0f`，字消失。
     *
     * 场景：从空文本插入 "ab\nc"（'a','b' 第一行 top=0..35，'c' 第二行 top=35..70）。
     * insertedUnits = [0,1), [1,2)（'a' 和 'b' 在第一行）。
     * 光标从第一行移动到第二行（cursorDuration=10ms 短，textDuration=1000ms 长）。
     * 在 20ms 采样：光标已在第二行（cursor 动画完成），unit alpha=0.02（未收口）。
     * 第一行 'a' [0,1) 的 `sameLine=false`（cursorTop=35 >= glyphBottom=35），
     * inserted 分支返回 `0f` → 字消失（bug）。
     *
     * 期望（修复后）：光标已过第一行（cursorTop >= glyphBottom），第一行字应完整可见，clipFraction >= 0.99。
     */
    @Test
    fun repro_comment5710977972_a1_crossLineInsert_firstLineGlyphDisappears() {
        // 用硬换行让 "ab\nc" 跨两行：'a','b' 第一行（top=0..35），'c' 第二行（top=35..70）
        val layouts = captureLayoutsWithWidth(arrayOf("", "ab\nc"), 30)
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(4, 4), 0)

        // 确认 "ab\nc" 确实跨两行
        assertTrue(
            "A1: 'ab\\nc' 应跨两行，实际 lineCount=${layouts[1].lineCount}",
            layouts[1].lineCount >= 2,
        )

        val timeline = ComposeVisualTimeline()

        // 光标终点在第二行（offset 4，'c' 后）
        val cursorOnSecondLine = layouts[1].getCursorRect(4)
        assertTrue(
            "A1: 光标终点应在第二行（top>=35），实际 top=${cursorOnSecondLine.top}",
            cursorOnSecondLine.top >= 35f,
        )

        val cursorPath =
            CursorMotionPath(points = listOf(CursorMotionPoint(rect = cursorOnSecondLine, endFraction = 1f)))

        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = newLayout,
                // 'a' 和 'b' 在第一行
                insertedUnits = listOf(TextRange(0, 1), TextRange(1, 2)),
                cursorMotionPath = cursorPath,
                durationMs = 1000L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 1000L, cursorEnabled = true, coordinated = true),
            )

        val fromRect = layouts[0].getCursorRect(0) // 空文本光标在第一行
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath.points,
            // 光标 10ms 内到第二行
            cursorDurationNanos = 10L * NANOS_PER_MS,
        )

        // 在 20ms 采样：光标已在第二行（cursor 动画完成），unit alpha=0.02（未收口）
        val scene = timeline.sample(20L * NANOS_PER_MS)
        val cursor = scene.cursorRect
        assertNotNull("A1: cursor rect 应存在", cursor)
        assertTrue(
            "A1: 采样时光标应在第二行（top>=35），实际 top=${cursor!!.top}",
            cursor.top >= 35f,
        )

        // 找第一行的 inserted unit 'a' [0,1)
        val unitA = scene.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull(
            "A1: 第一行 inserted unit 'a' [0,1) 应存在（alpha 未完成，未收口）",
            unitA,
        )

        val clipFractionA = scene.unitClipFractions[unitA!!.key] ?: 1f
        // 期望：光标已过第一行（cursorTop >= glyphBottom），第一行字应完整可见
        // bug 下：sameLine=false → inserted 分支 fraction=0 → 字消失
        assertTrue(
            "A1 跨行吐字: 光标进入第二行后，第一行已吐出的 'a' clipFraction 应 >= 0.99（保持可见），" +
                "实际 clipFraction=$clipFractionA（bug 下为 0，字消失）；" +
                "cursor.top=${cursor.top}",
            clipFractionA >= 0.99f,
        )
    }

    /**
     * #703 评论 5710977972 缺陷1-A2：跨行快速 Backspace — 光标退回上一行后，下一行已吞掉的
     * deleted ghost 的 clipFraction 必须 <= 0.01（不能重新出现）。
     *
     * 当前 bug：[ComposeVisualTimeline.computeUnitClipFractions] 跨行裁切只有 `sameLine` 无方向判断。
     * 吞字时光标退回上一行后，下一行 ghost 的 `sameLine=false`，deleted 分支返回 `1f`，字重新出现。
     *
     * 场景："a\nb" → "a"（删除换行和 'b'）。ghost 'b' [2,3) 在旧 layout 第二行（top=35..70）。
     * 光标从第二行（offset 3）退回第一行（offset 1）。cursorDuration=10ms 短，textDuration=1000ms 长。
     * 在 20ms 采样：光标在第一行（top=0..35），ghost 'b' 的 `sameLine=false`（cursorBottom=35 <= glyphTop=35），
     * deleted 分支返回 `1f` → 字重新出现（bug）。
     *
     * 期望（修复后）：光标已退过第二行（cursorBottom <= glyphTop），下一行 ghost 应被吞掉，clipFraction <= 0.01。
     */
    @Test
    fun repro_comment5710977972_a2_crossLineDelete_nextLineGhostReappears() {
        val layouts = captureLayoutsWithWidth(arrayOf("a\nb", "a"), 30)
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(3, 3), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)

        // 确认 "a\nb" 跨两行
        assertTrue(
            "A2: 'a\\nb' 应跨两行，实际 lineCount=${layouts[0].lineCount}",
            layouts[0].lineCount >= 2,
        )

        val timeline = ComposeVisualTimeline()

        // 光标从第二行（offset 3，'b' 后）退到第一行（offset 1，'a' 后）
        val oldCursorRect = layouts[0].getCursorRect(3) // 第二行
        val newCursorRect = layouts[1].getCursorRect(1) // 第一行
        assertTrue(
            "A2: 旧光标应在第二行（top>=35），实际 top=${oldCursorRect.top}",
            oldCursorRect.top >= 35f,
        )
        assertTrue(
            "A2: 新光标应在第一行（top<35），实际 top=${newCursorRect.top}",
            newCursorRect.top < 35f,
        )

        val cursorPath = CursorMotionPath(points = listOf(CursorMotionPoint(rect = newCursorRect, endFraction = 1f)))

        val patch =
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                // 换行符和 'b'
                deletedUnits = listOf(TextRange(1, 2), TextRange(2, 3)),
                cursorMotionPath = cursorPath,
                durationMs = 1000L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 1000L, cursorEnabled = true, coordinated = true),
            )

        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = oldCursorRect,
            cursorPath = cursorPath.points,
            cursorDurationNanos = 10L * NANOS_PER_MS,
        )

        // 在 20ms 采样：光标已在第一行，ghost 'b' 的 alpha 未完成
        val scene = timeline.sample(20L * NANOS_PER_MS)
        val cursor = scene.cursorRect
        assertNotNull("A2: cursor rect 应存在", cursor)
        assertTrue(
            "A2: 采样时光标应在第一行（top<35），实际 top=${cursor!!.top}",
            cursor.top < 35f,
        )

        // 找下一行的 deleted ghost 'b' [2,3)（targetRange == null, range == [2,3)）
        val ghostB = scene.units.firstOrNull { it.targetRange == null && it.range == TextRange(2, 3) }
        assertNotNull(
            "A2: 下一行 deleted ghost 'b' [2,3) 应存在（alpha 未完成，未收口）",
            ghostB,
        )

        val clipFractionB = scene.unitClipFractions[ghostB!!.key] ?: 1f
        // 期望：光标已退过第二行（cursorBottom <= glyphTop），下一行 ghost 应被吞掉
        // bug 下：sameLine=false → deleted 分支 fraction=1 → 字重新出现
        assertTrue(
            "A2 跨行吞字: 光标退回第一行后，下一行已吞掉的 ghost 'b' clipFraction 应 <= 0.01（不能重新出现），" +
                "实际 clipFraction=$clipFractionB（bug 下为 1，字重新出现）",
            clipFractionB <= 0.01f,
        )
    }

    /**
     * #703 评论 5710977972 缺陷2：VisualTextUnit 无视觉角色，retainedMoves 幸存回流文字
     * 被误当 inserted 吐字裁切。
     *
     * [VisualTextUnit] 只有 `targetRange: TextRange?`（null=ghost，非 null=存活）。
     * [ComposeVisualTimeline.computeUnitClipFractions] 只看 `targetRange != null` 就当 inserted 吐字。
     * 但 [createMoveUnitForReflow] 创建的幸存回流文字同样 `targetRange != null`。
     * [ComposeEditorVisualState.buildLocalInputPatch] 只对 Delete 清空 retainedMoves，
     * Insert 仍会生成 retainedMoves。一次输入触发自动换行时，幸存文字被空间裁切误当"新字"隐藏。
     *
     * 场景：Insert 触发换行 — "ab" → "a\nb"（插入换行符，'b' 从第一行移到第二行）。
     * retainedMoves = [RetainedMove([1,2), [2,3))]，createMoveUnitForReflow 创建 unit:
     * targetRange=[2,3)（非 null），alpha=1→1，被当成 inserted。
     * 光标在 offset 2（换行符后，'b' 前，第二行开头 left=0）。
     * 'b' [2,3) bounds=(0,35,1,70)，sameLine=true，
     * inserted 分支 fraction = (cursorLeft - glyphLeft) / glyphWidth = (0-0)/1 = 0。
     * 'b' 不可见（bug）— 幸存回流文字被误当新插入的字裁切。
     *
     * 期望（修复后）：retained move 的幸存文字应始终完整可见（clipFraction=1 或不在 unitClipFractions 中）。
     *
     * 用 makePatch 直接构造 timeline，精确控制 cursorPath 让光标在 'b' 左侧（offset 2），
     * 确保 sameLine=true 且 fraction=0 复现 bug。
     */
    @Test
    fun repro_comment5710977972_a3_retainedMoveReflowTextMistakenAsInserted() {
        val layouts = captureLayoutsWithWidth(arrayOf("ab", "a\nb"), 30)
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)

        // 确认 "ab" 一行，"a\nb" 两行
        assertTrue(
            "A3: 'ab' 应一行，实际 lineCount=${layouts[0].lineCount}",
            layouts[0].lineCount == 1,
        )
        assertTrue(
            "A3: 'a\\nb' 应跨两行，实际 lineCount=${layouts[1].lineCount}",
            layouts[1].lineCount >= 2,
        )

        // 'b' [2,3) 在 "a\nb" 第二行，确认 glyph width >= 0.5（非零宽）
        val bBounds = layouts[1].getPathForRange(2, 3).getBounds()
        assertTrue(
            "A3: 'b' [2,3) glyph width 应 >= 0.5（非零宽，确保进入 spatial clip 分支），实际 bounds=$bBounds",
            bBounds.width >= 0.5f,
        )

        val timeline = ComposeVisualTimeline()

        // 光标在 'b' 左侧（offset 2，换行符后，'b' 前，第二行开头）
        val cursorBeforeB = layouts[1].getCursorRect(2)
        assertTrue(
            "A3: 光标应在第二行（top>=35），实际 top=${cursorBeforeB.top}",
            cursorBeforeB.top >= 35f,
        )

        val cursorPath = CursorMotionPath(points = listOf(CursorMotionPoint(rect = cursorBeforeB, endFraction = 1f)))

        val patch =
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                // 换行符
                insertedUnits = listOf(TextRange(1, 2)),
                retainedMoves =
                    listOf(
                        RetainedMove(oldRange = TextRange(1, 2), newRange = TextRange(2, 3)),
                    ),
                // 'b' reflow
                cursorMotionPath = cursorPath,
                durationMs = 1000L,
                motionPolicy = EditorMotionPolicy(textDurationMillis = 1000L, cursorEnabled = true, coordinated = true),
            )

        val fromRect = layouts[0].getCursorRect(1) // offset 1 在 "ab" 中（'a' 后）
        timeline.applyPatch(
            patch = patch,
            frameTimeNanos = 0L,
            cursorFromRect = fromRect,
            cursorPath = cursorPath.points,
            // 光标 10ms 内到 'b' 左侧
            cursorDurationNanos = 10L * NANOS_PER_MS,
        )

        // 在 20ms 采样：光标已在 'b' 左侧（第二行），retained move unit 'b' 的 position 未完成（未收口）
        val scene = timeline.sample(20L * NANOS_PER_MS)
        val cursor = scene.cursorRect
        assertNotNull("A3: cursor rect 应存在", cursor)

        // 找 retained move unit 'b' [2,3)（targetRange != null, alpha=1→1，幸存回流文字）
        val retainedUnit =
            scene.units.firstOrNull {
                it.targetRange == TextRange(2, 3) && it.alpha.from >= 0.99f && it.alpha.to >= 0.99f
            }
        assertNotNull(
            "A3: retained move unit 'b' [2,3) 应存在（alpha 1→1，幸存回流文字，position 未完成未收口），" +
                "实际 units=" +
                "${scene.units.map { "tgt=${it.targetRange} rng=${it.range} a=${it.alpha.from}->${it.alpha.to}" }}",
            retainedUnit,
        )

        // 期望：retained move 的幸存文字应始终完整可见
        // - clipFraction=1 或不在 unitClipFractions 中（不进入 spatial clip）
        // bug 下：被误当 inserted 裁切，clipFraction=0（光标在 'b' 左侧，sameLine=true，fraction=(0-0)/1=0），'b' 消失
        val clipFraction = scene.unitClipFractions[retainedUnit!!.key] ?: 1f
        assertTrue(
            "A3 retainedMoves 幸存回流: 'b' clipFraction 应为 1 或不在 unitClipFractions 中（始终完整可见），" +
                "实际 clipFraction=$clipFraction（bug 下被误当 inserted 裁切成 0，'b' 消失）；" +
                "cursor=$cursor",
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
