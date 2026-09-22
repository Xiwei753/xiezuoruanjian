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
 * @param prepared Issue #737 评论 5782769758：是否处于 prepared 状态 —
 *   true 表示已构造（有 traversal、glyph ownership、progress=0）但还没开始计时；
 *   false 表示已开始计时（running）或瞬时完成。
 *   prepared motion 的 [sample] 永远返回 progress=0 的结果：
 *   - caretRect = traversal.sampleCaret(0f) = origin caret
 *   - inserted glyph fraction=0 → 不进 glyphOverlays，但进 hiddenRanges（Inserted && !finished）
 *     → 新字被 hidden ownership 接管
 *   - deleted glyph fraction=1 → 进 glyphOverlays（完整可见）
 *   这正是"新字第一次画出来时就已经处于 hidden ownership，caret 也还在 origin"。
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
    private val prepared: Boolean = false,
) {
    /**
     * Issue #737 评论 5782769758：是否处于 prepared 状态（已构造但未开始计时）。
     * 供 [ComposeEditorVisualState] 判断是否需要在 drainPendingPatchesAtFrame 里 start。
     */
    val isPrepared: Boolean get() = prepared

    /**
     * Issue #737 评论 5782769758：把 prepared motion 转成 running motion —
     * 返回一个 prepared=false、startedAtNanos=[startedAtNanos] 的新实例，
     * 所有其他字段（traversal、glyphChannels、durationNanos、old/new selection/caret/line）不变。
     *
     * 调用时机：[ComposeEditorVisualState.drainPendingPatchesAtFrame] 的真实 frameTimeNanos 到达时，
     * 把 applyFrameUpdate 构造的 prepared motion start 成 running motion。
     *
     * @param startedAtNanos 真实帧时间戳（Compose frame clock）。
     */
    fun start(startedAtNanos: Long): CoordinatedEditMotion =
        CoordinatedEditMotion(
            oldSelection = oldSelection,
            newSelection = newSelection,
            oldCaretRect = oldCaretRect,
            newCaretRect = newCaretRect,
            oldLine = oldLine,
            newLine = newLine,
            traversal = traversal,
            glyphChannels = glyphChannels,
            startedAtNanos = startedAtNanos,
            durationNanos = durationNanos,
            prepared = false,
        )

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
     * @param glyphOverlays 当前帧应绘制的 glyph overlay 列表
     *   （inserted 和 deleted 都进；deleted ghost 携带自己的 oldLayout）。
     * @param hiddenRanges 当前帧应被动画层接管（裁掉 BasicTextField 原字）的 range 列表。
     *   **Issue #737 评论 5781084709 修复点 4**：只包含 Inserted 角色的 current-layout ranges
     *   （这些 range 属于 newLayout，裁掉 BasicTextField 里的对应正文是正确的）。
     *   Deleted 不进入此列表 — deleted ghost 通过 [glyphOverlays] 用自己的 oldLayout 绘制，
     *   不裁 BasicTextField 当前正文。
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
     * Issue #737 评论 5781084709 修复点 4：overlay 携带自己的 [layout] —
     * inserted overlay 用 newLayout，deleted overlay 用 oldLayout。
     * [EditorTextFieldDrawLayer.drawGlyphOverlay] 用 overlay 自带的 layout 画 ghost，
     * 不用当前 BasicTextField 的 layout。
     *
     * @param key 唯一标识。
     * @param range UTF-16 range。
     * @param layout 所属 layout 快照（inserted=newLayout，deleted=oldLayout）。
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
     * - fraction > 0 的 glyph 进入 [Sample.glyphOverlays]（inserted 和 deleted 都进）。
     *
     * Issue #737 评论 5781084709 修复点 3：Inserted range 从 motion 开始到结束前都必须隐藏，
     * 即使当前 fraction=0。否则 BasicTextField 在 progress=0 时先完整显示新字，
     * 下一帧才被裁掉，表现为"新字先闪出来一下再重新吐字"。
     *
     * Issue #737 评论 5781084709 修复点 4：[Sample.hiddenRanges] 只包含 Inserted 角色的 range
     * （current-layout ranges）。Deleted 不加入 hiddenRanges — deleted ghost 用自己的 oldLayout
     * 通过 [Sample.glyphOverlays] 绘制，不裁 BasicTextField 当前正文。
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
            }
            // 修复点 3+4：Inserted range 从 motion 开始到结束前都必须隐藏（即使 fraction=0）。
            // Deleted 不加入 hiddenRanges（deleted ghost 用自己的 oldLayout 画，不裁 BasicTextField）。
            if (ch.role == GlyphRole.Inserted && !progress.finished) {
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
     * Issue #737 评论 5782769758：prepared motion 永远未完成（progress 固定 0）。
     *
     * @param frameTimeNanos 当前帧时间戳。
     */
    fun isFinished(frameTimeNanos: Long): Boolean = !prepared && computeProgress(frameTimeNanos).finished

    /**
     * 算 master progress [0,1] 和 finished 状态。
     *
     * Issue #737 评论 5782769758：prepared motion 直接返回 progress=0、未完成 —
     * 让 [sample] 产出"origin caret + 新字 hidden ownership"的初始 presentation。
     */
    private fun computeProgress(frameTimeNanos: Long): ProgressResult {
        if (prepared) return ProgressResult(0f, false)
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
         * @param prepared Issue #737 评论 5782769758：是否构造为 prepared motion（未开始计时）。
         */
        fun fromPatch(
            patch: ComposeVisualPatch,
            frameTimeNanos: Long,
            durationNanos: Long,
            prepared: Boolean = false,
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
                    // Issue #737 评论 5781634285 修复点 3：traversal 只使用 patch 里的
                    // originCaretOffset / targetCaretOffset — 这两个 offset 是生成
                    // originCaretRect / targetCaretRect 时用的同一份 fact selection end。
                    // 不再重新读 oldLayout.selection.end / newLayout.selection.end —
                    // 快速输入、同帧 batch、layout/selection 到达次序变化时，这两个 selection
                    // 不保证就是生成那两个 caret rect 时用的 offset，可能出现
                    // "rect 是 fact 的 caret，line 却是 snapshot selection 的 line"，
                    // 让 traversal 在软换行附近判错"同行/跨行"。
                    // oldSelection / newSelection 仍保留用于构造 CoordinatedEditMotion 的
                    // oldSelection/newSelection 参数（下方两处 return）。
                    oldCaretOffset = patch.originCaretOffset,
                    newCaretOffset = patch.targetCaretOffset,
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
                    prepared = prepared,
                )
            }

            val channels = buildGlyphChannels(patch)

            // Issue #737 评论 5781084709 修复点 6：从 traversal.oldLine / traversal.newLine
            // 取真实行号传入 motion 构造，不再固定传 -1。
            return CoordinatedEditMotion(
                oldSelection = oldSelection,
                newSelection = newSelection,
                oldCaretRect = oldCaretRect,
                newCaretRect = newCaretRect,
                oldLine = traversal.oldLine,
                newLine = traversal.newLine,
                traversal = traversal,
                glyphChannels = channels,
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
                prepared = prepared,
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
         * Issue #737 评论 5782106370：[originCaretOffset] / [targetCaretOffset] 是生成
         * [originCaretRect] / [targetCaretRect] 时用的同一份 caret offset，不再从
         * [ComposeLayoutSnapshot.selection] 读取。调用方传入的 oldLayout/newLayout 可能是
         * 同一个 snapshot（纯 selection 移动场景），从 layout.selection 读会让 old/new offset
         * 相同，[CaretTraversal] 误判为同行，光标斜穿两行。与 [fromPatch] 修复点 3 同理。
         *
         * @param oldLayout 编辑前 layout。
         * @param newLayout 编辑后 layout。
         * @param originCaretRect 编辑前 caret rect。
         * @param targetCaretRect 编辑后 caret rect。
         * @param originCaretOffset 生成 [originCaretRect] 时用的 caret offset（UTF-16）—
         *     必须与 rect 来自同一次实际移动，不从 layout.selection 读。
         * @param targetCaretOffset 生成 [targetCaretRect] 时用的 caret offset（UTF-16）—
         *     必须与 rect 来自同一次实际移动，不从 layout.selection 读。
         * @param frameTimeNanos motion 开始时间戳。
         * @param durationNanos motion 时长（<=0 表示瞬时完成）。
         * @param prepared Issue #737 评论 5782769758：是否构造为 prepared motion（未开始计时）。
         */
        fun forSelectionMove(
            oldLayout: ComposeLayoutSnapshot,
            newLayout: ComposeLayoutSnapshot,
            originCaretRect: Rect,
            targetCaretRect: Rect,
            originCaretOffset: Int,
            targetCaretOffset: Int,
            frameTimeNanos: Long,
            durationNanos: Long,
            prepared: Boolean = false,
        ): CoordinatedEditMotion {
            val traversal =
                CaretTraversal.fromLayouts(
                    oldLayout = oldLayout.result,
                    newLayout = newLayout.result,
                    oldCaretOffset = originCaretOffset,
                    newCaretOffset = targetCaretOffset,
                    oldCaretRect = originCaretRect,
                    newCaretRect = targetCaretRect,
                )
            // Issue #737 评论 5781084709 修复点 6：从 traversal.oldLine / traversal.newLine
            // 取真实行号传入 motion 构造，不再固定传 -1。
            // Issue #737 评论 5782447373：oldSelection/newSelection 用真实 old/new caret offset 构造，
            // 不再从 oldLayout.selection / newLayout.selection 读取 — 纯 selection move 调用方传入的
            // oldLayout/newLayout 可能是同一个 snapshot，从 layout.selection 读会让 old/new selection 相同，
            // 与"一笔 motion 保存完整 old/new 状态"不一致。oldLayout/newLayout 仍用于 traversal 行几何。
            return CoordinatedEditMotion(
                oldSelection = TextRange(originCaretOffset, originCaretOffset),
                newSelection = TextRange(targetCaretOffset, targetCaretOffset),
                oldCaretRect = originCaretRect,
                newCaretRect = targetCaretRect,
                oldLine = traversal.oldLine,
                newLine = traversal.newLine,
                traversal = traversal,
                glyphChannels = emptyMap(),
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
                prepared = prepared,
            )
        }
    }
}
