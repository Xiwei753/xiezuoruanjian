package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
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
     * 应用一个屏幕 diff — 先 sample(now) 拿到旧动画此刻屏幕真实画到的 alpha 和位置，
     * 再处理新 patch。不能从旧事务的 progress 反算，也不能先归零。
     *
     * @param patch 这一帧的屏幕 diff。
     * @param frameTimeNanos 当前帧时间戳（来自 Compose frame clock，不用 System.nanoTime()）。
     */
    fun applyPatch(patch: ComposeVisualPatch, frameTimeNanos: Long) {
        // 第一步：先 sample 当前所有 unit 到此刻的真实 alpha/位置。
        val sampledUnits = units.map { sampleUnit(it, frameTimeNanos) }

        // 第二步：把存活 unit 通过 offsetMap 映射到新正文。
        val offsetMap = patch.offsetMap
        val newLayout = patch.newLayout
        val newTextLength = newLayout.result.layoutInput.text.length

        // 分类：存活 / ghost（被覆盖或删除）
        val surviving = mutableListOf<VisualTextUnit>()
        val ghosting = mutableListOf<VisualTextUnit>()

        for (unit in sampledUnits) {
            val target = unit.targetRange
            if (target == null) {
                // 已经是 ghost：继续淡出。alpha 已到 0 的丢弃。
                val currentAlpha = currentAlpha(unit.alpha, frameTimeNanos)
                if (currentAlpha > 0f) {
                    ghosting.add(unit)
                }
                continue
            }
            // 尝试把 target 映射到新正文
            val mappedRange = if (offsetMap != null) {
                ComposeVisualRebase.mapRangeForwardThroughOffsetMapPublic(target, offsetMap, newTextLength)
            } else {
                // 无 offset map：若 target 仍在新正文范围内且文本未变，保留；否则转 ghost
                if (target.end <= newTextLength) target else null
            }
            if (mappedRange == null) {
                // 映射失败 → 转成 ghost fade-out
                ghosting.add(unit.copy(targetRange = null))
            } else {
                // 存活：alpha 通道不变（继续使用原 startedAtNanos）。
                // position 通道：只在新 layout 让位置发生变化时重定向。
                val newPosition = computeUnitPosition(newLayout, mappedRange)
                val oldPosition = currentOffset(unit.position, frameTimeNanos)
                val positionChannel =
                    if (newPosition != null && oldPosition != null && newPosition != oldPosition) {
                        // 位置变了：从此刻屏幕位置重定向到新位置
                        TimedOffset(
                            from = oldPosition,
                            to = newPosition,
                            startedAtNanos = frameTimeNanos,
                            durationNanos = patch.durationMs.coerceAtLeast(0L) * NANOS_PER_MS,
                        )
                    } else {
                        // 位置没变或无法计算：保留原 position 通道（不重建）
                        unit.position
                    }
                surviving.add(
                    unit.copy(
                        layout = newLayout,
                        range = mappedRange,
                        targetRange = mappedRange,
                        position = positionChannel,
                    ),
                )
            }
        }

        // 第三步：处理本 patch 新插入的 unit。
        val insertDurationNanos = patch.durationMs.coerceAtLeast(0L) * NANOS_PER_MS
        val inserted = mutableListOf<VisualTextUnit>()
        for (range in patch.insertedUnits) {
            if (range.start >= range.end) continue
            if (range.end > newTextLength) continue
            val position = computeUnitPosition(newLayout, range) ?: Offset.Zero
            val key = nextUnitKey++
            inserted.add(
                VisualTextUnit(
                    key = key,
                    layout = newLayout,
                    range = range,
                    targetRange = range,
                    alpha = TimedFloat(
                        from = 0f,
                        to = 1f,
                        startedAtNanos = frameTimeNanos,
                        durationNanos = insertDurationNanos,
                    ),
                    position = TimedOffset(
                        from = position,
                        to = position,
                        startedAtNanos = frameTimeNanos,
                        durationNanos = 0L,
                    ),
                ),
            )
        }

        // 第四步：处理本 patch 显式删除的 unit（Delete/Move 的 oldRanges）。
        // 这些 range 在 oldLayout 里，尝试找已存在的 unit 匹配；找不到就新建一个 ghost。
        for (range in patch.deletedUnits) {
            if (range.start >= range.end) continue
            val oldTextLength = patch.oldLayout.result.layoutInput.text.length
            if (range.end > oldTextLength) continue
            // 查找已存在 unit 中 range 匹配的（可能已被上面转成 ghost，跳过）
            val existing = surviving.firstOrNull { it.range == range }
            if (existing != null) {
                // 已存在：转成 ghost
                val currentAlphaValue = currentAlpha(existing.alpha, frameTimeNanos)
                val currentPosition = currentOffset(existing.position, frameTimeNanos) ?: Offset.Zero
                val idx = surviving.indexOf(existing)
                surviving[idx] = existing.copy(
                    targetRange = null,
                    alpha = TimedFloat(
                        from = currentAlphaValue,
                        to = 0f,
                        startedAtNanos = frameTimeNanos,
                        durationNanos = insertDurationNanos,
                    ),
                    position = TimedOffset(
                        from = currentPosition,
                        to = currentPosition,
                        startedAtNanos = frameTimeNanos,
                        durationNanos = 0L,
                    ),
                )
            }
            // 不在 surviving 里的删除 unit：oldLayout 上的文字本来就不由 timeline 接管，
            // 不需要新建 ghost（系统正文已经把它移除了）。
        }

        // 第五步：retainedMoves — 只给真正发生位移的存活 unit 重定向 position 通道。
        for (move in patch.retainedMoves) {
            val newRange = move.newRange
            if (newRange.start >= newRange.end) continue
            if (newRange.end > newTextLength) continue
            val idx = surviving.indexOfFirst { it.targetRange == newRange }
            if (idx < 0) continue
            val unit = surviving[idx]
            val newPosition = computeUnitPosition(newLayout, newRange) ?: continue
            val oldPosition = currentOffset(unit.position, frameTimeNanos) ?: newPosition
            // 只有位置真变了才重定向（删换行时几何没变的文字不产生 position track）
            if (newPosition != oldPosition) {
                surviving[idx] = unit.copy(
                    position = TimedOffset(
                        from = oldPosition,
                        to = newPosition,
                        startedAtNanos = frameTimeNanos,
                        durationNanos = insertDurationNanos,
                    ),
                )
            }
        }

        // 合并：存活 + 新插入 + ghost
        units = surviving + inserted + ghosting
    }

    /**
     * 采样当前时间线到指定帧时间 — 返回当前应绘制的 [ComposeVisualScene]。
     *
     * @param frameTimeNanos 当前帧时间戳。
     * @return 当前场景（units + hiddenRanges）。
     */
    fun sample(frameTimeNanos: Long): ComposeVisualScene {
        val sampledUnits = units.map { sampleUnit(it, frameTimeNanos) }
        // hiddenRanges：从当前 targetRange != null 且仍由 overlay 绘制的 unit 推导。
        // 不从上一事务 suppressed ranges 继承。
        val hiddenRanges = sampledUnits
            .filter { it.targetRange != null && currentAlpha(it.alpha, frameTimeNanos) < 1f }
            .mapNotNull { it.targetRange }
            .filter { it.start < it.end }
        return ComposeVisualScene(units = sampledUnits, hiddenRanges = hiddenRanges)
    }

    /**
     * 是否还有活动动画 — overlay 据此决定是否继续推进帧时钟。
     *
     * @param frameTimeNanos 当前帧时间戳。
     * @return true 表示还有 unit 的 alpha 或 position 通道未完成。
     */
    fun hasActiveAnimation(frameTimeNanos: Long): Boolean {
        return units.any { unit ->
            !isAlphaFinished(unit.alpha, frameTimeNanos) ||
                !isPositionFinished(unit.position, frameTimeNanos)
        }
    }

    /**
     * 清除所有状态 — 章节切换或 detach 时调用。
     */
    fun clear() {
        units = emptyList()
        nextUnitKey = 1L
    }

    // ==================== 内部采样与通道计算 ====================

    /**
     * 采样单个 unit 到指定帧时间 — 返回 alpha/position 已插值后的 unit。
     * 采样后的 unit 的 alpha/position 通道表示"此刻屏幕真实画到的状态"。
     */
    private fun sampleUnit(unit: VisualTextUnit, frameTimeNanos: Long): VisualTextUnit {
        return unit.copy(
            alpha = unit.alpha.copy(
                from = currentAlpha(unit.alpha, frameTimeNanos),
                to = unit.alpha.to,
                startedAtNanos = frameTimeNanos,
                durationNanos = remainingDurationNanos(unit.alpha, frameTimeNanos),
            ),
            position = unit.position.copy(
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
    private fun currentAlpha(channel: TimedFloat, frameTimeNanos: Long): Float {
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
    private fun currentOffset(channel: TimedOffset, frameTimeNanos: Long): Offset? {
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
    private fun isAlphaFinished(channel: TimedFloat, frameTimeNanos: Long): Boolean {
        if (channel.durationNanos <= 0L) return true
        return frameTimeNanos - channel.startedAtNanos >= channel.durationNanos
    }

    /**
     * 通道是否已完成（position）。
     */
    private fun isPositionFinished(channel: TimedOffset, frameTimeNanos: Long): Boolean {
        if (channel.durationNanos <= 0L) return true
        return frameTimeNanos - channel.startedAtNanos >= channel.durationNanos
    }

    /**
     * 通道剩余 duration — 采样后用。
     */
    private fun remainingDurationNanos(channel: TimedFloat, frameTimeNanos: Long): Long {
        if (channel.durationNanos <= 0L) return 0L
        return max(0L, channel.startedAtNanos + channel.durationNanos - frameTimeNanos)
    }

    private fun remainingDurationNanos(channel: TimedOffset, frameTimeNanos: Long): Long {
        if (channel.durationNanos <= 0L) return 0L
        return max(0L, channel.startedAtNanos + channel.durationNanos - frameTimeNanos)
    }

    /**
     * 从 layout 取 unit 的真实位置（左上角）。
     */
    private fun computeUnitPosition(layout: ComposeLayoutSnapshot, range: TextRange): Offset? {
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
 * #689 评论 5674631257 步骤2：一帧的视觉场景 — sample() 返回。
 *
 * @param units 当前所有文字单元（alpha/position 已插值到当前帧）。
 * @param hiddenRanges 当前应由 overlay 接管、BasicTextField 需设透明的 ranges。
 *   每一帧直接从当前 [VisualTextUnit.targetRange] != null 且仍由 overlay 绘制的 unit 推导，
 *   不从"上一事务 suppressed ranges"继承。
 */
data class ComposeVisualScene(
    val units: List<VisualTextUnit>,
    val hiddenRanges: List<TextRange>,
) {
    companion object {
        /** 空场景。 */
        val Empty = ComposeVisualScene(units = emptyList(), hiddenRanges = emptyList())
    }
}
