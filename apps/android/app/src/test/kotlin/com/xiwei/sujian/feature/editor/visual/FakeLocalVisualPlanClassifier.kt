package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.EditorByteRangeDto
import uniffi.writer_core.LocalVisualPlanDto
import uniffi.writer_core.LocalVisualSliceDto

/**
 * #694 评论 5693864609 问题3：测试用 fake [LocalVisualPlanClassifier]。
 *
 * Robolectric 测试环境无法加载 Rust 原生库，不能调 Core `classifyLocalVisualPlan`。
 * 本 fake 用 `java.text.BreakIterator` + ZWJ 合并模拟 Core 的 grapheme cluster 拆分，
 * 用 [chooseAnimationModeFake] 模拟 Core 的 `choose_animation_mode` 规则。
 *
 * 这不是 Core 业务规则的复制（Core 是 Rust 实现，这里是测试投影），
 * 只用于测试环境让 [ComposeEditorVisualState] 的 `buildLocalInputPatch` 能正常走通。
 * 生产环境用 [CoreLocalVisualPlanClassifier] 直接调 Core。
 */
object FakeLocalVisualPlanClassifier : LocalVisualPlanClassifier {
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
