package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

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
     * @return rebased handoff — units 已映射到新坐标系，ghostedCoverage 记录已转 ghost 的旧正文范围。
     */
    fun rebase(
        scene: ComposeVisualScene,
        patch: ComposeVisualPatch,
        nextChildKey: () -> Long,
    ): RebasedHandoff {
        val newLayout = patch.newLayout
        val newTextLength = newLayout.result.layoutInput.text.length
        val rebasedUnits = mutableListOf<VisualTextUnit>()
        val ghostedCoverage = mutableListOf<TextRange>()

        for (unit in scene.units) {
            val target = unit.targetRange
            if (target == null) {
                // 已有 ghost：继续保持当前可见状态（保持原有 alpha/position 通道不变）
                rebasedUnits.add(unit)
                continue
            }
            // 旧 active unit：通过 splitMappedRangeForward 映射到新正文坐标
            val slices = computeSlices(target, patch, newTextLength)
            // #708 评论 5727808906：split 时分配独立新 key —
            // 只有一个 slice 且代表整个父 unit 时保留 parent key；
            // 2 个及以上子 unit 时每个子 unit 分配独立新 key。
            val isSplit = slices.size >= 2
            for (slice in slices) {
                val childKey = if (isSplit) nextChildKey() else unit.key
                if (slice.kind == ComposeVisualRebase.MappedRangeSliceKind.SURVIVING &&
                    slice.newSubRange != null
                ) {
                    // 存活 slice：targetRange/range 改成 newRange，layout 改成 newLayout
                    // #708 评论 5727440517：传入 oldSubRange 用于计算 surviving slice 的正确屏幕位置
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
                    // 被删除 slice：从旧 unit 当前可见 alpha/position 转 handoff ghost
                    rebasedUnits.add(toHandoffGhost(unit, slice.oldSubRange, childKey))
                    ghostedCoverage.add(slice.oldSubRange)
                }
            }
        }

        return RebasedHandoff(
            units = rebasedUnits,
            ghostedCoverage = ghostedCoverage,
        )
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
     * alpha/position 固定在当前可见值（不瞬移、不推进时间）。
     *
     * **与 [ComposeVisualTimeline.mapSurvivingSlice] 的对应关系**（#708 评论 5725706551）：
     * - timeline 版本：alpha 通道不变（继续原动画），position 通道在新位置变化时创建
     *   TimedOffset(from=oldPosition, to=newPosition, startedAtNanos=frameTimeNanos, durationNanos)。
     * - handoff 版本（本方法）：alpha/position 都固定在当前可见值（TimedFloat(from, from, 0, 0)），
     *   不推进时间，等 timeline 接管后再创建动画通道。
     *
     * 旧 scene 的 units 已经 sampled 过，alpha.from = 当前可见 alpha，position.from = 当前屏幕位置。
     * handoff 把 alpha/position 固定在当前值（TimedFloat(from, from, 0, 0)），
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
        )
    }

    /**
     * 被删除 slice → handoff ghost —
     * 从旧 unit 当前可见 alpha/position 转 ghost（不新建 alpha=1 的完整 ghost）。
     *
     * **与 [ComposeVisualTimeline.toGhost] 的对应关系**（#708 评论 5725706551）：
     * - timeline 版本：alpha 从当前值继续到 0（TimedFloat(alphaNow, 0f, now, durationNanos)），
     *   position 固定在当前屏幕位置（考虑切片 ghost 的父 unit 位移）。
     * - handoff 版本（本方法）：alpha/position 都固定在当前可见值（TimedFloat(from, from, 0, 0)），
     *   不推进时间，等 timeline 接管后再创建 alpha → 0 的淡出通道。
     *
     * 关键约束（#708 评论 5725706551）：
     * handoff ghost 继承当前可见 alpha — 不能新建 alpha=1 的完整 ghost 来替代正在动画中的 unit。
     * 如果旧 unit 正在做 alpha 0→1 的插入动画（当前 alpha=0.5），handoff ghost 应保持 alpha=0.5，
     * 而不是新建 alpha=1 的完整 ghost。否则会出现"旧字残留 + 新字同时画"的重影。
     *
     * targetRange = null，role = DeletedGhost。
     * alpha/position 固定在当前可见值，等 timeline 接管后再创建 alpha → 0 的淡出通道。
     */
    private fun toHandoffGhost(
        unit: VisualTextUnit,
        ghostRange: TextRange,
        childKey: Long,
    ): VisualTextUnit {
        val frozenAlpha = TimedFloat(unit.alpha.from, unit.alpha.from, 0L, 0L)
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
     */
    data class RebasedHandoff(
        val units: List<VisualTextUnit>,
        val ghostedCoverage: List<TextRange>,
    )
}
