package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.effectiveRawText

/**
 * #708 评论 5725706551：本地 handoff scene rebase —
 * 把旧 visible scene + patch.offsetMap + patch.oldLayout/newLayout → rebased handoff scene。
 *
 * **背景**：旧 [ComposeEditorVisualState.publishLocalHandoffScene] 把上一笔动画当前可见帧 scene
 * （旧正文坐标）原样塞进新正文坐标系，导致快速输入/删除时出现重影、旧字残留和闪烁。
 * 核心问题是：
 * 1. 旧 hiddenRanges 直接复制到新坐标系 — 坐标不匹配；
 * 2. 为 deletedUnits 新建 alpha=1 的完整 ghost — 如果旧 active unit 正在动画中
 *    （alpha 在 0..1），直接新建 alpha=1 的 ghost 会导致重影；
 * 3. 旧 active unit 没有做 rebase — 仍然在旧坐标系中。
 *
 * **本 helper 做正确的 rebase**：
 * 1. 旧 active unit（targetRange != null）通过 [ComposeVisualRebase.splitMappedRangeForward]
 *    映射到新正文坐标：
 *    - 存活 slice（SURVIVING）：targetRange/range 改成 newRange，layout 改成 newLayout，
 *      alpha/position 固定在当前可见值（不瞬移、不推进时间）；
 *    - 被删除 slice（GHOST）：从旧 unit 当前可见 alpha/position 转 handoff ghost
 *      （不新建 alpha=1 的完整 ghost）；
 * 2. 已有 ghost（targetRange == null）继续保持当前可见状态
 *    （保持原有 alpha/position 通道不变）；
 * 3. [RebasedHandoff.ghostedCoverage] 记录 rebase 阶段已经转成 ghost 的旧正文范围，
 *    供 [ComposeEditorVisualState.publishLocalHandoffScene] 计算
 *    "deletedUnits - 已由旧 active unit 转 ghost 的范围 = 还需要从 oldLayout 新建完整 ghost 的范围"。
 *
 * **handoff 与 timeline 的关键区别**：
 * - timeline（[ComposeVisualTimeline.applyPatch]）：有 frameTimeNanos，创建后续动画通道，
 *   alpha 从当前值继续到目标值，position 从当前值重定向到新位置。
 * - handoff（本 helper）：不推进时间，只保持上一可见帧的 alpha/position，
 *   完成新坐标系所有权交接。后续 timeline 接管后再创建动画通道。
 */
internal object ComposeLocalHandoffRebase {
    /**
     * 把旧 visible scene + patch → rebased handoff。
     *
     * 旧 scene 的 units 已经 sampled 过（由 [ComposeVisualTimeline.sample] 返回），
     * 所以 alpha.from = 当前可见 alpha，position.from = 当前屏幕位置。
     * handoff 不推进时间，把 alpha/position 固定在当前可见值，等 timeline 接管后再创建动画通道。
     *
     * @param scene 旧 visible scene（上一帧 sample 出来的）。
     * @param patch 本笔 patch（含 offsetMap, oldLayout, newLayout）。
     * @param nextChildKey split 时为每个子 unit 分配独立新 key 的 allocator。
     *   #708 评论 5727808906：split 后子 unit 不能共用父 key，否则
     *   [ComposeVisualScene.unitClipFractions]（Map<Long, Float>，key=unit.key）
     *   同 key 互相覆盖，三段文字拿同一个 fraction。
     *   allocator 由调用方 [ComposeEditorVisualState] 提供（nextHandoffUnitKey++）。
     * @return rebased handoff — units 已映射到新坐标系，ghostedCoverage 记录已转 ghost 的旧正文范围，
     *   initialClipFractionsByKey 记录每个 child 的真实首帧 fraction。
     */
    @Suppress("CyclomaticComplexMethod")
    fun rebase(
        scene: ComposeVisualScene,
        patch: ComposeVisualPatch,
        nextChildKey: () -> Long,
    ): RebasedHandoff {
        val newLayout = patch.newLayout
        // Issue #717 评论 5742904417 修复1：targetRange 是 raw 坐标，边界检查用 rawText 长度。
        val newTextLength = newLayout.effectiveRawText.length
        val rebasedUnits = mutableListOf<VisualTextUnit>()
        val ghostedCoverage = mutableListOf<TextRange>()
        // #708 评论 5731952690 修复3：改名为 initialClipFractionsByKey —
        // 语义从"child 继承 parent 的旧 fraction"改成"每个 rebase child 的真实首帧 fraction"。
        // ghost 首帧继承这个具体 glyph/slice 上一帧真实可见多少，而不是一刀切 0 或 parent 进度。
        val initialClipFractionsByKey = mutableMapOf<Long, Float>()
        // Issue #725 评论 5750735497：停止自绘屏幕 caret —
        // 不再记录 initialClipCursorsByKey，clipFraction 改由 alpha 通道直接驱动。
        // split child 继承 parent 旧 fraction，不再用 cursor 位置算精确 fraction。
        val oldCoordinated = scene.coordinatedSpatialClip

        for (unit in scene.units) {
            val target = unit.targetRange
            if (target == null) {
                // 已有 ghost：继续保持当前可见状态（保持原有 alpha/position 通道不变）
                // 历史 ghost 沿用旧 clip fraction
                rebasedUnits.add(unit)
                scene.unitClipFractions[unit.key]?.let { fraction ->
                    initialClipFractionsByKey[unit.key] = fraction
                }
                continue
            }
            // 旧 active unit：通过 splitMappedRangeForward 映射到新正文坐标
            val slices = computeSlices(target, patch, newTextLength)
            val isSplit = slices.size >= 2
            val parentOldFraction = scene.unitClipFractions[unit.key]
            for (slice in slices) {
                // Issue #720 评论 5747339452：本地 patch + surviving slice 自然几何变化 →
                // handoff 首帧就释放给 BasicTextField，不加入 rebasedUnits、不写 fraction/cursor map、不进入 mergedHidden。
                // 这样 BasicTextField 从 handoff 首帧直接画最终位置，不是等 timeline 下一帧才释放。
                // 真正需要 retained/reflow ownership 的非本地/Core 路径不受 patch.intent == null 这层门控影响。
                val isLocalSurvivorWithGeometryChange =
                    patch.intent == null &&
                        slice.kind == ComposeVisualRebase.MappedRangeSliceKind.SURVIVING &&
                        slice.newSubRange != null &&
                        ComposeVisualRebase.naturalGeometryChanged(
                            oldLayout = unit.layout,
                            oldRange = slice.oldSubRange,
                            newLayout = newLayout,
                            newRange = slice.newSubRange,
                        )
                if (isLocalSurvivorWithGeometryChange) {
                    continue
                }
                val childKey = if (isSplit) nextChildKey() else unit.key
                // Issue #720 评论 5747339452：handoff 首帧就释放自然几何变化的 survivor，
                // 不保留瞬态。handoff 和 timeline 两边都释放（timeline 侧由
                // ComposeVisualTimeline.mapSurvivingUnits() + naturalGeometryChanged 释放），
                // BasicTextField 从 handoff 首帧直接画最终位置，不再有 oldPosition→newPosition 位移动画。
                // #708 评论 5731952690 修复3：记录每个 rebase child 的真实首帧 fraction —
                // 逻辑抽取到 computeSliceInitialFraction helper，降低 rebase() 复杂度。
                // 返回 null 表示不需要记录（非 split 且 parentOldFraction==null）。
                computeSliceInitialFraction(
                    unit = unit,
                    slice = slice,
                    isSplit = isSplit,
                    parentOldFraction = parentOldFraction,
                    oldCoordinated = oldCoordinated,
                )?.let { initialClipFractionsByKey[childKey] = it }
                processSlice(
                    unit = unit,
                    slice = slice,
                    childKey = childKey,
                    newLayout = newLayout,
                    rebasedUnits = rebasedUnits,
                    ghostedCoverage = ghostedCoverage,
                )
            }
        }

        return RebasedHandoff(
            units = rebasedUnits,
            ghostedCoverage = ghostedCoverage,
            initialClipFractionsByKey = initialClipFractionsByKey,
        )
    }

    /**
     * #708 评论 5731952690 修复3 / 评论 5733321056 修复3：计算 rebase child 的真实首帧 fraction —
     * 从 rebase() 抽取以降低复杂度。
     *
     * Issue #725 评论 5750735497：停止自绘屏幕 caret 后，不再用 cursor 位置算精确 split fraction。
     * Issue #725 评论 5752025711：parent 被 split 时，把 parent 的纯文字 reveal 进度投影到每个
     * child 自己的区间（child 在 reveal 边界之前→1，之后→0，边界落在 child 内→局部 fraction），
     * 不再让所有 split child 机械继承同一个 parent fraction。
     *
     * 返回值语义：
     * - null：不需要记录（parentOldFraction==null）
     * - 非 null：要记录的 fraction
     */
    private fun computeSliceInitialFraction(
        unit: VisualTextUnit,
        slice: ComposeVisualRebase.MappedRangeSlice,
        isSplit: Boolean,
        parentOldFraction: Float?,
        oldCoordinated: Boolean,
    ): Float? {
        if (parentOldFraction == null) {
            return null
        }
        // Issue #725 评论 5752025711：parent unit 被 split 时，不再让所有 child
        // 机械继承同一个 parent fraction。把 parent 的纯文字 reveal 进度投影到每个
        // child 自己的区间（按 slice.oldSubRange 在 unit.range 中的相对位置）：
        // - child 完全位于 parent reveal 边界之前 → 已完整显示 → fraction=1
        // - child 完全位于边界之后 → 尚未显示 → fraction=0
        // - reveal 边界落在 child 内 → 换算成 child 局部 0..1 fraction
        // 不读取屏幕 caret，继续用纯文字模型。
        return computeRevealFractionForChild(
            parentRange = unit.range,
            parentRevealFraction = parentOldFraction,
            childRange = slice.oldSubRange,
        )
    }

    private fun processSlice(
        unit: VisualTextUnit,
        slice: ComposeVisualRebase.MappedRangeSlice,
        childKey: Long,
        newLayout: ComposeLayoutSnapshot,
        rebasedUnits: MutableList<VisualTextUnit>,
        ghostedCoverage: MutableList<TextRange>,
    ) {
        if (slice.kind == ComposeVisualRebase.MappedRangeSliceKind.SURVIVING &&
            slice.newSubRange != null
        ) {
            rebasedUnits.add(
                mapSurvivingSliceToHandoff(
                    unit = unit,
                    oldRange = slice.oldSubRange,
                    newRange = slice.newSubRange,
                    newLayout = newLayout,
                    childKey = childKey,
                ),
            )
        } else {
            rebasedUnits.add(toHandoffGhost(unit, slice.oldSubRange, childKey))
            ghostedCoverage.add(slice.oldSubRange)
        }
    }

    /**
     * 计算切片 — 调用 [ComposeVisualRebase.computeSlices] 共享 helper。
     *
     * #708 评论 5725706551：切片逻辑已抽取到 [ComposeVisualRebase.computeSlices]，
     * timeline 和 handoff 共用同一份切片计算。
     */
    private fun computeSlices(
        target: TextRange,
        patch: ComposeVisualPatch,
        newTextLength: Int,
    ): List<ComposeVisualRebase.MappedRangeSlice> =
        ComposeVisualRebase.computeSlices(target, patch.offsetMap, newTextLength, patch.intent)

    /**
     * 存活 slice → handoff unit —
     * targetRange/range 改成 newRange，layout 改成 newLayout，
     * alpha/position/reveal 固定在当前可见值（不瞬移、不推进时间）。
     *
     * **与 [ComposeVisualTimeline.mapSurvivingSlice] 的对应关系**（#708 评论 5725706551）：
     * - timeline 版本：alpha/reveal 通道不变（继续原动画），position 通道在新位置变化时创建
     *   TimedOffset(from=oldPosition, to=newPosition, startedAtNanos=frameTimeNanos, durationNanos)。
     * - handoff 版本（本方法）：alpha/position/reveal 都固定在当前可见值（TimedFloat(from, from, 0, 0)），
     *   不推进时间，等 timeline 接管后再创建动画通道。
     *
     * 旧 scene 的 units 已经 sampled 过，alpha.from = 当前可见 alpha，position.from = 当前屏幕位置，
     * reveal.from = 当前可见 reveal。
     * handoff 把 alpha/position/reveal 固定在当前值（TimedFloat(from, from, 0, 0)），
     * draw 层 sample 时得到 from = 当前可见值，保持连续性。
     * 如果新布局位置变化，后面的正式 timeline 再负责 position redirect。
     */
    private fun mapSurvivingSliceToHandoff(
        unit: VisualTextUnit,
        oldRange: TextRange,
        newRange: TextRange,
        newLayout: ComposeLayoutSnapshot,
        childKey: Long,
    ): VisualTextUnit {
        val frozenAlpha = TimedFloat(unit.alpha.from, unit.alpha.from, 0L, 0L)
        val frozenReveal = TimedFloat(unit.reveal.from, unit.reveal.from, 0L, 0L)
        // #708 评论 5727440517：surviving slice 的屏幕位置用 sliceScreenPosition 计算 —
        // oldRange == unit.range 时返回 unit.position.from（父当前屏幕位置）；
        // oldRange 是父 unit 真子区间时用"slice 自然位置 + 父 unit 当前位移"，
        // 不再直接用父 unit 左上角，避免 surviving 首帧文字跳到父 unit 开头位置。
        // 注意：算位置必须用 oldRange + unit.layout（旧 layout），保持这一帧用户已看到的旧屏幕位置；
        // 不能直接用 newRange 的自然位置，否则 surviving 文字会提前跳到最终位置。
        val currentSlicePosition =
            ComposeVisualRebase.sliceScreenPosition(
                layout = unit.layout,
                parentRange = unit.range,
                sliceRange = oldRange,
                parentScreenPosition = unit.position.from,
            ) ?: unit.position.from
        val frozenPosition = TimedOffset(currentSlicePosition, currentSlicePosition, 0L, 0L)
        return unit.copy(
            key = childKey,
            layout = newLayout,
            range = newRange,
            targetRange = newRange,
            alpha = frozenAlpha,
            position = frozenPosition,
            reveal = frozenReveal,
        )
    }

    /**
     * 被删除 slice → handoff ghost —
     * 从旧 unit 当前可见 alpha/position/reveal 转 ghost（不新建 alpha=1 的完整 ghost）。
     *
     * **与 [ComposeVisualTimeline.toGhost] 的对应关系**（#708 评论 5725706551）：
     * - timeline 版本：alpha/reveal 从当前值继续到 0（TimedFloat(now, 0f, now, durationNanos)），
     *   position 固定在当前屏幕位置（考虑切片 ghost 的父 unit 位移）。
     * - handoff 版本（本方法）：alpha/position/reveal 都固定在当前可见值（TimedFloat(from, from, 0, 0)），
     *   不推进时间，等 timeline 接管后再创建 alpha → 0 / reveal → 0 的淡出通道。
     *
     * 关键约束（#708 评论 5725706551）：
     * handoff ghost 继承当前可见 alpha — 不能新建 alpha=1 的完整 ghost 来替代正在动画中的 unit。
     * 如果旧 unit 正在做 alpha 0→1 的插入动画（当前 alpha=0.5），handoff ghost 应保持 alpha=0.5，
     * 而不是新建 alpha=1 的完整 ghost。否则会出现"旧字残留 + 新字同时画"的重影。
     *
     * targetRange = null，role = DeletedGhost。
     * alpha/position/reveal 固定在当前可见值，等 timeline 接管后再创建淡出通道。
     */
    private fun toHandoffGhost(
        unit: VisualTextUnit,
        ghostRange: TextRange,
        childKey: Long,
    ): VisualTextUnit {
        val frozenAlpha = TimedFloat(unit.alpha.from, unit.alpha.from, 0L, 0L)
        val frozenReveal = TimedFloat(unit.reveal.from, unit.reveal.from, 0L, 0L)
        // #708 评论 5726837636：子片段 ghost 的屏幕位置用 sliceScreenPosition 计算 —
        // ghostRange == unit.range 时返回 unit.position.from（父当前屏幕位置）；
        // ghostRange 是父 unit 真子区间时用"slice 自然位置 + 父 unit 当前位移"，
        // 不再直接用父 unit 左上角，避免 handoff 首帧旧字跳到父 unit 开头位置。
        val slicePosition =
            ComposeVisualRebase.sliceScreenPosition(
                layout = unit.layout,
                parentRange = unit.range,
                sliceRange = ghostRange,
                parentScreenPosition = unit.position.from,
            ) ?: unit.position.from
        val frozenPosition = TimedOffset(slicePosition, slicePosition, 0L, 0L)
        return unit.copy(
            key = childKey,
            range = ghostRange,
            targetRange = null,
            alpha = frozenAlpha,
            position = frozenPosition,
            reveal = frozenReveal,
            role = VisualUnitRole.DeletedGhost,
        )
    }

    /**
     * rebase 结果。
     *
     * @param units rebased units（存活 + ghost），已映射到新坐标系。
     * @param ghostedCoverage rebase 阶段已经转成 ghost 的旧正文范围（旧坐标系）。
     *   供 [ComposeEditorVisualState.publishLocalHandoffScene] 计算
     *   "deletedUnits - 已由旧 active unit 转 ghost 的范围 = 还需要从 oldLayout 新建完整 ghost 的范围"。
     * @param initialClipFractionsByKey #708 评论 5731952690 修复3 / 评论 5733321056 修复3：
     *   每个 rebase child 的真实首帧 fraction —
     *   语义为"这个具体 glyph/slice 上一帧真实可见多少"：
     *   - 历史 ghost：scene.unitClipFractions（沿用旧 fraction）
     *   - active unit 转出的 ghost（整个 unit 变 ghost，非 split）：parent 旧 fraction
     *   - partial split 的 ghost slice：用 parent unit 自己的 unitClipCursor + fractionFor
     *     算 slice 自己的旧 fraction（role=DeletedGhost）
     *   - partial split 的 surviving slice：用 parent unit 自己的 unitClipCursor + fractionFor
     *     算 slice 自己的旧 fraction（role 保持原 unit.role，不是 DeletedGhost）
     *   publishLocalHandoffScene 用此 map 设 handoff 首帧 unitClipFractions，
     *   不再一刀切 ghost=0。key 不在 map 中的 child 表示没有旧 fraction（新插入 unit）。
     */
    data class RebasedHandoff(
        val units: List<VisualTextUnit>,
        val ghostedCoverage: List<TextRange>,
        val initialClipFractionsByKey: Map<Long, Float> = emptyMap(),
    )
}
