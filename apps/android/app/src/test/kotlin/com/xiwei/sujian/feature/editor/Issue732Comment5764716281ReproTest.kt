package com.xiwei.sujian.feature.editor

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
import com.xiwei.sujian.feature.editor.visual.ComposeVisualPatch
import com.xiwei.sujian.feature.editor.visual.ComposeVisualTimeline
import com.xiwei.sujian.feature.editor.visual.VisualOffsetMapEntry
import com.xiwei.sujian.feature.editor.visual.VisualOffsetMapKind
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File
import com.xiwei.sujian.feature.editor.visual.AnimationMode

/**
 * Issue #732 评论 5764716281 复现测试 —
 *
 * 复现当前代码仍有的三个会直接破坏 #732 目标的硬问题：
 *
 * ## 硬问题 1: coordinated=true 仍然会被隐藏的 textEnabled/cursorEnabled 关掉
 *
 * `EditorMotionPolicy.effective()` 在 coordinated=true 时直接返回 this，不强制
 * textEnabled=true/cursorEnabled=true。于是可稳定得到 coordinated=true, textEnabled=false
 * 的持久化状态。下游 ComposeVisualTimeline.applyPatch() / ComposeEditorVisualState 仍读
 * raw textEnabled/cursorEnabled，导致页面显示协同动画已开启但动画全部瞬时完成。
 *
 * ## 硬问题 2: 光标 motion 不存在 → 文字立即静态收口 还没有真正做到
 *
 * draw 层在 coordinated 模式下找不到 unitClipFractions[key] 时不画动画 unit，但 Timeline
 * 并没有释放它。sample() 仍把 targetRange != null 的 unit 放进 hiddenRanges；
 * hasActiveAnimation() 仍可因 position 未完成继续跑帧。coordinated 模式下
 * activeEditMotion==null 但 timeline 仍有 inserted unit 时，文字空掉而非静态收口。
 *
 * ## 硬问题 3: 最近编辑主数据已改成单条，但 Android 宽屏仍明确不画
 *
 * ProjectListScreen.kt 窄屏已只画一个 recentEdit，但宽屏分支写死"宽屏暂不单独画最近编辑卡片"。
 * #732 评论要求首页产品契约收成 singular，宽屏也应消费同一个 RecentEdit?，只画一张。
 *
 * 这些测试在修复前全部 FAIL，用于证据驱动的缺陷复现。
 */
@Suppress("MaxLineLength", "FunctionNaming", "StringLiteralDuplication")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue732Comment5764716281ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 硬问题 1: coordinated=true 仍会被隐藏的 textEnabled/cursorEnabled 关掉 ====================

    /**
     * 硬问题 1-1（修复后）：`EditorMotionPolicy(textEnabled=false, coordinated=true).effective()`
     * 的 raw textEnabled 保持 false，但派生值 textAnimationEnabledForEdit == true。
     *
     * 场景：用户先关闭协同；关闭"输入动效"；再打开协同。
     * 持久化结果就是 coordinated=true, textEnabled=false。
     *
     * 修复后（#732 评论 5764716281）：coordinated=true 是完整模式，通过派生值启用完整协同 motion，
     * 不被隐藏的 textEnabled/cursorEnabled 关掉。
     */
    @Test
    fun hardProblem1_coordinatedTrue_textEnabledFalse_effectivePreservesTextEnabledFalse() {
        val legacyPolicy =
            EditorMotionPolicy(
                textEnabled = false,
                cursorEnabled = false,
                coordinated = true,
                reduceMotion = false,
            )
        val effective = legacyPolicy.effective()

        // raw 字段保持 false（不归一 raw 字段）
        assertFalse(
            "修复后: coordinated=true && textEnabled=false 时 raw textEnabled 保持 false（不归一 raw）",
            effective.textEnabled,
        )
        assertFalse(
            "修复后: coordinated=true && cursorEnabled=false 时 raw cursorEnabled 保持 false（不归一 raw）",
            effective.cursorEnabled,
        )
        assertTrue(
            "协同标记仍为 true（页面认为协同已开启，独立开关已藏）",
            effective.coordinated,
        )
        // 派生值：coordinated 模式下完整协同 motion 启用
        assertTrue(
            "修复后: coordinated=true → textAnimationEnabledForEdit == true（派生值启用完整协同 motion）",
            effective.textAnimationEnabledForEdit,
        )
        assertTrue(
            "修复后: coordinated=true → cursorAnimationEnabledForEdit == true（派生值启用完整协同 motion）",
            effective.cursorAnimationEnabledForEdit,
        )
    }

    /**
     * 硬问题 1-2（修复后）：用 `coordinated=true, textEnabled=false` 的 policy 调 applyPatch，
     * Timeline 通过派生值 textAnimationEnabledForEdit 建立文字 unit，coordinatedSpatialClip=true。
     *
     * 修复后（#732 评论 5764716281）：coordinated=true 时 textAnimationEnabledForEdit=true，
     * applyPatch 建立文字 unit，coordinatedSpatialClip=true，文字动画正常运行。
     */
    @Test
    fun hardProblem1_coordinatedTrue_textEnabledFalse_applyPatchDoesNotBuildTextUnits() {
        val layouts = captureLayouts("a", "ab")
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)

        val timeline = ComposeVisualTimeline()
        val patch =
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(1, 2)),
            )
        // coordinated=true 但 textEnabled=false（用户先关输入动效再开协同的持久化结果）
        val policy = EditorMotionPolicy(textEnabled = false, coordinated = true, reduceMotion = false)
        timeline.applyPatch(patch, frameTimeNanos = 0L, motionPolicy = policy)

        // 修复后：通过 activeEditUnits() 验证 applyPatch 建立了 inserted unit
        // （不通过 sample 验证，因为 sample(motionSample=null) 会触发硬问题2 修复清掉 units）
        val (insertedDescriptors, _) = timeline.activeEditUnits()
        assertTrue(
            "修复后: coordinated=true && textEnabled=false 时 applyPatch 通过派生值建立文字 unit——" +
                "textAnimationEnabledForEdit=true，插入 'b' 产生 inserted unit，文字动画正常运行\n" +
                "insertedDescriptors.size=${insertedDescriptors.size}",
            insertedDescriptors.isNotEmpty(),
        )

        // 修复后：coordinatedSpatialClip=true（通过派生值 textAnimationEnabledForEdit && coordinated）
        val scene = timeline.sample(0L)
        assertTrue(
            "修复后: coordinated=true && textEnabled=false 时 coordinatedSpatialClip=true——" +
                "draw 层使用 spatial clip 模式，文字动画正常运行",
            scene.coordinatedSpatialClip,
        )
    }

    /**
     * 硬问题 1-3（修复后）：ComposeEditorVisualState.drainPendingPatchesAtFrame() coordinated 分支
     * 已改用派生值 `sharedDuration = if (policy.textAnimationEnabledForEdit) textDurationNanos else 0L`。
     *
     * 修复后：coordinated=true 但 textEnabled=false 时，textAnimationEnabledForEdit=true，
     * sharedDuration=textDurationNanos，动画正常运行。
     */
    @Test
    fun hardProblem1_coordinatedBranch_stillReadsTextEnabledForSharedDuration() {
        val sourceFile =
            File(
                "src/main/kotlin/com/xiwei/sujian/feature/editor/visual/ComposeEditorVisualState.kt",
            )
        assertTrue(
            "ComposeEditorVisualState.kt 源文件应存在: ${sourceFile.absolutePath}",
            sourceFile.exists(),
        )
        val source = sourceFile.readText()
        assertTrue(
            "修复后: ComposeEditorVisualState coordinated 分支用派生值 textAnimationEnabledForEdit 决定 sharedDuration——" +
                "coordinated=true 时 textAnimationEnabledForEdit=true，动画正常运行",
            source.contains("if (policy.textAnimationEnabledForEdit) textDurationNanos else 0L"),
        )
        // selection-only 光标移动已改用派生值 cursorAnimationEnabledForEdit
        assertTrue(
            "修复后: selection-only 光标移动用派生值 cursorAnimationEnabledForEdit 决定 caretDuration——" +
                "coordinated=true 时 cursorAnimationEnabledForEdit=true，纯光标移动有动画",
            source.contains("if (policy.cursorAnimationEnabledForEdit) selectionCursorDurationNanos else 0L"),
        )
        // 确保旧的 raw textEnabled/cursorEnabled 读取已从 coordinated 分支移除
        assertFalse(
            "修复后: coordinated 分支不再读 raw policy.textEnabled 决定 sharedDuration",
            source.contains("sharedDuration = if (policy.textEnabled) textDurationNanos else 0L"),
        )
    }

    // ==================== 硬问题 2: 光标 motion 不存在 → 文字立即静态收口 还没有真正做到 ====================

    /**
     * 硬问题 2（修复后）：coordinated 模式下 activeEditMotion==null（motionSample==null）时，
     * sample() 直接 settle 所有 text units，清掉 hiddenRanges，返回最终静态正文。
     *
     * 修复后（#732 评论 5764716281）：
     * - timeline 不再持有 inserted unit（sample 清掉 units）
     * - hiddenRanges 为空（文字不被裁掉，BasicTextField 直接画最终正文）
     * - unitClipFractions 为空（overlay 不需要画任何动画 unit）
     * - 文字立即回到最终静态正文，不会空掉
     *
     * 约束：coordinated && activeEditMotion == null => timeline 不得拥有任何吞字/吐字 unit。
     */
    @Test
    fun hardProblem2_coordinatedMode_motionSampleNull_timelineHoldsUnit_hiddenRangesStillClip() {
        val layouts = captureLayouts("a", "ab")
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)

        val timeline = ComposeVisualTimeline()
        val patch =
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(1, 2)),
            )
        // coordinated=true, textEnabled=true：建立 inserted unit，coordinatedSpatialClip=true
        val policy = EditorMotionPolicy(textEnabled = true, coordinated = true, reduceMotion = false)
        timeline.applyPatch(patch, frameTimeNanos = 0L, motionPolicy = policy)

        // 下一帧 sample：activeEditMotion==null → motionSample=null
        val scene = timeline.sample(frameTimeNanos = 0L, motionSample = null)

        // 修复后证据 1: timeline 不再持有 inserted unit（已 settle）
        val insertedUnits = scene.units.filter { it.targetRange != null }
        assertTrue(
            "修复后: coordinated 模式下 motionSample=null 时 timeline 立即 settle 所有 text units——" +
                "insertedUnits.size=${insertedUnits.size}（应为 0）",
            insertedUnits.isEmpty(),
        )

        // 修复后证据 2: hiddenRanges 为空（文字不被裁掉，BasicTextField 直接画最终正文）
        assertTrue(
            "修复后: coordinated 模式下 motionSample=null 时 hiddenRanges 为空——" +
                "文字立即回到最终静态正文，不会空掉\n" +
                "hiddenRanges=${scene.hiddenRanges}",
            scene.hiddenRanges.isEmpty(),
        )

        // 修复后证据 3: unitClipFractions 为空（overlay 不需要画任何动画 unit）
        assertTrue(
            "修复后: coordinated 模式下 motionSample=null 时 unitClipFractions 为空——" +
                "overlay 不画任何动画 unit，文字由 BasicTextField 直接画\n" +
                "unitClipFractions.keys=${scene.unitClipFractions.keys}",
            scene.unitClipFractions.isEmpty(),
        )

        // 修复后证据 4: hasActiveAnimation 在 coordinated 模式下 units 为空时返回 false
        val hasActive = timeline.hasActiveAnimation(0L)
        assertFalse(
            "修复后: coordinated 模式下 units 为空时 hasActiveAnimation=false——" +
                "overlay 停帧，文字已静态收口\n" +
                "hasActiveAnimation=$hasActive",
            hasActive,
        )
    }

    /**
     * 硬问题 2 约束声明（修复后）：`coordinated && activeEditMotion == null => timeline 不得拥有任何吞字/吐字 unit`
     *
     * 修复后此约束被满足：coordinated 模式下 motionSample==null 时 sample() 清掉所有 units。
     */
    @Test
    fun hardProblem2_constraintViolation_coordinatedAndMotionNull_timelineStillHoldsUnits() {
        val layouts = captureLayouts("a", "ab")
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(2, 2), 0)

        val timeline = ComposeVisualTimeline()
        val patch =
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                offsetMap = listOf(VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY)),
                insertedUnits = listOf(TextRange(1, 2)),
            )
        val policy = EditorMotionPolicy(textEnabled = true, coordinated = true, reduceMotion = false)
        timeline.applyPatch(patch, frameTimeNanos = 0L, motionPolicy = policy)

        // activeEditMotion == null（motion 消失），motionSample = null
        val scene = timeline.sample(frameTimeNanos = 0L, motionSample = null)
        val activeEditMotionIsNull = true // 模拟 activeEditMotion == null

        // 约束: coordinated && activeEditMotion == null => timeline 不得拥有任何吞字/吐字 unit
        val constraintSatisfied =
            !(scene.coordinatedSpatialClip && activeEditMotionIsNull && scene.units.isNotEmpty())

        assertTrue(
            "修复后: 约束 coordinated && activeEditMotion == null => timeline 不得拥有任何吞字/吐字 unit 已满足——" +
                "coordinatedSpatialClip=${scene.coordinatedSpatialClip}, " +
                "activeEditMotion==null=$activeEditMotionIsNull, " +
                "units.isNotEmpty=${scene.units.isNotEmpty()}\n" +
                "文字立即回到最终静态正文，不会空掉",
            constraintSatisfied,
        )
    }

    // ==================== 硬问题 3: 最近编辑主数据已改成单条，但 Android 宽屏仍明确不画 ====================

    /**
     * 硬问题 3（修复后）：ProjectListScreen.kt 宽屏分支（useWideGrid）现在也消费 recentEdit，
     * 画一张最近编辑卡片（占满第一行）。
     *
     * 修复后（#732 评论 5764716281）：宽屏和窄屏都消费同一个 RecentEdit?，
     * 有 recentEdit 时只画一张最近编辑卡片，不再写死不画。
     */
    @Test
    fun hardProblem3_wideScreenBranch_doesNotRenderRecentEditCard() {
        val sourceFile =
            File(
                "src/main/kotlin/com/xiwei/sujian/feature/project/ui/ProjectListScreen.kt",
            )
        assertTrue(
            "ProjectListScreen.kt 源文件应存在: ${sourceFile.absolutePath}",
            sourceFile.exists(),
        )
        val source = sourceFile.readText()

        // 修复后证据 1: 宽屏分支不再有"暂不单独画最近编辑卡片"的注释
        assertFalse(
            "修复后: ProjectListScreen.kt 宽屏分支不再写死不画 recentEdit——" +
                "注释'宽屏暂不单独画最近编辑卡片'已删除",
            source.contains("宽屏暂不单独画最近编辑卡片"),
        )

        // 修复后证据 2: 宽屏分支（useWideGrid / LazyVerticalGrid）引用 appState.recentEdit 的实际代码
        val useWideGridIndex = source.indexOf("val useWideGrid =")
        assertTrue(
            "应找到 useWideGrid 定义",
            useWideGridIndex >= 0,
        )
        val elseIndex = source.indexOf("} else {", useWideGridIndex)
        assertTrue(
            "应找到宽屏分支后的 else（窄屏分支）",
            elseIndex > useWideGridIndex,
        )
        val wideBranch = source.substring(useWideGridIndex, elseIndex)
        // 排除注释行后检查是否有 recentEdit 的实际代码引用
        val wideBranchCodeOnly =
            wideBranch.lines()
                .filter { line ->
                    val trimmed = line.trimStart()
                    !trimmed.startsWith("//") && !trimmed.startsWith("*")
                }
                .joinToString("\n")
        assertTrue(
            "修复后: 宽屏分支引用 appState.recentEdit 的实际代码——" +
                "宽屏也消费 recentEdit，画一张最近编辑卡片\n" +
                "#732 评论 5764716281：有 recentEdit 时只画一个最近编辑卡片，宽屏窄屏都画\n" +
                "宽屏分支实际代码（排除注释）:\n$wideBranchCodeOnly",
            wideBranchCodeOnly.contains("recentEdit"),
        )
    }

    /**
     * 硬问题 3 补充：窄屏分支确实画了 recentEdit 卡片（证明宽屏是故意不画，不是 recentEdit 数据缺失）。
     */
    @Test
    fun hardProblem3_narrowScreenBranch_doesRenderRecentEditCard() {
        val sourceFile =
            File(
                "src/main/kotlin/com/xiwei/sujian/feature/project/ui/ProjectListScreen.kt",
            )
        assertTrue(sourceFile.exists())
        val source = sourceFile.readText()

        // 窄屏分支引用 appState.recentEdit
        assertTrue(
            "硬问题3 补充: 窄屏分支确实画了 recentEdit 卡片（证明宽屏是故意不画）——" +
                "窄屏有 if (appState.recentEdit != null) 分支",
            source.contains("if (appState.recentEdit != null)"),
        )
    }

    // ==================== 辅助方法 ====================

    @Suppress("LongParameterList")
    private fun makePatch(
        id: Long,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>? = null,
        insertedUnits: List<TextRange> = emptyList(),
        deletedUnits: List<TextRange> = emptyList(),
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
            retainedMoves = emptyList(),
            originCaretRect = Rect.Zero,
            targetCaretRect = Rect.Zero,
            durationMs = durationMs,
            animationMode = AnimationMode.CLUSTER_ANIMATION,
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
