package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.EditorSoftBreakProjection
import com.xiwei.sujian.feature.editor.layout.boundsForRawRange
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.EditorByteRangeDto
import uniffi.writer_core.LocalVisualPlanDto
import uniffi.writer_core.LocalVisualSliceDto

/**
 * Issue #720 设备侧 instrumented test — 真实软换行生产路径验证。
 *
 * 与 Robolectric 版本（[ComposeVisualIssue720ReflowOwnershipTest]）不同，本测试在真实设备上运行，
 * [rememberTextMeasurer] 做真实字体度量，可以通过窄宽度触发软换行（U+200B 折行），
 * 不需要用 `\n` 模拟软换行效果。
 *
 * 生产路径：
 * 1. [EditorSoftBreakProjection.fromRawText] 生成 projection（计算 U+200B 插入点）
 * 2. [buildDisplayText] 在 display text 中插入 U+200B（ZERO_WIDTH_SPACE）
 * 3. `BasicTextField` 在真实宽度约束下决定折行
 * 4. `onTextLayout` 返回最终 [TextLayoutResult]
 *
 * 测试场景：
 * - T0 = "ab"（一行），先插入 'c' 创建 active unit [2,3)
 * - T1 = "abc"（一行），再在 'a' 前插入 "xyz" → "xyzabc"（'c' 软换行到第二行）
 *
 * 断言：
 * a. rawText 不含 `\n`
 * b. displayText 含 U+200B
 * c. old/new [TextLayoutResult] 的目标 glyph 行号真的发生变化（从第一行变到第二行）
 * d. 把这两份真实 layout + projection 喂给 [ComposeEditorVisualState]
 * e. 在第二笔 `onAuthoritativeLayout()` 后、drain 前，断言 survivor 已不在 `scene.units` 和 `hiddenRanges`
 * f. drain 后再断言 timeline 没把 survivor 接回来
 */
@Suppress(
    "LongMethod",
    "MaxLineLength",
    "LargeClass",
    "LongParameterList",
    "StringLiteralDuplication",
    "TooManyFunctions",
)
@RunWith(AndroidJUnit4::class)
class ComposeVisualIssue720SoftWrapInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L
    }

    /**
     * 真实软换行 — surviving unit 退出 overlay，释放给 BasicTextField。
     *
     * 设备侧测试中 [rememberTextMeasurer] 做真实字体度量，窄宽度（35px）下 "xyzabc" 自然折行，
     * 'c' 从第一行变到第二行。rawText 不含 `\n`，displayText 含 U+200B。
     *
     * 用 [ComposeEditorVisualState] 走真实路径：
     * recordLocalInput → onAuthoritativeLayout → drainPendingPatchesAtFrame → sampleVisualScene。
     */
    @Test
    fun softWrap_realTextMeasurer_survivingUnit_releasedToBasicTextField() {
        // === 1. 用 EditorSoftBreakProjection.fromRawText 生成 projection ===
        val rawTexts = listOf("ab", "abc", "xyzabc")
        val projections = rawTexts.map { EditorSoftBreakProjection.fromRawText(it) }
        val displayTexts = rawTexts.zip(projections).map { (raw, proj) -> buildDisplayText(raw, proj) }

        // === 2. 断言：rawText 不含 \n ===
        for (raw in rawTexts) {
            assertTrue(
                "rawText 不应包含 \\n: \"$raw\"",
                !raw.contains('\n'),
            )
        }

        // === 3. 断言：displayText 含 U+200B ===
        for ((index, displayText) in displayTexts.withIndex()) {
            assertTrue(
                "displayText[$index] 应含 U+200B (ZERO_WIDTH_SPACE)，实际: \"$displayText\"",
                displayText.contains(EditorSoftBreakProjection.ZERO_WIDTH_SPACE),
            )
        }

        // === 4. 用真实 TextMeasurer 测量布局（设备侧真实字体度量） ===
        val layouts = measureAllLayouts(displayTexts, maxWidth = 35, fontSizeSp = 14f)

        // 构建 ComposeLayoutSnapshots
        val snapshots =
            rawTexts.zip(projections).zip(layouts).map { (rawAndProj, layout) ->
                val (raw, proj) = rawAndProj
                ComposeLayoutSnapshot(layout, TextRange(raw.length, raw.length), 0, proj, raw)
            }

        val abSnapshot = snapshots[0]
        val abcSnapshot = snapshots[1]
        val xyzabcSnapshot = snapshots[2]

        // === 5. 断言：T0("abc") 一行，T1("xyzabc") 跨行（真实软换行） ===
        assertTrue(
            "'abc' 应一行，实际 lineCount=${abcSnapshot.result.lineCount}",
            abcSnapshot.result.lineCount == 1,
        )
        assertTrue(
            "'xyzabc' 应跨行（软换行），实际 lineCount=${xyzabcSnapshot.result.lineCount}",
            xyzabcSnapshot.result.lineCount >= 2,
        )

        // === 6. 断言：'c' 在 T0("abc") 第一行，在 T1("xyzabc") 第二行（行号变化） ===
        val cBoundsAbc = abcSnapshot.boundsForRawRange(TextRange(2, 3))
        val cBoundsXyzabc = xyzabcSnapshot.boundsForRawRange(TextRange(5, 6))
        assertNotNull("'c' 在 'abc' 中的 bounds 应非 null", cBoundsAbc)
        assertNotNull("'c' 在 'xyzabc' 中的 bounds 应非 null", cBoundsXyzabc)
        assertTrue(
            "'c' 在 'abc' 中应在第一行（top < 35），实际 top=${cBoundsAbc!!.top}",
            cBoundsAbc.top < 35f,
        )
        assertTrue(
            "'c' 在 'xyzabc' 中应在第二行（top >= 35，软换行），实际 top=${cBoundsXyzabc!!.top}",
            cBoundsXyzabc!!.top >= 35f,
        )

        // === 7. 把真实 layout + projection 喂给 ComposeEditorVisualState ===
        val state =
            ComposeEditorVisualState(
                targetId = "test-720-softwrap-instrumented",
                classifier = FakeLocalVisualPlanClassifier,
            )

        // 初始 layout "ab"
        state.onAuthoritativeLayout(
            abSnapshot.result,
            TextRange(2, 2),
            0,
            projection = abSnapshot.projection,
            rawText = "ab",
        )

        // 第一笔：插入 'c'，"ab" → "abc"（创建 active unit [2,3)）
        state.recordLocalInput(
            oldText = "ab",
            newText = "abc",
            oldSelection = TextRange(2, 2),
            newSelection = TextRange(3, 3),
            changes = listOf(LocalInputChange(newRange = TextRange(2, 3), oldRange = TextRange(2, 2))),
        )
        state.onAuthoritativeLayout(
            abcSnapshot.result,
            TextRange(3, 3),
            0,
            projection = abcSnapshot.projection,
            rawText = "abc",
        )

        // drain patch1（'c' 成为 active unit）
        state.drainPendingPatchesAtFrame(0L)

        // 第二笔：在 'a' 前插入 "xyz"，"abc" → "xyzabc"（'c' 软换行到第二行）
        state.recordLocalInput(
            oldText = "abc",
            newText = "xyzabc",
            oldSelection = TextRange(3, 3),
            newSelection = TextRange(6, 6),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 3), oldRange = TextRange(0, 0))),
        )
        state.onAuthoritativeLayout(
            xyzabcSnapshot.result,
            TextRange(6, 6),
            0,
            projection = xyzabcSnapshot.projection,
            rawText = "xyzabc",
        )

        // === 8. handoff 首帧断言 — drain 前 ===
        // 'c' 在新正文 "xyzabc" 中是 [5,6)，软换行后从第一行变到第二行（自然几何变化），
        // handoff 首帧就应释放给 BasicTextField：
        // - 'c' 的 surviving unit 不在 scene.units
        // - 'c' 对应的 range 不在 scene.hiddenRanges（BasicTextField 首帧不被裁掉）
        val handoffScene = state.drawSnapshot().scene
        val cSurvivingInHandoff = handoffScene.units.firstOrNull { it.targetRange == TextRange(5, 6) }
        assertNull(
            "handoff 首帧：'c' [5,6) surviving unit 应已释放给 BasicTextField，不在 handoff scene.units 里，" +
                "实际 units=${handoffScene.units.map { "tgt=${it.targetRange} rng=${it.range}" }}",
            cSurvivingInHandoff,
        )
        val cHiddenInHandoff = handoffScene.hiddenRanges.any { it.start <= 5 && it.end >= 6 }
        assertTrue(
            "handoff 首帧：'c' [5,6) 不应在 hiddenRanges（BasicTextField 首帧不被裁掉），" +
                "实际 hiddenRanges=${handoffScene.hiddenRanges}",
            !cHiddenInHandoff,
        )

        // === 9. drain 后 timeline 帧断言 — survivor 不会重新出现 ===
        state.drainPendingPatchesAtFrame(20L * NANOS_PER_MS)
        val scene = state.sampleVisualScene(20L * NANOS_PER_MS)
        val cSurviving = scene.units.firstOrNull { it.targetRange == TextRange(5, 6) }
        assertNull(
            "timeline 帧：'c' [5,6) surviving unit 应已释放给 BasicTextField（软换行），" +
                "实际 units=${scene.units.map { "tgt=${it.targetRange} rng=${it.range}" }}",
            cSurviving,
        )
    }

    // ==================== 辅助方法 ====================

    /**
     * 按 [projection.insertPoints] 在 raw text 中插入 U+200B 生成 display text。
     *
     * 与生产路径一致：只在 [EditorSoftBreakProjection.ZERO_WIDTH_SPACE]（U+200B）处插入，
     * 不用 `\n` 模拟软换行。
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
     *
     * 设备侧测试中 [rememberTextMeasurer] 做真实字体度量，
     * 窄宽度下文本自然折行（软换行），不需要用 `\n` 模拟。
     */
    private fun measureAllLayouts(
        texts: List<String>,
        maxWidth: Int,
        fontSizeSp: Float,
    ): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = rememberTextMeasurer()
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

/**
 * 测试用 fake [LocalVisualPlanClassifier] — 设备侧 instrumented test 版本。
 *
 * 与 `src/test` 中的 [FakeLocalVisualPlanClassifier] 相同模式：
 * 用 `java.text.BreakIterator` + ZWJ 合并模拟 Core 的 grapheme cluster 拆分，
 * 用 [chooseAnimationModeFake] 模拟 Core 的 `choose_animation_mode` 规则。
 *
 * androidTest 源集无法访问 `src/test` 源集，所以在此创建副本。
 * 生产环境用 [CoreLocalVisualPlanClassifier] 直接调 Core。
 */
private object FakeLocalVisualPlanClassifier : LocalVisualPlanClassifier {
    override fun classify(
        oldText: String,
        newText: String,
        oldAffectedRanges: List<TextRange>,
        newAffectedRanges: List<TextRange>,
        animationEnabled: Boolean,
    ): LocalVisualPlanDto {
        val oldSlices =
            oldAffectedRanges.mapNotNull { range -> ComposeLocalVisualRebase.buildLocalVisualSliceDto(oldText, range) }
        val newSlices =
            newAffectedRanges.mapNotNull { range -> ComposeLocalVisualRebase.buildLocalVisualSliceDto(newText, range) }
        // 与 composition 分类一致：优先用 newSlices（插入侧），空时回退到 oldSlices（删除侧）。
        val classifySlices = if (newSlices.isNotEmpty()) newSlices else oldSlices
        val clusterCount = classifySlices.sumOf { slice -> splitGraphemeClusters(slice.text).size }
        val containsNewline = classifySlices.any { it.text.contains('\n') }
        val containsComplex =
            classifySlices.any {
                splitGraphemeClusters(it.text).any { cluster -> cluster.length > 1 }
            }
        val animationMode =
            chooseAnimationModeFake(
                clusterCount = clusterCount,
                containsNewline = containsNewline,
                containsComplexGrapheme = containsComplex,
                animationEnabled = animationEnabled,
            )
        val oldUnits = buildAnimationUnitsFromSlices(oldSlices, animationMode)
        val newUnits = buildAnimationUnitsFromSlices(newSlices, animationMode)
        return LocalVisualPlanDto(
            animationMode = animationMode,
            oldAnimationUnits = oldUnits,
            newAnimationUnits = newUnits,
        )
    }

    private fun splitGraphemeClusters(text: String): List<String> =
        ComposeLocalVisualRebase.splitGraphemeClusterRangesWithZwjMerge(text).map { range ->
            text.substring(range.start, range.end)
        }

    /**
     * Kotlin 投影 of `visual_classification.rs::choose_animation_mode`。
     * 与 Core 保持一致：0 cluster -> SystemSuppressed；含换行 -> LineReflowAnimation；
     * 复杂 grapheme -> ClusterAnimation；<= 8 cluster -> GlyphAnimation；> 8 -> RunAnimation。
     */
    private fun chooseAnimationModeFake(
        clusterCount: Int,
        containsNewline: Boolean,
        containsComplexGrapheme: Boolean,
        animationEnabled: Boolean,
    ): AnimationModeDto {
        if (!animationEnabled) return AnimationModeDto.SYSTEM_SUPPRESSED
        if (clusterCount == 0) return AnimationModeDto.SYSTEM_SUPPRESSED
        if (containsNewline) return AnimationModeDto.LINE_REFLOW_ANIMATION
        if (containsComplexGrapheme) return AnimationModeDto.CLUSTER_ANIMATION
        return if (clusterCount <= 8) AnimationModeDto.GLYPH_ANIMATION else AnimationModeDto.RUN_ANIMATION
    }

    /**
     * 按 animationMode 从 [LocalVisualSliceDto] 生成 animation units（UTF-8 byte ranges）。
     */
    private fun buildAnimationUnitsFromSlices(
        slices: List<LocalVisualSliceDto>,
        animationMode: AnimationModeDto,
    ): List<EditorByteRangeDto> =
        when (animationMode) {
            AnimationModeDto.SYSTEM_SUPPRESSED -> emptyList()
            AnimationModeDto.LINE_REFLOW_ANIMATION, AnimationModeDto.SNAPSHOT_ANIMATION -> {
                slices.map { slice ->
                    val utf8End = TextOffsetUtils.utf8OffsetForCharIndex(slice.text, slice.text.length)
                    EditorByteRangeDto(
                        start = slice.absoluteStart,
                        endExclusive = (slice.absoluteStart.toInt() + utf8End).toUInt(),
                    )
                }
            }
            AnimationModeDto.GLYPH_ANIMATION, AnimationModeDto.CLUSTER_ANIMATION -> {
                val result = mutableListOf<EditorByteRangeDto>()
                for (slice in slices) {
                    val clusterRanges = ComposeLocalVisualRebase.splitGraphemeClusterRangesWithZwjMerge(slice.text)
                    for (clusterRange in clusterRanges) {
                        val utf8Start = TextOffsetUtils.utf8OffsetForCharIndex(slice.text, clusterRange.start)
                        val utf8End = TextOffsetUtils.utf8OffsetForCharIndex(slice.text, clusterRange.end)
                        result.add(
                            EditorByteRangeDto(
                                start = (slice.absoluteStart.toInt() + utf8Start).toUInt(),
                                endExclusive = (slice.absoluteStart.toInt() + utf8End).toUInt(),
                            ),
                        )
                    }
                }
                result
            }
            AnimationModeDto.RUN_ANIMATION -> {
                slices.map { slice ->
                    val utf8End = TextOffsetUtils.utf8OffsetForCharIndex(slice.text, slice.text.length)
                    EditorByteRangeDto(
                        start = slice.absoluteStart,
                        endExclusive = (slice.absoluteStart.toInt() + utf8End).toUInt(),
                    )
                }
            }
        }
}
