package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import kotlin.math.max

/**
 * #689 评论 5674631257 步骤2：持续视觉时间线 —
 * 真正长期存在的屏幕动画状态。
 *
 * 不再用一条全局 `0f..1f` 控制所有字。每个仍由 overlay 接管的文字单元保存两条
 * **互不重置**的通道：透明度和位置。
 *
 * 核心不变量：
 * - 已有 unit 通过 [ComposeVisualPatch.offsetMap] 映射到新正文后仍然存活时，
 *   原来的 alpha 动画**继续使用原来的 startedAtNanos**，
 *   快速输入第二个字不能让第一个字重新从 0 开始。
 * - 如果新 layout 让它的位置发生变化，只把 position 通道从"此刻屏幕位置"重定向到新位置，
 *   position 使用新的开始时间；位置没变则 position 通道也不重建。
 * - 新插入 unit：从 `alpha 0 -> 1` 新建自己的 alpha 通道，位置直接是新 layout 的真实位置。
 * - 删除 unit：先取它此刻屏幕实际 alpha/位置，再变成 ghost unit（targetRange = null），
 *   alpha 从当前值继续到 0，位置保持当前屏幕位置。
 * - [hiddenRanges] 每一帧直接从当前 `VisualTextUnit.targetRange != null` 且仍由 overlay
 *   绘制的 unit 推导，不从"上一事务 suppressed ranges"继承。
 *
 * 这个类不是 Compose 可观察状态 — 它是纯数据状态机。
 * [ComposeEditorVisualState] 持有它并在每次 sample 后把结果同步给 Compose StateFlow。
 */
@Suppress("TooManyFunctions")
class ComposeVisualTimeline {
    /** 当前所有文字单元（存活 + ghost）。 */
    private var units: List<VisualTextUnit> = emptyList()

    /** 单调递增的 unit key — 新插入 unit 分配唯一 key。 */
    private var nextUnitKey: Long = 1L

    /**
     * #691：统一光标位置 — 由同一个 VisualScene / frame clock 维护。
     * 不再使用独立的 Animatable<Rect> + LaunchedEffect。
     *
     * #691 评论 5679242735 修改3：cursorChannel 改成 [CursorTrack]，
     * 支持多段 [CursorMotionPath]（一次提交多个插入 unit 时光标依次经过每个字/cluster）。
     */
    private var cursorChannel: CursorTrack? = null

    /**
     * 应用一个屏幕 diff — 先 sample(now) 拿到旧动画此刻屏幕真实画到的 alpha 和位置，
     * 再处理新 patch。不能从事务的 progress 反算，也不能先归零。
     *
     * #691：同时接受光标 motion 参数，在同一个调用内处理文字和光标，
     * 保证 cursor 和 text units 使用同一个 frameTimeNanos。
     *
     * #691 评论 5679242735 修改2：检查 [patch.motionPolicy.effective] 的 textEnabled —
     * 文字动画关闭时不创建任何文字 alpha/position track（units = emptyList()），
     * 不只是把 duration 改成 0。否则仍可能产生一帧 hiddenRanges/ghost 所有权问题。
     *
     * #691 评论 5679242735 修改3：cursor 参数从 `cursorToRect: Rect?` 改成
     * `cursorPath: List<CursorMotionPoint>?`，支持多段路径。
     * 一次提交多个插入 unit 时光标依次经过每个字/cluster，不再被压成一条直线。
     *
     * @param patch 这一帧的屏幕 diff — 包含 [ComposeVisualPatch.intent] 用于 fallback survival map。
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock，不用 System.nanoTime()）。
     * @param cursorFromRect 光标在旧 layout 中的位置（屏幕坐标）— null 表示无光标 motion。
     * @param cursorPath 光标运动路径点序列（屏幕坐标）— null 表示无光标 motion。
     *   单点路径：snap；多点路径：按 [CursorMotionPoint.endFraction] 分段插值。
     * @param cursorDurationNanos 光标动画时长 — 0 表示瞬时 snap。
     */
    fun applyPatch(
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        cursorFromRect: Rect? = null,
        cursorPath: List<CursorMotionPoint>? = null,
        cursorDurationNanos: Long = 0L,
    ) {
        // #691 评论 5679242735 修改2 / 设置语义 G：文字动画时长以用户设置 textDurationMillis 为唯一事实来源，
        // 不再用 Core intent 的 patch.durationMs（那样用户改时长设置不生效）。
        val policy = patch.motionPolicy.effective()
        val durationNanos = policy.textDurationMillis.coerceAtLeast(0L) * NANOS_PER_MS

        // #691 评论 5679242735 修改2：textEnabled=false 时不创建任何文字 alpha/position track。
        // 不要只把 duration 改成 0 — 否则仍可能产生一帧 hiddenRanges/ghost 所有权问题。
        if (policy.textEnabled) {
            // 第一步：先 sample 当前所有 unit 到此刻的真实 alpha/位置。
            val sampledUnits = units.map { sampleUnit(it, frameTimeNanos) }

            // 第二步：把存活 unit 通过 offsetMap 映射到新正文（缺陷5 切片）。
            val surviving = mutableListOf<VisualTextUnit>()
            val ghosting = mutableListOf<VisualTextUnit>()
            mapSurvivingUnits(sampledUnits, patch, frameTimeNanos, durationNanos, surviving, ghosting)

            // 第三步：处理本 patch 新插入的 unit。
            val inserted = createInsertedUnits(patch, frameTimeNanos, durationNanos)

            // 第四步：处理本 patch 显式删除的 unit（缺陷1 从 oldLayout 建 ghost）。
            createDeletedGhosts(patch, frameTimeNanos, durationNanos, sampledUnits, ghosting)

            // 第五步：retainedMoves（缺陷4 临时接管回流文字）。
            applyRetainedMoves(patch, frameTimeNanos, durationNanos, surviving)

            // 合并：存活 + 新插入 + ghost
            units = surviving + inserted + ghosting
        } else {
            // 文字动画关闭：不创建任何文字 alpha/position track。
            units = emptyList()
        }

        // #691 评论 5679242735 修改3：光标 motion 并入 timeline — 与文字共享同一个 frameTimeNanos。
        // cursor 独立按 policy.cursorEnabled 继续处理（由调用方 computeCursorParamsForPatch 决定是否传参）。
        // 支持多段路径：cursorPath 是 List<CursorMotionPoint>，不再只取 last().rect。
        if (cursorFromRect != null && cursorPath != null && cursorPath.isNotEmpty()) {
            val current = cursorChannel
            val startRect =
                if (current != null) {
                    sampleCursorRect(frameTimeNanos) ?: cursorFromRect
                } else {
                    cursorFromRect
                }
            cursorChannel =
                CursorTrack(
                    fromRect = startRect,
                    points = cursorPath,
                    startedAtNanos = frameTimeNanos,
                    durationNanos = cursorDurationNanos,
                )
        }
    }

    /**
     * #689 评论 5675270164 缺陷5：把存活 unit 通过 offsetMap 映射到新正文。
     * 用 splitMappedRangeForward 切片，不整块判死。
     */
    @Suppress("LongMethod", "CyclomaticComplexMethod", "CognitiveComplexMethod")
    private fun mapSurvivingUnits(
        sampledUnits: List<VisualTextUnit>,
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        durationNanos: Long,
        surviving: MutableList<VisualTextUnit>,
        ghosting: MutableList<VisualTextUnit>,
    ) {
        val offsetMap = patch.offsetMap
        val newLayout = patch.newLayout
        val newTextLength = newLayout.result.layoutInput.text.length
        for (unit in sampledUnits) {
            val target = unit.targetRange
            if (target == null) {
                // 已经是 ghost：继续淡出。alpha 已到 0 的丢弃。
                if (currentAlpha(unit.alpha, frameTimeNanos) > 0f) {
                    ghosting.add(unit)
                }
                continue
            }
            val slices = computeSlices(target, offsetMap, newTextLength, patch.intent)
            for (slice in slices) {
                if (slice.kind == ComposeVisualRebase.MappedRangeSliceKind.SURVIVING && slice.newSubRange != null) {
                    surviving.add(mapSurvivingSlice(unit, slice.newSubRange, newLayout, frameTimeNanos, durationNanos))
                } else {
                    // 缺陷5 GHOST slice：按 slice.oldSubRange 创建 ghost，alpha 当前值 -> 0
                    ghosting.add(toGhost(unit, frameTimeNanos, durationNanos, slice.oldSubRange))
                }
            }
        }
    }

    private fun computeSlices(
        target: TextRange,
        offsetMap: List<VisualOffsetMapEntry>?,
        newTextLength: Int,
        intent: EditorVisualIntent? = null,
    ): List<ComposeVisualRebase.MappedRangeSlice> {
        val effectiveMap =
            offsetMap ?: intent?.let { ComposeVisualRebase.entriesForIntent(it) }
        if (effectiveMap != null) {
            return ComposeVisualRebase.splitMappedRangeForward(target, effectiveMap)
        }
        // 无 offset map 且无 intent 信息：若 target 仍在新正文范围内，保留；否则转 ghost
        return if (target.end <= newTextLength) {
            listOf(
                ComposeVisualRebase.MappedRangeSlice(
                    oldSubRange = target,
                    newSubRange = target,
                    kind = ComposeVisualRebase.MappedRangeSliceKind.SURVIVING,
                ),
            )
        } else {
            listOf(
                ComposeVisualRebase.MappedRangeSlice(
                    oldSubRange = target,
                    newSubRange = null,
                    kind = ComposeVisualRebase.MappedRangeSliceKind.GHOST,
                ),
            )
        }
    }

    private fun mapSurvivingSlice(
        unit: VisualTextUnit,
        mappedRange: TextRange,
        newLayout: ComposeLayoutSnapshot,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): VisualTextUnit {
        // 存活：alpha 通道不变（继续使用原 startedAtNanos）。
        // position 通道：只在新 layout 让位置发生变化时重定向。
        val newPosition = computeUnitPosition(newLayout, mappedRange)
        val oldPosition = currentOffset(unit.position, frameTimeNanos)
        val positionChannel =
            if (newPosition != null && oldPosition != null && newPosition != oldPosition) {
                TimedOffset(
                    from = oldPosition,
                    to = newPosition,
                    startedAtNanos = frameTimeNanos,
                    durationNanos = durationNanos,
                )
            } else {
                unit.position
            }
        return unit.copy(
            layout = newLayout,
            range = mappedRange,
            targetRange = mappedRange,
            position = positionChannel,
        )
    }

    /**
     * 第三步：处理本 patch 新插入的 unit。
     */
    private fun createInsertedUnits(
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): List<VisualTextUnit> {
        val newLayout = patch.newLayout
        val newTextLength = newLayout.result.layoutInput.text.length
        val inserted = mutableListOf<VisualTextUnit>()
        for (range in patch.insertedUnits) {
            if (range.start >= range.end) continue
            if (range.end > newTextLength) continue
            val position = computeUnitPosition(newLayout, range) ?: Offset.Zero
            inserted.add(
                VisualTextUnit(
                    key = nextUnitKey++,
                    layout = newLayout,
                    range = range,
                    targetRange = range,
                    alpha = TimedFloat(0f, 1f, frameTimeNanos, durationNanos),
                    position = TimedOffset(position, position, frameTimeNanos, 0L),
                ),
            )
        }
        return inserted
    }

    /**
     * #689 评论 5675270164 缺陷1：处理删除 unit。
     * 找不到 active unit 时从 patch.oldLayout 建 ghost（alpha 1->0）。
     */
    private fun createDeletedGhosts(
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        durationNanos: Long,
        sampledUnits: List<VisualTextUnit>,
        ghosting: MutableList<VisualTextUnit>,
    ) {
        val oldTextLength = patch.oldLayout.result.layoutInput.text.length
        for (range in patch.deletedUnits) {
            if (range.start >= range.end) continue
            if (range.end > oldTextLength) continue
            // 检查 ghosting 里是否已有覆盖此 range 的 ghost（存活映射阶段已切片处理）
            if (ghosting.any { it.range == range }) continue
            // 检查 sampledUnits 里是否有 active unit 覆盖此 range（已在存活映射阶段处理）
            if (sampledUnits.any { it.targetRange != null && it.range == range }) continue
            // 缺陷1：从 oldLayout 建 ghost（alpha 1->0）
            val oldPosition = computeUnitPosition(patch.oldLayout, range) ?: continue
            ghosting +=
                VisualTextUnit(
                    key = nextUnitKey++,
                    layout = patch.oldLayout,
                    range = range,
                    targetRange = null,
                    alpha = TimedFloat(1f, 0f, frameTimeNanos, durationNanos),
                    position = TimedOffset(oldPosition, oldPosition, frameTimeNanos, 0L),
                )
        }
    }

    /**
     * #689 评论 5675270164 缺陷4：retainedMoves 不能依赖 unit 事先存在。
     * 匹配不到 active unit 时从 patch.oldLayout + move.oldRange 创建 move unit。
     */
    private fun applyRetainedMoves(
        patch: ComposeVisualPatch,
        frameTimeNanos: Long,
        durationNanos: Long,
        surviving: MutableList<VisualTextUnit>,
    ) {
        val newLayout = patch.newLayout
        val newTextLength = newLayout.result.layoutInput.text.length
        for (move in patch.retainedMoves) {
            val newRange = move.newRange
            if (newRange.start >= newRange.end) continue
            if (newRange.end > newTextLength) continue
            val idx = surviving.indexOfFirst { it.targetRange == newRange }
            if (idx >= 0) {
                redirectExistingMoveUnit(surviving, idx, newRange, newLayout, frameTimeNanos, durationNanos)
            } else {
                createMoveUnitForReflow(patch, move, frameTimeNanos, durationNanos, surviving)
            }
        }
    }

    private fun redirectExistingMoveUnit(
        surviving: MutableList<VisualTextUnit>,
        idx: Int,
        newRange: TextRange,
        newLayout: ComposeLayoutSnapshot,
        frameTimeNanos: Long,
        durationNanos: Long,
    ) {
        val unit = surviving[idx]
        val newPosition = computeUnitPosition(newLayout, newRange) ?: return
        val oldPosition = currentOffset(unit.position, frameTimeNanos) ?: newPosition
        // 只有位置真变了才重定向（删换行时几何没变的文字不产生 position track）
        if (newPosition != oldPosition) {
            surviving[idx] =
                unit.copy(
                    position = TimedOffset(oldPosition, newPosition, frameTimeNanos, durationNanos),
                )
        }
    }

    private fun createMoveUnitForReflow(
        patch: ComposeVisualPatch,
        move: RetainedMove,
        frameTimeNanos: Long,
        durationNanos: Long,
        surviving: MutableList<VisualTextUnit>,
    ) {
        // 缺陷4：匹配不到 active unit，从 oldLayout + move.oldRange 创建 move unit
        val newRange = move.newRange
        val newLayout = patch.newLayout
        val oldPosition = computeUnitPosition(patch.oldLayout, move.oldRange) ?: return
        val newPosition = computeUnitPosition(newLayout, newRange) ?: return
        // 只有位置真变了才创建 move unit
        if (newPosition == oldPosition) return
        surviving +=
            VisualTextUnit(
                key = nextUnitKey++,
                layout = newLayout,
                range = newRange,
                targetRange = newRange,
                alpha = TimedFloat(1f, 1f, frameTimeNanos, 0L),
                position = TimedOffset(oldPosition, newPosition, frameTimeNanos, durationNanos),
            )
    }

    /**
     * 采样当前时间线到指定帧时间 — 返回当前应绘制的 [ComposeVisualScene]。
     *
     * #689 评论 5675270164 缺陷3：sample 后做收口 — 持续 timeline 只保存"当前仍需要
     * overlay 接管的东西"。
     * - 存活 unit（targetRange != null）：alpha==1 且 position 已到目标 -> 从 timeline 移除，交还 BasicTextField
     * - ghost unit（targetRange == null）：alpha==0 -> 删除
     * - 仍在 alpha/position 动画中的 unit -> 保留
     *
     * 收口要修改 timeline 内部的 units 列表（移除已稳定的 unit），不只是过滤返回值。
     * 否则 units 里残留的 unit 会在下次 applyPatch 时被处理，可能导致问题。
     *
     * @param frameTimeNanos 当前帧时间戳。
     * @return 当前场景（units + hiddenRanges）。
     */
    fun sample(frameTimeNanos: Long): ComposeVisualScene {
        // 缺陷3：收口 — 移除已稳定的 unit，只保留仍需 overlay 接管的 unit
        val remainingUnits = mutableListOf<VisualTextUnit>()
        val sampledUnits = mutableListOf<VisualTextUnit>()
        for (unit in units) {
            val sampled = sampleUnit(unit, frameTimeNanos)
            val target = sampled.targetRange
            val alphaFinished = isAlphaFinished(sampled.alpha, frameTimeNanos)
            val positionFinished = isPositionFinished(sampled.position, frameTimeNanos)
            if (target != null) {
                // 存活 unit：alpha==1 且 position 已到目标 -> 从 timeline 移除，交还 BasicTextField
                if (alphaFinished && positionFinished && sampled.alpha.to >= 1f) {
                    continue
                }
            } else {
                // ghost unit：alpha==0 -> 删除
                if (alphaFinished && sampled.alpha.to <= 0f) {
                    continue
                }
            }
            sampledUnits.add(sampled)
            remainingUnits.add(unit)
        }
        // 缺陷3：收口要修改 timeline 内部 units 列表（移除已稳定的 unit）
        units = remainingUnits
        // hiddenRanges：从当前 targetRange != null 且仍由 overlay 绘制的 unit 推导。
        // 收口后 sampledUnits 里的存活 unit 都是"仍由 overlay 接管"的（未稳定的），
        // 所以它们的 targetRange 都应在 hiddenRanges 中。
        val hiddenRanges =
            sampledUnits
                .filter { it.targetRange != null }
                .mapNotNull { it.targetRange }
                .filter { it.start < it.end }
        // #691：采样光标位置 — 与文字使用同一个 frameTimeNanos
        val sampledCursor = sampleCursorRect(frameTimeNanos)
        return ComposeVisualScene(units = sampledUnits, hiddenRanges = hiddenRanges, cursorRect = sampledCursor)
    }

    /**
     * 是否还有活动动画 — overlay 据此决定是否继续推进帧时钟。
     *
     * #691：同时检查文字 units 和光标 cursorChannel 的活动状态。
     *
     * @param frameTimeNanos 当前帧时间戳。
     * @return true 表示还有 unit 的 alpha 或 position 通道未完成，或光标动画未完成。
     */
    fun hasActiveAnimation(frameTimeNanos: Long): Boolean {
        val textActive =
            units.any { unit ->
                !isAlphaFinished(unit.alpha, frameTimeNanos) ||
                    !isPositionFinished(unit.position, frameTimeNanos)
            }
        if (textActive) return true
        // #691：光标动画也算活动状态
        return hasActiveCursorAnimation(frameTimeNanos)
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        units = emptyList()
        nextUnitKey = 1L
        cursorChannel = null
    }

    /**
     * #691 评论 5679242735 修改2：运行时 policy 切换时清掉旧 text units / ghost / cursorChannel。
     *
     * 由 [ComposeEditorVisualState.applyMotionPolicyAtFrame] 调用 —
     * 用户在动画进行中关闭文字动画或打开 reduce-motion 时，
     * 旧 patch 已带着原来的 insertedUnits/deletedUnits/retainedMoves 入队，
     * drain 时会再把文字动画重新启动。本方法把当前 timeline 里所有活动文字/光标动画清空，
     * 让后续 drain 用新 policy 重新决定是否创建 track。
     */
    fun settleForPolicyChange() {
        units = emptyList()
        cursorChannel = null
    }

    // ==================== 统一光标位置（#691） ====================

    /**
     * #691：snapshot 光标到当前帧时间 — 返回当前位置。
     * 如果光标动画已完成，返回最终位置。
     *
     * #691 评论 5679242735 修改3：支持多段 [CursorTrack] 路径插值。
     */
    fun sampleCursorRect(frameTimeNanos: Long): Rect? {
        val ch = cursorChannel ?: return null
        return currentRect(ch, frameTimeNanos)
    }

    /**
     * #691：光标动画是否仍在进行。
     *
     * #691 评论 5679242735 修改3：基于 [CursorTrack] 判断。
     */
    fun hasActiveCursorAnimation(frameTimeNanos: Long): Boolean {
        val ch = cursorChannel ?: return false
        return !isCursorFinished(ch, frameTimeNanos)
    }

    /**
     * #691 评论 5679242735 修改3：计算 [CursorTrack] 在指定帧时间的当前位置。
     *
     * 多段路径插值：按 [CursorMotionPoint.endFraction] 把整条 timeline 分成多段，
     * 每段在前一段终点和本段目标点之间线性插值。
     * 单点路径退化为 from -> points[0].rect 的线性插值。
     */
    private fun currentRect(
        channel: CursorTrack,
        frameTimeNanos: Long,
    ): Rect {
        if (channel.durationNanos <= 0L) return channel.points.last().rect
        val elapsed = frameTimeNanos - channel.startedAtNanos
        if (elapsed <= 0L) return channel.fromRect
        if (elapsed >= channel.durationNanos) return channel.points.last().rect
        val progress = elapsed.toFloat() / channel.durationNanos.toFloat()
        val points = channel.points
        if (points.size == 1) {
            return interpolateRect(channel.fromRect, points[0].rect, progress)
        }
        var prevRect = channel.fromRect
        var prevFraction = 0f
        for (i in points.indices) {
            val point = points[i]
            if (progress <= point.endFraction || i == points.size - 1) {
                val segmentProgress =
                    if (point.endFraction > prevFraction) {
                        ((progress - prevFraction) / (point.endFraction - prevFraction)).coerceIn(0f, 1f)
                    } else {
                        1f
                    }
                return interpolateRect(prevRect, point.rect, segmentProgress)
            }
            prevRect = point.rect
            prevFraction = point.endFraction
        }
        return points.last().rect
    }

    /**
     * #691 评论 5679242735 修改3：两个 Rect 之间的线性插值。
     */
    private fun interpolateRect(
        from: Rect,
        to: Rect,
        t: Float,
    ): Rect =
        Rect(
            left = from.left + (to.left - from.left) * t,
            top = from.top + (to.top - from.top) * t,
            right = from.right + (to.right - from.right) * t,
            bottom = from.bottom + (to.bottom - from.bottom) * t,
        )

    /**
     * #691 评论 5679242735 修改3：[CursorTrack] 是否已完成。
     */
    private fun isCursorFinished(
        channel: CursorTrack,
        frameTimeNanos: Long,
    ): Boolean {
        if (channel.durationNanos <= 0L) return true
        return frameTimeNanos - channel.startedAtNanos >= channel.durationNanos
    }

    // ==================== 内部采样与通道计算 ====================

    /**
     * #689 评论 5675270164 缺陷2：把 unit 转成 ghost — alpha 从当前值继续到 0。
     *
     * 旧实现 `unit.copy(targetRange = null)` 只把 targetRange 设 null，没把 alpha 改成
     * "当前值 -> 0"。如果 unit 原来正在做插入动画（alpha 0->1），快速输入后马上删除，
     * 它会变成 ghost 继续淡入到 1，alpha 到 1 后 hasActiveAnimation 认为完成但 units 里
     * 没删掉，overlay 继续以 alpha=1 画在旧位置。
     *
     * @param unit 要转 ghost 的 unit。
     * @param now 当前帧时间。
     * @param durationNanos ghost 淡出时长。
     * @param ghostRange ghost 的 range；默认 unit.range（整个 unit 变 ghost）。
     *   切片场景传入 slice.oldSubRange（unit 的一部分变 ghost）。
     */
    private fun toGhost(
        unit: VisualTextUnit,
        now: Long,
        durationNanos: Long,
        ghostRange: TextRange = unit.range,
    ): VisualTextUnit {
        val alphaNow = currentAlpha(unit.alpha, now)
        val positionNow =
            if (ghostRange == unit.range) {
                currentOffset(unit.position, now) ?: unit.position.to
            } else {
                // 切片 ghost：如果父 unit 正在移动，切片应继承父 unit 当前的屏幕位移量
                val parentCurrent = currentOffset(unit.position, now) ?: unit.position.to
                val parentNatural = computeUnitPosition(unit.layout, unit.range) ?: parentCurrent
                val parentDelta = Offset(parentCurrent.x - parentNatural.x, parentCurrent.y - parentNatural.y)
                val sliceNatural = computeUnitPosition(unit.layout, ghostRange) ?: parentCurrent
                Offset(sliceNatural.x + parentDelta.x, sliceNatural.y + parentDelta.y)
            }
        return unit.copy(
            range = ghostRange,
            targetRange = null,
            alpha = TimedFloat(alphaNow, 0f, now, durationNanos),
            position = TimedOffset(positionNow, positionNow, now, 0L),
        )
    }

    /**
     * 采样单个 unit 到指定帧时间 — 返回 alpha/position 已插值后的 unit。
     * 采样后的 unit 的 alpha/position 通道表示"此刻屏幕真实画到的状态"。
     */
    private fun sampleUnit(
        unit: VisualTextUnit,
        frameTimeNanos: Long,
    ): VisualTextUnit {
        return unit.copy(
            alpha =
                unit.alpha.copy(
                    from = currentAlpha(unit.alpha, frameTimeNanos),
                    to = unit.alpha.to,
                    startedAtNanos = frameTimeNanos,
                    durationNanos = remainingDurationNanos(unit.alpha, frameTimeNanos),
                ),
            position =
                unit.position.copy(
                    from = currentOffset(unit.position, frameTimeNanos) ?: unit.position.from,
                    to = unit.position.to,
                    startedAtNanos = frameTimeNanos,
                    durationNanos = remainingDurationNanos(unit.position, frameTimeNanos),
                ),
        )
    }

    /**
     * 计算通道当前值（alpha）。
     * durationNanos <= 0 表示瞬时完成，直接返回 to。
     */
    private fun currentAlpha(
        channel: TimedFloat,
        frameTimeNanos: Long,
    ): Float {
        if (channel.durationNanos <= 0L) return channel.to
        val elapsed = frameTimeNanos - channel.startedAtNanos
        if (elapsed <= 0L) return channel.from
        if (elapsed >= channel.durationNanos) return channel.to
        val t = elapsed.toFloat() / channel.durationNanos.toFloat()
        return channel.from + (channel.to - channel.from) * t
    }

    /**
     * 计算通道当前值（offset）。
     */
    private fun currentOffset(
        channel: TimedOffset,
        frameTimeNanos: Long,
    ): Offset? {
        if (channel.durationNanos <= 0L) return channel.to
        val elapsed = frameTimeNanos - channel.startedAtNanos
        if (elapsed <= 0L) return channel.from
        if (elapsed >= channel.durationNanos) return channel.to
        val t = elapsed.toFloat() / channel.durationNanos.toFloat()
        return Offset(
            channel.from.x + (channel.to.x - channel.from.x) * t,
            channel.from.y + (channel.to.y - channel.from.y) * t,
        )
    }

    /**
     * 通道是否已完成（alpha）。
     */
    private fun isAlphaFinished(
        channel: TimedFloat,
        frameTimeNanos: Long,
    ): Boolean {
        if (channel.durationNanos <= 0L) return true
        return frameTimeNanos - channel.startedAtNanos >= channel.durationNanos
    }

    /**
     * 通道是否已完成（position）。
     */
    private fun isPositionFinished(
        channel: TimedOffset,
        frameTimeNanos: Long,
    ): Boolean {
        if (channel.durationNanos <= 0L) return true
        return frameTimeNanos - channel.startedAtNanos >= channel.durationNanos
    }

    /**
     * 通道剩余 duration — 采样后用。
     */
    private fun remainingDurationNanos(
        channel: TimedFloat,
        frameTimeNanos: Long,
    ): Long {
        if (channel.durationNanos <= 0L) return 0L
        return max(0L, channel.startedAtNanos + channel.durationNanos - frameTimeNanos)
    }

    private fun remainingDurationNanos(
        channel: TimedOffset,
        frameTimeNanos: Long,
    ): Long {
        if (channel.durationNanos <= 0L) return 0L
        return max(0L, channel.startedAtNanos + channel.durationNanos - frameTimeNanos)
    }

    /**
     * 从 layout 取 unit 的真实位置（左上角）。
     */
    private fun computeUnitPosition(
        layout: ComposeLayoutSnapshot,
        range: TextRange,
    ): Offset? {
        val bounds = ComposeVisualRebase.safePathBounds(layout.result, range) ?: return null
        return Offset(bounds.left, bounds.top)
    }

    companion object {
        /** 1 ms = 1_000_000 ns。 */
        private const val NANOS_PER_MS = 1_000_000L
    }
}

/**
 * #689 评论 5674631257 步骤2：带时间戳的 float 通道 —
 * 从 [from] 到 [to]，从 [startedAtNanos] 开始，持续 [durationNanos]。
 *
 * 通道创建后 startedAtNanos 不被重置（除非位置真变了重建 position 通道）。
 * 快速输入第二个字不能让第一个字重新从 0 开始。
 */
data class TimedFloat(
    val from: Float,
    val to: Float,
    val startedAtNanos: Long,
    val durationNanos: Long,
)

/**
 * #689 评论 5674631257 步骤2：带时间戳的 offset 通道。
 */
data class TimedOffset(
    val from: Offset,
    val to: Offset,
    val startedAtNanos: Long,
    val durationNanos: Long,
)

/**
 * #689 评论 5674631257 步骤2：单个文字单元的持续视觉状态。
 *
 * @param key 唯一标识 — 快速输入时不重置。
 * @param layout 当前所属 layout 快照。
 * @param range 在 [layout] 中的 UTF-16 range。
 * @param targetRange 在当前 new text 中的目标 range —
 *   null = ghost unit（只属于旧画面，最终应消失）；
 *   非 null = 仍存活，overlay 绘制时用此 range。
 * @param alpha 透明度通道 — 互不重置。
 * @param position 位置通道 — 互不重置；位置没变不重建。
 */
data class VisualTextUnit(
    val key: Long,
    val layout: ComposeLayoutSnapshot,
    val range: TextRange,
    val targetRange: TextRange?,
    val alpha: TimedFloat,
    val position: TimedOffset,
)

/**
 * #691：带时间戳的 Rect 通道 —
 * 从 [from] 到 [to]，从 [startedAtNanos] 开始，持续 [durationNanos]。
 *
 * 用于光标位置动画，与文字 timeline 共享同一个 frame clock。
 *
 * #691 评论 5679242735 修改3：cursor 不再用本类，改用 [CursorTrack] 支持多段路径。
 * 保留本数据类以兼容可能的其他引用。
 */
data class TimedRect(
    val from: Rect,
    val to: Rect,
    val startedAtNanos: Long,
    val durationNanos: Long,
)

/**
 * #691 评论 5679242735 修改3：带时间戳的多段光标路径通道 —
 * 从 [fromRect] 出发，依次经过 [points] 中每个 [CursorMotionPoint]，
 * 从 [startedAtNanos] 开始，持续 [durationNanos]。
 *
 * 单点路径退化为 fromRect -> points[0].rect 的线性插值。
 * 多点路径按 [CursorMotionPoint.endFraction] 分段插值 —
 * 一次提交多个插入 unit 时光标依次经过每个字/cluster，不再被压成一条直线。
 *
 * 与文字 timeline 共享同一个 frame clock。
 */
data class CursorTrack(
    val fromRect: Rect,
    val points: List<CursorMotionPoint>,
    val startedAtNanos: Long,
    val durationNanos: Long,
)

/**
 * #689 评论 5674631257 步骤2：一帧的视觉场景 — sample() 返回。
 *
 * #691：新增 [cursorRect] — 光标位置由同一个 timeline / frame clock 采样，
 * 不再由独立的 Animatable<Rect> 维护。
 *
 * @param units 当前所有文字单元（alpha/position 已插值到当前帧）。
 * @param hiddenRanges 当前应由 overlay 接管、BasicTextField 需设透明的 ranges。
 *   每一帧直接从当前 [VisualTextUnit.targetRange] != null 且仍由 overlay 绘制的 unit 推导，
 *   不从"上一事务 suppressed ranges"继承。
 * @param cursorRect 光标当前位置（已插值到当前帧）— null 表示无光标动画且无静止光标。
 */
data class ComposeVisualScene(
    val units: List<VisualTextUnit>,
    val hiddenRanges: List<TextRange>,
    val cursorRect: Rect? = null,
) {
    companion object {
        /** 空场景。 */
        val Empty = ComposeVisualScene(units = emptyList(), hiddenRanges = emptyList())
    }
}
