package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
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
     * @return rebased handoff — units 已映射到新坐标系，ghostedCoverage 记录已转 ghost 的旧正文范围，
     *   initialClipFractionsByKey 记录每个 child 的真实首帧 fraction。
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
        // #708 评论 5731952690 修复3：改名为 initialClipFractionsByKey —
        // 语义从"child 继承 parent 的旧 fraction"改成"每个 rebase child 的真实首帧 fraction"。
        // ghost 首帧继承这个具体 glyph/slice 上一帧真实可见多少，而不是一刀切 0 或 parent 进度。
        val initialClipFractionsByKey = mutableMapOf<Long, Float>()
        // #708 评论 5734842845：记录每个 rebase child 的 clip driver cursor ownership —
        // parent 有 scene.unitClipCursors[parentKey] 时，所有由这个 parent 派生的 child key
        // 都记录同一份 parent clip cursor；历史 ghost 按原 key 复制旧 cursor；
        // 非 split、key 不变的 active unit 同样保留；parent 本来没有 clip cursor 时不要造。
        // publishLocalHandoffScene 用此 map 设 handoff 首帧 unitClipCursors，
        // 不再沿用旧 scene.unitClipCursors — 否则下一次 rebase 处理 child 时
        // parentOldCursorRect = scene.unitClipCursors[childKey] 返回 null，
        // computeSliceInitialFraction 走 `if (parentOldCursorRect == null) return parentOldFraction`
        // 分支，front/ghost/back 全部继承同一个 parentOldFraction。
        val initialClipCursorsByKey = mutableMapOf<Long, Rect>()
        // #708 评论 5733321056 修复3：不再用全局 scene.cursorRect 独立变量 —
        // 改为在 for (unit in scene.units) 循环内对每个 unit 取 parent 自己的
        // scene.unitClipCursors[unit.key]。scene.cursorRect 只表示屏幕最新视觉光标，
        // 可能已切到下一笔的 cursorTrack，用它算 split child 首帧 fraction 会与
        // timeline 用 per-unit clipTrackId 算的结果不一致 = 首帧跳变。
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
                // #708 评论 5734842845：历史 ghost 沿用旧 clip cursor ownership
                scene.unitClipCursors[unit.key]?.let { cursor ->
                    initialClipCursorsByKey[unit.key] = cursor
                }
                continue
            }
            // 旧 active unit：通过 splitMappedRangeForward 映射到新正文坐标
            val slices = computeSlices(target, patch, newTextLength)
            val isSplit = slices.size >= 2
            val parentOldFraction = scene.unitClipFractions[unit.key]
            // #708 评论 5733321056 修复3：parent unit 自己的 clip cursor —
            // 从 scene.unitClipCursors 取（由 timeline.computeUnitClipFractions 记录）。
            // parent 不在 map 中表示它本来就不是 spatial clip 驱动（无 clipTrackId 或 track 已清），
            // 不要凭空造一个 cursor。
            val parentOldCursorRect = scene.unitClipCursors[unit.key]
            for (slice in slices) {
                val childKey = if (isSplit) nextChildKey() else unit.key
                // #708 评论 5731952690 修复3：记录每个 rebase child 的真实首帧 fraction —
                // 逻辑抽取到 computeSliceInitialFraction helper，降低 rebase() 复杂度。
                // 返回 null 表示不需要记录（非 split 且 parentOldFraction==null）。
                computeSliceInitialFraction(
                    unit = unit,
                    slice = slice,
                    isSplit = isSplit,
                    parentOldFraction = parentOldFraction,
                    parentOldCursorRect = parentOldCursorRect,
                    oldCoordinated = oldCoordinated,
                )?.let { initialClipFractionsByKey[childKey] = it }
                // #708 评论 5734842845：记录 child clip cursor ownership —
                // parent 有 scene.unitClipCursors[parentKey] 时，所有由这个 parent 派生的 child key
                // （split surviving / split ghost / 非 split key 不变的 active unit）
                // 都记录同一份 parent clip cursor；parent 本来没有 clip cursor 时不写 entry。
                // 这样下一次 rebase 处理 child 时 parentOldCursorRect = scene.unitClipCursors[childKey]
                // 能拿到这份 cursor，computeSliceInitialFraction 走精确算分支而非 fallback。
                parentOldCursorRect?.let { initialClipCursorsByKey[childKey] = it }
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
            initialClipCursorsByKey = initialClipCursorsByKey,
        )
    }

    /**
     * #708 评论 5731952690 修复3 / 评论 5733321056 修复3：计算 rebase child 的真实首帧 fraction —
     * 从 rebase() 抽取以降低复杂度。
     *
     * 返回值语义：
     * - null：不需要记录（非 split 且 parentOldFraction==null）
     * - 非 null：要记录的 fraction
     *
     * 四种情况：
     * - 非 split（整个 unit 变 ghost 或 surviving）：继承 parent 旧 fraction
     * - split surviving slice：用 slice.oldSubRange + sliceScreenPosition + parentOldCursorRect
     *   + fractionFor 算自己真实首帧 fraction（role 保持原 unit.role，不是 DeletedGhost）。
     *   旧实现直接返回 parentOldFraction，导致 front/back surviving 都拿到 parent 整体进度，
     *   首帧画错（cursor 已越过 front 但 front fraction=0.5）。
     * - split ghost slice 有 parent cursor：用 fractionFor 精确算，
     *   safePathBounds 零宽时用 text-offset heuristic fallback。
     * - split ghost slice 无 parent cursor：parent 旧 fraction
     *
     * #708 评论 5733321056 修复3：parentOldCursorRect 从 scene.unitClipCursors[unit.key] 取，
     * 不再用全局 scene.cursorRect — parent 没有 unitClipCursor 时表示它本来就不是 spatial clip 驱动，
     * 不要凭空造一个。
     */
    private fun computeSliceInitialFraction(
        unit: VisualTextUnit,
        slice: ComposeVisualRebase.MappedRangeSlice,
        isSplit: Boolean,
        parentOldFraction: Float?,
        parentOldCursorRect: Rect?,
        oldCoordinated: Boolean,
    ): Float? {
        if (!isSplit) {
            // 非 split：整个 unit 变 ghost 或 surviving，继承 parent 旧 fraction
            return parentOldFraction
        }
        val isGhostSlice = slice.kind == ComposeVisualRebase.MappedRangeSliceKind.GHOST
        if (parentOldCursorRect == null) {
            // parent 没有 unitClipCursor — 它本来就不是 spatial clip 驱动，
            // 不要凭空造一个 cursor。继承 parent 旧 fraction（可能为 null）。
            return parentOldFraction
        }
        // #708 评论 5733321056 修复3：split surviving slice 也用 fractionFor 算 —
        // 旧实现对 surviving slice 直接返回 parentOldFraction，front/back 都拿到 parent 整体进度。
        // 现在统一用 slice.oldSubRange + sliceScreenPosition + parentOldCursorRect + fractionFor
        // 算 slice 自己在 parent cursor 下的真实可见度。
        // surviving slice 的 role 保持原 unit.role（Inserted / RetainedMove / ReflowMove），
        // 不是 DeletedGhost — 这样 fractionFor 走 insert/retained 分支，符合"cursor 越过 = 已吐完"语义。
        // ghost slice 的 role 改 DeletedGhost，走 delete 分支。
        return computeSliceFractionWithParentCursor(
            unit = unit,
            slice = slice,
            isGhostSlice = isGhostSlice,
            parentOldFraction = parentOldFraction,
            parentOldCursorRect = parentOldCursorRect,
            oldCoordinated = oldCoordinated,
        )
    }

    /**
     * #708 评论 5733321056 修复3：用 parent 自己的 unitClipCursor + fractionFor 算 split slice
     * （surviving 或 ghost）的真实首帧 fraction — 从 computeSliceInitialFraction 抽取以降低复杂度。
     *
     * 两条路径：
     * 1. safePathBounds 正常 → fractionFor 精确算
     * 2. safePathBounds 零宽（测试环境 getPathForRange 限制）→ text-offset heuristic fallback
     *
     * surviving slice 的 role 保持原 unit.role（不是 DeletedGhost）；
     * ghost slice 的 role 改 DeletedGhost。
     */
    private fun computeSliceFractionWithParentCursor(
        unit: VisualTextUnit,
        slice: ComposeVisualRebase.MappedRangeSlice,
        isGhostSlice: Boolean,
        parentOldFraction: Float?,
        parentOldCursorRect: Rect,
        oldCoordinated: Boolean,
    ): Float {
        // 构造临时 slice unit 算 slice 自己在旧 scene 下的真实 fraction。
        // unit.alpha.from 已是上一帧可见 alpha（scene.units 已 sampled）。
        // sliceUnit 的 position 必须是 slice 自己的屏幕位置，不是 parent 的 position。
        val sliceScreenPos =
            ComposeVisualRebase.sliceScreenPosition(
                layout = unit.layout,
                parentRange = unit.range,
                sliceRange = slice.oldSubRange,
                parentScreenPosition = unit.position.from,
            ) ?: unit.position.from
        // #708 评论 5733321056 修复3：surviving slice 保持原 role（Inserted / RetainedMove / ReflowMove），
        // ghost slice 改 DeletedGhost — fractionFor 据此走 insert/delete 分支。
        val sliceRole = if (isGhostSlice) VisualUnitRole.DeletedGhost else unit.role
        val sliceUnit =
            unit.copy(
                range = slice.oldSubRange,
                role = sliceRole,
                position = TimedOffset(sliceScreenPos, sliceScreenPos, 0L, 0L),
            )
        val naturalBounds =
            ComposeVisualRebase.safePathBounds(
                sliceUnit.layout.result,
                sliceUnit.range,
            )
        val parentBounds =
            ComposeVisualRebase.safePathBounds(unit.layout.result, unit.range)
        // #708 评论 5733321056 修复3：fallback 策略抽取到 chooseSliceFractionBranch —
        // 降低本方法圈复杂度。
        return chooseSliceFractionBranch(
            sliceUnit = sliceUnit,
            unit = unit,
            slice = slice,
            isGhostSlice = isGhostSlice,
            naturalBounds = naturalBounds,
            parentWidth = parentBounds?.width ?: 0f,
            parentOldFraction = parentOldFraction,
            parentOldCursorRect = parentOldCursorRect,
            oldCoordinated = oldCoordinated,
        )
    }

    /**
     * #708 评论 5733321056 修复3：slice fraction fallback 策略 —
     * 从 computeSliceFractionWithParentCursor 抽取以降低圈复杂度。
     *
     * 三条路径：
     * 1. 零宽 glyph + ghost slice：用 fractionFor 算（零宽 DeletedGhost 用 cursor 位置判断），
     *    与 timeline 的 fractionFor 零宽逻辑一致，避免首帧跳变。
     * 2. 零宽/不精确 bounds + surviving slice：走 text-offset heuristic fallback
     *    （基于文本偏移，不依赖 getPathForRange 几何精度）。
     * 3. 精确非零宽：走 fractionFor 精确算。
     *
     * 不精确 bounds：slice 是 parent 真子区间但 naturalBounds.width 接近 parent width，
     * Robolectric getPathForRange 对子 range 可能返回整个 parent 的 bounds。
     */
    @Suppress("CyclomaticComplexMethod", "LongParameterList")
    private fun chooseSliceFractionBranch(
        sliceUnit: VisualTextUnit,
        unit: VisualTextUnit,
        slice: ComposeVisualRebase.MappedRangeSlice,
        isGhostSlice: Boolean,
        naturalBounds: Rect?,
        parentWidth: Float,
        parentOldFraction: Float?,
        parentOldCursorRect: Rect,
        oldCoordinated: Boolean,
    ): Float {
        val isZeroWidthBounds = naturalBounds == null || naturalBounds.width < 0.5f
        val isImpreciseBounds =
            slice.oldSubRange != unit.range &&
                naturalBounds != null &&
                parentWidth > 0.5f &&
                naturalBounds.width >= parentWidth * 0.9f
        val useHeuristic = !isGhostSlice && (isZeroWidthBounds || isImpreciseBounds)
        return when {
            isZeroWidthBounds && isGhostSlice -> {
                // ghost slice 零宽：用 fractionFor 算（零宽 DeletedGhost 用 cursor 位置判断），
                // 与 timeline 的 fractionFor 零宽逻辑一致。
                fractionForOrFallback(sliceUnit, parentOldCursorRect, oldCoordinated, parentOldFraction)
            }
            useHeuristic -> {
                // surviving slice 零宽/不精确：走 text-offset heuristic fallback
                computeTextOffsetHeuristicFraction(unit, slice, parentOldFraction)
            }
            else -> {
                fractionForOrFallback(sliceUnit, parentOldCursorRect, oldCoordinated, parentOldFraction)
            }
        }
    }

    /** fractionFor 算不出的 fallback — 抽取以降低 chooseSliceFractionBranch 圈复杂度。 */
    private fun fractionForOrFallback(
        sliceUnit: VisualTextUnit,
        cursorRect: Rect,
        coordinated: Boolean,
        fallback: Float?,
    ): Float = ComposeVisualClip.fractionFor(sliceUnit, cursorRect, coordinated) ?: (fallback ?: 0f)

    /**
     * #708 评论 5731952690 修复3：text-offset heuristic fallback —
     * safePathBounds 返回零宽时用 parentOldFraction 估算 cursor 在 parent 中的文本偏移，
     * 与 slice 的文本范围比较：cursor 在 slice 之前→0，之后→1，中间→线性插值。
     */
    private fun computeTextOffsetHeuristicFraction(
        unit: VisualTextUnit,
        slice: ComposeVisualRebase.MappedRangeSlice,
        parentOldFraction: Float?,
    ): Float {
        val parentLen = (unit.range.end - unit.range.start).coerceAtLeast(1)
        val cursorApproxOffset = (parentOldFraction ?: 0f) * parentLen
        val sliceStart = slice.oldSubRange.start - unit.range.start
        val sliceEnd = slice.oldSubRange.end - unit.range.start
        return when {
            cursorApproxOffset <= sliceStart -> 0f
            cursorApproxOffset >= sliceEnd -> 1f
            sliceEnd > sliceStart -> (cursorApproxOffset - sliceStart) / (sliceEnd - sliceStart)
            else -> 0f
        }
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
     * @param initialClipCursorsByKey #708 评论 5734842845：每个 rebase child 的 clip driver cursor ownership —
     *   语义为"这个 child 上一帧算 fraction 时所用的 cursor rect"：
     *   - 历史 ghost：scene.unitClipCursors（沿用旧 cursor）
     *   - 非 split、key 不变的 active unit：scene.unitClipCursors[unit.key]（沿用旧 cursor）
     *   - split 派生的 child（surviving 或 ghost）：parent 的 scene.unitClipCursors[parentKey]
     *     （所有由同一 parent 派生的 child 共享同一份 parent clip cursor）
     *   - parent 没有 scene.unitClipCursors[parentKey] 时不写 entry —
     *     表示 parent 本来就不是 spatial clip 驱动，不要凭空造一个 cursor。
     *   publishLocalHandoffScene 用此 map 设 handoff 首帧 unitClipCursors，
     *   不再沿用旧 scene.unitClipCursors — 旧 parent key 已不在 rebasedUnits 里，
     *   留下它会让 scene 的 units/fractions/cursors 三份 key 集合不一致，
     *   且下一次 rebase 处理 child 时 parentOldCursorRect = scene.unitClipCursors[childKey] 返回 null，
     *   导致 computeSliceInitialFraction 走 `if (parentOldCursorRect == null) return parentOldFraction`
     *   分支，front/ghost/back 全部继承同一个 parentOldFraction，
     *   重新出现"split child 直接复制 parent 整体 fraction"的回归。
     */
    data class RebasedHandoff(
        val units: List<VisualTextUnit>,
        val ghostedCoverage: List<TextRange>,
        val initialClipFractionsByKey: Map<Long, Float> = emptyMap(),
        val initialClipCursorsByKey: Map<Long, Rect> = emptyMap(),
    )
}
