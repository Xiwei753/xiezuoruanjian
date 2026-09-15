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
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto
import java.io.File

/**
 * #689 评论 5675270164 复现测试 — 暴露当前 ComposeVisualTimeline/Rebase/Overlay 的 7 个硬问题。
 *
 * 这些测试在修复前全部 FAIL，用于证据驱动的缺陷复现。
 * ComposeVisualTimeline 是纯数据状态机，可直接实例化，不需要设备。
 *
 * 7 个缺陷：
 * 1. 删除已有正文时根本没有 ghost，吞字动画直接消失。
 * 2. 已经在 timeline 里的 unit 被删掉时变成"永远不消失的 ghost"。
 * 3. 完成动画的存活 unit 永远留在 overlay，和 BasicTextField 重画同一份正文。
 * 4. retainedMoves 只会移动"本来就在 timeline 里的字"，普通正文回流时不动画。
 * 5. 一个 active unit 跨过删除洞时整块判死，不按 offset map 切开。
 * 6. ComposeTextAnimationOverlay 混用 System.nanoTime() 和 withFrameNanos 两种时间基准。
 * 7. 旧回归测试被大面积改成 placeholder，覆盖的行为没一起消失。
 */
@Suppress("StringLiteralDuplication", "MaxLineLength", "FunctionNaming")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualTimelineComment5675270164ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 缺陷 1 ====================

    /**
     * 缺陷 1：删除已有正文时根本没有 ghost，吞字动画会直接消失。
     *
     * 场景：章节打开后 timeline 为空（旧字不在 timeline 里），用户第一次 Backspace。
     * BasicTextField 马上显示删除后正文，timeline 不建 ghost 旧字就直接消失，根本没有"吞字"。
     *
     * 期望：删除动画必须能从普通系统正文临时接管旧字，从 patch.oldLayout + deletedUnits
     * 新建 ghost（alpha 1->0）。
     *
     * 当前行为：ComposeVisualTimeline.kt 第 166-167 行，找不到 surviving unit 就跳过，
     * 不建 ghost。测试在修复前 FAIL。
     */
    @Test
    fun defect1_deleteExistingText_producesGhostFromOldLayout() {
        val layouts = captureLayouts("abc", "ab")
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(3, 3), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)

        val timeline = ComposeVisualTimeline()
        // 章节打开后 timeline 为空，用户第一次 Backspace 删除 "c"
        val patch =
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY)),
                deletedUnits = listOf(TextRange(2, 3)),
            )
        timeline.applyPatch(patch, frameTimeNanos = 0L)

        val scene = timeline.sample(0L)
        val ghosts = scene.units.filter { it.targetRange == null }
        assertTrue(
            "缺陷1: 删除已有正文应产生 ghost unit（alpha 1->0），但当前 ghosts.size=${ghosts.size}\n" +
                "场景：章节打开后 timeline 为空，第一次 Backspace 删除 'c'\n" +
                "当前行为：ComposeVisualTimeline.kt L166-167 找不到 surviving unit 就跳过，不建 ghost\n" +
                "期望：从 patch.oldLayout + deletedUnits 新建 ghost（alpha 1->0）",
            ghosts.isNotEmpty(),
        )
        val ghost = ghosts.first()
        assertEquals(
            "缺陷1: ghost alpha 应从 1 开始（旧字本来完全可见），实际 from=${ghost.alpha.from}",
            1f,
            ghost.alpha.from,
            0.001f,
        )
        assertEquals(
            "缺陷1: ghost alpha 应淡向 0（吞字消失），实际 to=${ghost.alpha.to}",
            0f,
            ghost.alpha.to,
            0.001f,
        )
    }

    // ==================== 缺陷 2 ====================

    /**
     * 缺陷 2：已经在 timeline 里的 unit 被删掉时变成"永远不消失的 ghost"。
     *
     * 场景：先插入 "a"（alpha 0->1，duration=100ms），在 30ms（动画未结束，alpha=0.3）
     * 立即删除 "a"。
     *
     * 期望：映射失败要走 toGhost()，alpha 从当前值 0.3 -> 0，ghost alpha 到 0 后从 timeline 删除。
     *
     * 当前行为：ComposeVisualTimeline.kt 第 75-77 行只把 targetRange 设 null，
     * alpha 通道没改（还是 0->1），ghost 继续淡入到 1，永远留在 overlay。
     * 测试在修复前 FAIL。
     */
    @Test
    fun defect2_deleteActiveUnit_ghostFadesFromCurrentAlphaToZero() {
        val layouts = captureLayouts("", "a", "")
        val emptyLayout0 = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val emptyLayout2 = ComposeLayoutSnapshot(layouts[2], TextRange(0, 0), 0)

        val timeline = ComposeVisualTimeline()

        // Step 1: 插入 "a"（alpha 0->1，duration=100ms）
        val insertPatch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout0,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
            )
        timeline.applyPatch(insertPatch, frameTimeNanos = 0L)

        // Step 2: 在 30ms（动画未结束，alpha≈0.3）删除 "a"
        val frameTimeB = 30L * 1_000_000L
        val deletePatch =
            makePatch(
                id = 2L,
                oldLayout = aLayout,
                newLayout = emptyLayout2,
                offsetMap = null,
                deletedUnits = listOf(TextRange(0, 1)),
            )
        timeline.applyPatch(deletePatch, frameTimeNanos = frameTimeB)

        val scene = timeline.sample(frameTimeB)
        val ghosts = scene.units.filter { it.targetRange == null }
        assertTrue(
            "缺陷2: 删除 active unit 应有 ghost",
            ghosts.isNotEmpty(),
        )
        val ghost = ghosts.first()
        // 当前 alpha ≈ 0.3，应从 0.3 淡向 0
        assertTrue(
            "缺陷2: ghost alpha.from 应为当前值（≈0.3），实际 from=${ghost.alpha.from}",
            ghost.alpha.from > 0f && ghost.alpha.from < 1f,
        )
        assertEquals(
            "缺陷2: ghost alpha 应淡向 0（从当前值消失），实际 to=${ghost.alpha.to}\n" +
                "场景：插入 'a'（alpha 0->1）后 30ms 立即删除\n" +
                "当前行为：L75-77 只把 targetRange 设 null，alpha 通道没改（还是 0->1）\n" +
                "ghost 继续淡入到 1，alpha 到 1 后 hasActiveAnimation 认为完成但 units 里没删掉\n" +
                "overlay 继续以 alpha=1 画在旧位置\n" +
                "期望：映射失败走 toGhost()，alpha 从当前值 -> 0",
            0f,
            ghost.alpha.to,
            0.001f,
        )
    }

    // ==================== 缺陷 3 ====================

    /**
     * 缺陷 3：完成动画的存活 unit 永远留在 overlay，和 BasicTextField 重画同一份正文。
     *
     * 场景：插入 "abc"（duration=100ms），在 200ms（动画已结束）sample。
     *
     * 期望：sample 后收口——存活 unit alpha==1 且 position 已到目标 -> 从 timeline 移除
     * 交还 BasicTextField；scene.units 为空，hiddenRanges 为空。
     *
     * 当前行为：ComposeVisualTimeline.kt 第 203-212 行 sample() 只是
     * `units.map { sampleUnit(it, frameTimeNanos) }`，不收口。
     * 动画完成的插入 unit 变成 alpha=1 仍留在 units 里，drawVisualScene 继续画它，
     * BasicTextField 已恢复显示最终正文，overlay 又在同一位置画一遍。
     * 测试在修复前 FAIL。
     */
    @Test
    fun defect3_completedAnimation_unitRemovedFromScene() {
        val layouts = captureLayouts("", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[1], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()

        val patch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = abcLayout,
                insertedUnits = listOf(TextRange(0, 3)),
            )
        timeline.applyPatch(patch, frameTimeNanos = 0L)

        // 在 200ms sample（动画时长 100ms，已结束）
        val frameTimeEnd = 200L * 1_000_000L
        val scene = timeline.sample(frameTimeEnd)

        assertTrue(
            "缺陷3: 动画完成后存活 unit 应从 timeline 移除交还 BasicTextField，" +
                "但当前 scene.units.size=${scene.units.size}\n" +
                "场景：插入 'abc'（duration=100ms），200ms 时动画已结束\n" +
                "当前行为：sample() L203-212 不收口，存活 unit alpha==1 仍留在 units\n" +
                "drawVisualScene 继续画，BasicTextField 也画同一份正文 -> 重复抗锯齿/字重\n" +
                "期望：sample 后收口，alpha==1 且 position 已到目标 -> 从 timeline 移除",
            scene.units.isEmpty(),
        )
        assertTrue(
            "缺陷3: 动画完成后 hiddenRanges 应为空，但当前 hiddenRanges=${scene.hiddenRanges}",
            scene.hiddenRanges.isEmpty(),
        )
    }

    // ==================== 缺陷 4 ====================

    /**
     * 缺陷 4：retainedMoves 只会移动"本来就在 timeline 里的字"，普通正文回流时不动画。
     *
     * 场景：章节打开后 timeline 为空，删换行 "ab\ncd" → "abcd"，"cd" 回流。
     * patch.retainedMoves 能算出回流，但这些字不在 surviving 全部 continue，回流动画根本不画。
     *
     * 期望：retainedMoves 不能依赖 unit 事先存在。匹配不到 active unit 时从
     * patch.oldLayout + move.oldRange 创建 move unit（alpha 1->1，position old->new），
     * 把 newRange 放进 hiddenRanges 直到 position 动画结束。
     *
     * 当前行为：ComposeVisualTimeline.kt 第 175-176 行
     * `val idx = surviving.indexOfFirst { it.targetRange == newRange }; if (idx < 0) continue`，
     * surviving 为空全部 continue。测试在修复前 FAIL。
     */
    @Test
    fun defect4_retainedMoves_createsMoveUnitForReflowText() {
        val layouts = captureLayouts("ab\ncd", "abcd")
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(3, 3), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)

        val timeline = ComposeVisualTimeline()
        // 章节打开后 timeline 为空，删换行导致 "cd" 回流
        val patch =
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap =
                    listOf(
                        // "ab\n"
                        VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY),
                        // "cd" 上移
                        VisualOffsetMapEntry(3, 2, 2, VisualOffsetMapKind.SHIFTED),
                    ),
                // 删除 "\n"
                deletedUnits = listOf(TextRange(2, 3)),
                retainedMoves = listOf(RetainedMove(oldRange = TextRange(3, 5), newRange = TextRange(2, 4))),
            )
        timeline.applyPatch(patch, frameTimeNanos = 0L)

        val scene = timeline.sample(0L)
        val moveUnit = scene.units.firstOrNull { it.targetRange == TextRange(2, 4) }
        assertNotNull(
            "缺陷4: retainedMoves 应为回流文字创建 move unit（newRange=[2,4)），" +
                "但当前没有\n" +
                "场景：章节打开后 timeline 为空，删换行 'ab\\ncd' → 'abcd'，'cd' 回流\n" +
                "当前行为：L175-176 surviving.indexOfFirst 找不到就 continue，回流动画不画\n" +
                "期望：匹配不到 active unit 时从 oldLayout + move.oldRange 创建 move unit",
            moveUnit,
        )
        assertTrue(
            "缺陷4: 回流的 newRange [2,4) 应在 hiddenRanges 中，但当前 hiddenRanges=${scene.hiddenRanges}",
            scene.hiddenRanges.contains(TextRange(2, 4)),
        )
    }

    // ==================== 缺陷 5 ====================

    /**
     * 缺陷 5：一个 active unit 跨过删除洞时整块判死，不按 offset map 切开。
     *
     * 场景：timeline 里有 [0,4)="ab\nc"（active unit），删除中间换行 [2,3)。
     * "ab" 和 "c" 实际都还活着，但整个 [0,4) 不可能映成连续新 range。
     *
     * 期望：ComposeVisualRebase 新增 splitMappedRangeForward(range, offsetMap) 返回
     * List<MappedRangeSlice>，把旧 unit 按 offset-map entry 切成存活 slice 和被删除 slice。
     * "ab" 映到 [0,2) 继续原 alpha，"c" 映到 [2,3) 继续原 alpha。
     *
     * 当前行为：ComposeVisualTimeline.kt 第 69-74 行对整个 targetRange 调一次
     * mapRangeForwardThroughOffsetMapPublic，返回 null 就整块变 ghost。
     * ComposeVisualRebase 没有 splitMappedRangeForward 函数。
     * 测试在修复前 FAIL。
     */
    @Test
    fun defect5_activeRangeCrossingDeleteHole_splitIntoSurvivingSlices() {
        val layouts = captureLayouts("", "ab\nc", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val oldLayout = ComposeLayoutSnapshot(layouts[1], TextRange(4, 4), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[2], TextRange(3, 3), 0)

        val timeline = ComposeVisualTimeline()

        // Step 1: 先插入 "ab\nc" 作为 active unit（timeline 里有 [0,4) 的 unit）
        val insertPatch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = oldLayout,
                insertedUnits = listOf(TextRange(0, 4)),
            )
        timeline.applyPatch(insertPatch, frameTimeNanos = 0L)

        // Step 2: 删除中间换行 [2,3)，"ab\nc" → "abc"
        val frameTimeB = 30L * 1_000_000L
        val deletePatch =
            makePatch(
                id = 2L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap =
                    listOf(
                        // "ab"
                        VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                        // "c"
                        VisualOffsetMapEntry(3, 2, 1, VisualOffsetMapKind.SHIFTED),
                    ),
                // 删除 "\n"
                deletedUnits = listOf(TextRange(2, 3)),
            )
        timeline.applyPatch(deletePatch, frameTimeNanos = frameTimeB)

        val scene = timeline.sample(frameTimeB)
        // 期望：[0,4) 应切成 [0,2)="ab"（存活，映到 [0,2)）和 [3,4)="c"（存活，映到 [2,3)）
        val survivingAb = scene.units.firstOrNull { it.targetRange == TextRange(0, 2) }
        val survivingC = scene.units.firstOrNull { it.targetRange == TextRange(2, 3) }
        assertNotNull(
            "缺陷5: 跨过删除洞的 active range 应切成存活 slice [0,2)（ab），但当前没有\n" +
                "场景：timeline 有 [0,4)='ab\\nc'，删除中间换行 [2,3)\n" +
                "当前行为：L69-74 整块调 mapRangeForwardThroughOffsetMapPublic 返回 null -> 整块转 ghost\n" +
                "期望：按 offset-map entry 切成存活 slice",
            survivingAb,
        )
        assertNotNull(
            "缺陷5: 跨过删除洞的 active range 应切成存活 slice [2,3)（c），但当前没有",
            survivingC,
        )
        // 不应整块转 ghost
        val wholeGhost = scene.units.firstOrNull { it.targetRange == null && it.range == TextRange(0, 4) }
        assertNull(
            "缺陷5: 跨过删除洞的 active range 不应整块转 ghost [0,4)，" +
                "会把没删除的上一行一起重新接管/重画，制造抽动\n" +
                "当前存在整块 ghost: ${wholeGhost != null}",
            wholeGhost,
        )
    }

    /**
     * 缺陷 5b：ComposeVisualRebase 应有 splitMappedRangeForward 函数。
     *
     * 当前行为：函数不存在。测试在修复前 FAIL。
     */
    @Test
    fun defect5b_rebaseHasSplitMappedRangeForwardFunction() {
        val hasSplit =
            ComposeVisualRebase::class.java.declaredMethods.any {
                it.name.contains("splitMappedRange") || it.name.contains("splitRangeForward")
            }
        assertTrue(
            "缺陷5b: ComposeVisualRebase 应有 splitMappedRangeForward 函数，但当前不存在\n" +
                "期望：新增 splitMappedRangeForward(range, offsetMap) 返回 List<MappedRangeSlice>\n" +
                "把旧 unit 按 offset-map entry 切成存活 slice 和被删除 slice",
            hasSplit,
        )
    }

    // ==================== 缺陷 6 ====================

    /**
     * 缺陷 6：ComposeTextAnimationOverlay 混用 System.nanoTime() 和 withFrameNanos 两种时间基准。
     *
     * 循环第 125 行 `while (visualState.hasActiveVisuals(System.nanoTime()))` 拿
     * withFrameNanos 创建的 startedAtNanos 再用 System.nanoTime() 判断结束。
     * Compose 官方明确 withFrameNanos 的 frameTimeNanos time base 是 implementation-defined，
     * 不保证等于 System.nanoTime()。
     *
     * 期望：全过程只用 frameTimeNanos，循环写成同一个 frame clock。
     *
     * 当前行为：源码第 125 行有 System.nanoTime()。测试在修复前 FAIL。
     */
    @Test
    fun defect6_overlayUsesFrameClockOnly_notSystemNanoTime() {
        val sourceFile =
            File(
                "src/main/kotlin/com/xiwei/sujian/feature/editor/visual/ComposeTextAnimationOverlay.kt",
            )
        assertTrue(
            "ComposeTextAnimationOverlay.kt 源文件应存在: ${sourceFile.absolutePath}",
            sourceFile.exists(),
        )
        val source = sourceFile.readText()
        // 排除注释中的提及（注释里说"不用 System.nanoTime()"是正确的）
        val codeLines =
            source.lines()
                .filter { it.isNotBlank() && !it.trimStart().startsWith("//") && !it.trimStart().startsWith("*") }
                .joinToString("\n")
        assertFalse(
            "缺陷6: ComposeTextAnimationOverlay 不应使用 System.nanoTime()（应只用 withFrameNanos 的 frameTimeNanos）\n" +
                "当前行为：L125 while (visualState.hasActiveVisuals(System.nanoTime()))\n" +
                "Compose 官方明确 withFrameNanos 的 frameTimeNanos time base 是 implementation-defined\n" +
                "不保证等于 System.nanoTime()\n" +
                "期望：全过程只用 frameTimeNanos，循环写成同一个 frame clock",
            codeLines.contains("System.nanoTime()"),
        )
    }

    // ==================== 缺陷 7 ====================

    /**
     * 缺陷 7：旧回归测试被大面积改成 placeholder，覆盖的行为没一起消失。
     *
     * 好几个原来几百行的回归文件被改成十几行 placeholder_oldTransactionMechanismRemoved()。
     * 旧 API 删了但覆盖的行为没一起消失。
     *
     * 期望：至少补这些场景的实质测试（本文件 defect1-defect6 已补），
     * 且旧回归文件不应被改成 placeholder。
     *
     * 当前行为：10 个测试文件被改成 39 行 placeholder。测试在修复前 FAIL。
     */
    @Test
    fun defect7_oldRegressionTestsNotReplacedByPlaceholder() {
        val testDir = File("src/test/kotlin/com/xiwei/sujian/feature/editor/visual")
        assertTrue("测试目录应存在: ${testDir.absolutePath}", testDir.exists())

        val placeholderFiles =
            listOf(
                "ComposeVisualActiveOwnershipReproTest.kt",
                "ComposeVisualCoordinateBugsReproTest.kt",
                "ComposeVisualFinishTransactionGuardReproTest.kt",
                "ComposeVisualLayoutFirstPolicyReproTest.kt",
                "ComposeVisualMasterProgressGuardTest.kt",
                "ComposeVisualOldAnimationUnitsOwnershipReproTest.kt",
                "ComposeVisualPatchAdversarialTest.kt",
                "ComposeVisualStateSuppressionReproTest.kt",
                "ComposeVisualSuppressedRangesReproTest.kt",
                "ComposeEditorVisualStateTest.kt",
            )
        val replacedFiles = mutableListOf<String>()
        for (fileName in placeholderFiles) {
            val file = File(testDir, fileName)
            if (!file.exists()) continue
            val content = file.readText()
            if (content.contains("placeholder_oldTransactionMechanismRemoved")) {
                replacedFiles.add(fileName)
            }
        }
        assertTrue(
            "缺陷7: 以下旧回归测试文件被改成 placeholder，覆盖的行为没一起消失:\n" +
                replacedFiles.joinToString("\n  - ", prefix = "  - ") +
                "\n期望：旧 API 删了但行为测试应重写为实质测试，不是 placeholder\n" +
                "至少补这些场景：已有正文第一次 Backspace -> 1->0 ghost；" +
                "刚插入的字动画未结束立刻删除 -> ghost 从当前 alpha->0；" +
                "删除换行 -> 上一行不产生 move unit 下一行回流文字产生 move unit；" +
                "跨删除洞的 active range -> 切成 surviving + ghost；" +
                "动画结束 -> scene 不再绘制已稳定 unit；" +
                "帧时钟推进 -> 只使用 frameTimeNanos",
            replacedFiles.isEmpty(),
        )
    }

    // ==================== 辅助方法 ====================

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
            cursorMotionPath = null,
            durationMs = durationMs,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            motionPolicy = EditorMotionPolicy(textDurationMillis = durationMs),
        )

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
}
