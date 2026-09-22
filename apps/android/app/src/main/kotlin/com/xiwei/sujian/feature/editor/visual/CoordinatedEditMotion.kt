package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * Issue #737：一笔编辑的完整协调运动状态。
 *
 * caret 和吞字/吐字共用同一个 [CaretTraversal]、同一个 progress、同一个生命周期。
 * 要有都有，要没有都没有。
 *
 * - 有光标运动才有文字运动：[traversal] 无效时 [isValid] = false，[glyphChannels] 为空，
 *   直接显示最终静态正文。
 * - 同一笔 motion 里同时产出 caret 和文字采样结果（[sample]）。
 * - 一笔编辑只有一个 motion — 不再分别维护"文字动画是否 active"和"光标动画是否 active"。
 *
 * @param oldSelection 编辑前选区。
 * @param newSelection 编辑后选区。
 * @param oldCaretRect 编辑前 caret rect。
 * @param newCaretRect 编辑后 caret rect。
 * @param oldLine 编辑前 caret 所在行（-1 表示未确定）。
 * @param newLine 编辑后 caret 所在行（-1 表示未确定）。
 * @param traversal 光标遍历路径 — 唯一主运动。
 * @param glyphChannels 文字 glyph 运动通道（traversal 无效时为空）。
 * @param startedAtNanos motion 开始时间戳（Compose frame clock）。
 * @param durationNanos motion 时长（<=0 表示瞬时完成）。
 */
@Suppress("LongParameterList")
class CoordinatedEditMotion(
    val oldSelection: TextRange,
    val newSelection: TextRange,
    val oldCaretRect: Rect,
    val newCaretRect: Rect,
    val oldLine: Int,
    val newLine: Int,
    val traversal: CaretTraversal,
    val glyphChannels: Map<Long, GlyphChannel>,
    private val startedAtNanos: Long,
    private val durationNanos: Long,
) {
    /** 文字 glyph 角色。 */
    enum class GlyphRole { Inserted, Deleted }

    /**
     * 单个 glyph 的运动通道。
     *
     * @param key 唯一标识。
     * @param range UTF-16 range。
     * @param layout 所属 layout 快照。
     * @param role [GlyphRole.Inserted]（吐字 0→1）/ [GlyphRole.Deleted]（吞字 1→0）。
     * @param fromFraction 起始 fraction。
     * @param toFraction 目标 fraction。
     * @param startProgress 在 master progress 中的区间起点。
     * @param endProgress 在 master progress 中的区间终点。
     */
    data class GlyphChannel(
        val key: Long,
        val range: TextRange,
        val layout: ComposeLayoutSnapshot,
        val role: GlyphRole,
        val fromFraction: Float,
        val toFraction: Float,
        val startProgress: Float,
        val endProgress: Float,
    )

    /**
     * 一帧的采样结果 — 同时包含 caret 和文字。
     *
     * @param caretRect 当前帧 caret rect。
     * @param glyphOverlays 当前帧应绘制的 glyph overlay 列表。
     * @param hiddenRanges 当前帧应被动画层接管（裁掉 BasicTextField 原字）的 range 列表。
     * @param finished motion 是否已完成。
     * @param isValid motion 是否有效（traversal 是否建出来）。
     */
    data class Sample(
        val caretRect: Rect,
        val glyphOverlays: List<GlyphOverlay>,
        val hiddenRanges: List<TextRange>,
        val finished: Boolean,
        val isValid: Boolean,
    )

    /**
     * 单个 glyph 的绘制 overlay。
     *
     * @param key 唯一标识。
     * @param range UTF-16 range。
     * @param layout 所属 layout 快照。
     * @param role [GlyphRole]。
     * @param clipFraction 可见区域裁切 fraction（0..1）。
     */
    data class GlyphOverlay(
        val key: Long,
        val range: TextRange,
        val layout: ComposeLayoutSnapshot,
        val role: GlyphRole,
        val clipFraction: Float,
    )

    /** motion 是否有效 — traversal 建出来才有效。 */
    val isValid: Boolean get() = traversal.isValid

    /**
     * 采样当前帧 — 同时产出 caret rect 和文字 glyph overlay。
     *
     * - traversal 无效时 caret 直接落在 [newCaretRect]，glyph overlays 为空。
     * - traversal 有效时 caret 由 [CaretTraversal.sampleCaret] 算，
     *   每个 glyph channel 按 master progress 经区间映射后线性插值 fraction。
     * - fraction > 0 的 glyph 进入 [Sample.glyphOverlays] 和 [Sample.hiddenRanges]。
     *
     * @param frameTimeNanos 当前帧时间戳（Compose frame clock）。
     */
    fun sample(frameTimeNanos: Long): Sample {
        val progress = computeProgress(frameTimeNanos)
        val caretRect = if (isValid) traversal.sampleCaret(progress.value) else newCaretRect
        val glyphOverlays = mutableListOf<GlyphOverlay>()
        val hiddenRanges = mutableListOf<TextRange>()
        for ((key, ch) in glyphChannels) {
            val localProgress = mapProgressToChannel(progress.value, ch.startProgress, ch.endProgress)
            val fraction =
                (ch.fromFraction + (ch.toFraction - ch.fromFraction) * localProgress).coerceIn(0f, 1f)
            if (fraction > 0f) {
                glyphOverlays.add(
                    GlyphOverlay(
                        key = key,
                        range = ch.range,
                        layout = ch.layout,
                        role = ch.role,
                        clipFraction = fraction,
                    ),
                )
                hiddenRanges.add(ch.range)
            }
        }
        return Sample(
            caretRect = caretRect,
            glyphOverlays = glyphOverlays,
            hiddenRanges = hiddenRanges,
            finished = progress.finished,
            isValid = isValid,
        )
    }

    /**
     * motion 是否已完成（progress >= 1 或 durationNanos <= 0）。
     *
     * @param frameTimeNanos 当前帧时间戳。
     */
    fun isFinished(frameTimeNanos: Long): Boolean = computeProgress(frameTimeNanos).finished

    /**
     * 算 master progress [0,1] 和 finished 状态。
     */
    private fun computeProgress(frameTimeNanos: Long): ProgressResult {
        if (durationNanos <= 0L) return ProgressResult(1f, true)
        val elapsed = frameTimeNanos - startedAtNanos
        return when {
            elapsed <= 0L -> ProgressResult(0f, false)
            elapsed >= durationNanos -> ProgressResult(1f, true)
            else -> ProgressResult(elapsed.toFloat() / durationNanos.toFloat(), false)
        }
    }

    private data class ProgressResult(val value: Float, val finished: Boolean)

    /**
     * 把 master progress 映射到 channel 局部 progress [0,1]。
     */
    private fun mapProgressToChannel(
        masterProgress: Float,
        start: Float,
        end: Float,
    ): Float {
        if (masterProgress <= start) return 0f
        if (masterProgress >= end) return 1f
        val span = end - start
        if (span <= 0f) return 1f
        return (masterProgress - start) / span
    }

    companion object {
        /** glyph key 单调递增计数器 — 进程级唯一。 */
        private var nextGlyphKey = 1L

        /**
         * 从 [ComposeVisualPatch] 构造一笔协调运动。
         *
         * 1. 先构造 [CaretTraversal]（从 old/new layout + caret rect）。
         * 2. traversal 有效时，为 inserted/deleted glyph 分配 master progress 区间：
         *    - inserted 占前半段 [0, 0.5]，按 targetRange.start 顺序，0→1 吐字；
         *    - deleted 占后半段 [0.5, 1]，按 range.start 反序（右往左吞），1→0 吞字。
         * 3. traversal 无效时 glyph channels 为空，motion 无效 → 直接显示最终静态正文。
         *
         * @param patch 屏幕帧差异描述。
         * @param frameTimeNanos motion 开始时间戳。
         * @param durationNanos motion 时长（<=0 表示瞬时完成）。
         */
        fun fromPatch(
            patch: ComposeVisualPatch,
            frameTimeNanos: Long,
            durationNanos: Long,
        ): CoordinatedEditMotion {
            val oldLayout = patch.oldLayout
            val newLayout = patch.newLayout
            val oldCaretRect = patch.originCaretRect
            val newCaretRect = patch.targetCaretRect
            val oldSelection = oldLayout.selection
            val newSelection = newLayout.selection

            val traversal =
                CaretTraversal.fromLayouts(
                    oldLayout = oldLayout.result,
                    newLayout = newLayout.result,
                    oldCaretOffset = oldSelection.end,
                    newCaretOffset = newSelection.end,
                    oldCaretRect = oldCaretRect,
                    newCaretRect = newCaretRect,
                )

            // traversal 无效 → 无文字动画，motion 整体无效
            if (!traversal.isValid) {
                return CoordinatedEditMotion(
                    oldSelection = oldSelection,
                    newSelection = newSelection,
                    oldCaretRect = oldCaretRect,
                    newCaretRect = newCaretRect,
                    oldLine = -1,
                    newLine = -1,
                    traversal = traversal,
                    glyphChannels = emptyMap(),
                    startedAtNanos = frameTimeNanos,
                    durationNanos = durationNanos,
                )
            }

            val channels = buildGlyphChannels(patch)

            return CoordinatedEditMotion(
                oldSelection = oldSelection,
                newSelection = newSelection,
                oldCaretRect = oldCaretRect,
                newCaretRect = newCaretRect,
                oldLine = -1,
                newLine = -1,
                traversal = traversal,
                glyphChannels = channels,
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )
        }

        /**
         * 为 inserted/deleted glyph 分配 master progress 区间。
         *
         * - 只有 inserted：inserted 占全区间 [0, 1]
         * - 只有 deleted：deleted 占全区间 [0, 1]
         * - 混合：inserted 占前半段 [0, 0.5]，deleted 占后半段 [0.5, 1]
         */
        private fun buildGlyphChannels(patch: ComposeVisualPatch): Map<Long, GlyphChannel> {
            val insertedRanges = patch.insertedUnits
            val deletedRanges = patch.deletedUnits
            if (insertedRanges.isEmpty() && deletedRanges.isEmpty()) return emptyMap()

            val channels = mutableMapOf<Long, GlyphChannel>()
            val insertedCount = insertedRanges.size
            val deletedCount = deletedRanges.size
            val hasBoth = insertedCount > 0 && deletedCount > 0
            val newLayout = patch.newLayout
            val oldLayout = patch.oldLayout

            // inserted 区间：混合时占 [0, 0.5]，纯 inserted 时占 [0, 1]
            val insertedSpan = if (hasBoth) 0.5f else 1f
            for (i in insertedRanges.indices) {
                val key = nextGlyphKey++
                val range = insertedRanges[i]
                val start = insertedSpan * i.toFloat() / insertedCount.toFloat()
                val end = insertedSpan * (i + 1).toFloat() / insertedCount.toFloat()
                channels[key] =
                    GlyphChannel(
                        key = key,
                        range = range,
                        layout = newLayout,
                        role = GlyphRole.Inserted,
                        fromFraction = 0f,
                        toFraction = 1f,
                        startProgress = start,
                        endProgress = end,
                    )
            }

            // deleted 区间：混合时占 [0.5, 1]，纯 deleted 时占 [0, 1]，反序（右往左吞）
            val deletedOffset = if (hasBoth) 0.5f else 0f
            val deletedSpan = if (hasBoth) 0.5f else 1f
            for (i in deletedRanges.indices) {
                val key = nextGlyphKey++
                val range = deletedRanges[i]
                val start = deletedOffset + deletedSpan * (deletedCount - 1 - i).toFloat() / deletedCount.toFloat()
                val end = deletedOffset + deletedSpan * (deletedCount - i).toFloat() / deletedCount.toFloat()
                channels[key] =
                    GlyphChannel(
                        key = key,
                        range = range,
                        layout = oldLayout,
                        role = GlyphRole.Deleted,
                        fromFraction = 1f,
                        toFraction = 0f,
                        startProgress = start,
                        endProgress = end,
                    )
            }
            return channels
        }

        /**
         * Issue #737：纯 selection/caret 移动构造的 motion — 无文字吞吐。
         *
         * caret 从 [originCaretRect] 移动到 [targetCaretRect]，glyph channels 为空。
         * traversal 由 old/new layout + caret rect 构造（可能跨行）。
         *
         * @param oldLayout 编辑前 layout。
         * @param newLayout 编辑后 layout。
         * @param originCaretRect 编辑前 caret rect。
         * @param targetCaretRect 编辑后 caret rect。
         * @param frameTimeNanos motion 开始时间戳。
         * @param durationNanos motion 时长（<=0 表示瞬时完成）。
         */
        fun forSelectionMove(
            oldLayout: ComposeLayoutSnapshot,
            newLayout: ComposeLayoutSnapshot,
            originCaretRect: Rect,
            targetCaretRect: Rect,
            frameTimeNanos: Long,
            durationNanos: Long,
        ): CoordinatedEditMotion {
            val traversal =
                CaretTraversal.fromLayouts(
                    oldLayout = oldLayout.result,
                    newLayout = newLayout.result,
                    oldCaretOffset = oldLayout.selection.end,
                    newCaretOffset = newLayout.selection.end,
                    oldCaretRect = originCaretRect,
                    newCaretRect = targetCaretRect,
                )
            return CoordinatedEditMotion(
                oldSelection = oldLayout.selection,
                newSelection = newLayout.selection,
                oldCaretRect = originCaretRect,
                newCaretRect = targetCaretRect,
                oldLine = -1,
                newLine = -1,
                traversal = traversal,
                glyphChannels = emptyMap(),
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )
        }
    }
}
