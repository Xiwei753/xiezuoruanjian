package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect

/**
 * Issue #728 评论 5754045689：统一编辑 motion —
 * 一笔编辑只创建一个 motion，统一保存 old/new caret rect + inserted/deleted glyph units，
 * 用同一只钟驱动 caret 移动和文字吞吐。
 *
 * 背景：#725 删除了旧自绘 caret，文字吞吐改由 [ComposeTextRevealTrack] 独立驱动，
 * 系统 caret 由 BasicTextField 自己画。但"文字有自己的钟、caret 由系统画"导致
 * caret 已经跳完/没动画时字还在吐或吞，视觉不协调。
 *
 * #728 决策：**一笔编辑一只钟**。caret 移动和文字吞吐共用同一个 progress：
 * - 插入：caret 从旧位置向新位置移动，文字 reveal 从 0→1（吐字），共用 progress。
 * - 删除：caret 从旧位置向新位置移动（反向），文字 conceal 从 1→0（吞字），共用 progress。
 * - 快速连续输入时，先 sample 上一笔当前帧，再从当前 caret 位置和当前文字可见比例
 *   重定向到新目标；不能把已有文字重新从 0 或 1 开始播。
 *
 * motion 不保存自己的动画进度（progress 由 frameTime 算），也不拥有文字 units
 * （units 由 [ComposeVisualTimeline] 持有）。motion 只保存 caret 两端 + 每个 unit 的
 * from/to fraction 通道，sample 时用统一 progress 算 caret 插值和文字 fraction。
 *
 * 复用 [TimedFloat] 的概念（from/to/startedAtNanos/durationNanos），但所有通道
 * 共用同一个 startedAtNanos/durationNanos（同一只钟），所以只存一份。
 *
 * @param originCaretRect 旧 caret rect（编辑前位置）。
 * @param targetCaretRect 新 caret rect（编辑后位置）。
 * @param unitChannels 每个 unit key → (fromFraction, toFraction)。
 *   - Inserted unit：from=0, to=1（吐字）。
 *   - Deleted unit：from=1, to=0（吞字）。
 *   redirectTo 时已有 unit 的 from = 当前 fraction，to 不变；新 unit from=0/1, to=1/0。
 * @param startedAtNanos 通道开始时间戳（来自 Compose frame clock）。
 * @param durationNanos 通道持续时长。<=0 表示瞬时完成。
 */
class ComposeEditMotion(
    private val originCaretRect: Rect,
    private val targetCaretRect: Rect,
    private val unitChannels: Map<Long, UnitChannel>,
    private val startedAtNanos: Long,
    private val durationNanos: Long,
) {
    /**
     * 单个 unit 的 fraction 通道 — from→to，由统一 progress 驱动。
     */
    data class UnitChannel(
        val from: Float,
        val to: Float,
    )

    /**
     * sample 结果 — 一帧的 caret rect + 每个 unit 的 clip fraction + 是否结束。
     *
     * @param caretRect 当前帧的 caret rect（由 origin→target 插值）。
     * @param unitClipFractions 按 unit key → 可见 fraction（0..1）。
     * @param finished motion 是否已完成（sample 后 caret 在 target、fraction 在 to）。
     */
    data class Sample(
        val caretRect: Rect,
        val unitClipFractions: Map<Long, Float>,
        val finished: Boolean,
    )

    /**
     * 采样当前帧 — 用统一 progress 算 caret 插值和文字 fraction。
     *
     * - durationNanos <= 0：瞬时完成，caret=target，fraction=to。
     * - elapsed <= 0：caret=origin，fraction=from。
     * - elapsed >= duration：caret=target，fraction=to，finished=true。
     * - 中间：progress = elapsed/duration，线性插值。
     */
    fun sample(frameTimeNanos: Long): Sample {
        if (durationNanos <= 0L) {
            return Sample(
                caretRect = targetCaretRect,
                unitClipFractions = unitChannels.mapValues { (_, ch) -> ch.to.coerceIn(0f, 1f) },
                finished = true,
            )
        }
        val elapsed = frameTimeNanos - startedAtNanos
        val progress: Float
        val finished: Boolean
        when {
            elapsed <= 0L -> {
                progress = 0f
                finished = false
            }
            elapsed >= durationNanos -> {
                progress = 1f
                finished = true
            }
            else -> {
                progress = elapsed.toFloat() / durationNanos.toFloat()
                finished = false
            }
        }
        val caretRect = lerpRect(originCaretRect, targetCaretRect, progress)
        val fractions =
            unitChannels.mapValues { (_, ch) ->
                (ch.from + (ch.to - ch.from) * progress).coerceIn(0f, 1f)
            }
        return Sample(caretRect, fractions, finished)
    }

    /**
     * 从当前 sample 状态重定向到新目标 — 快速连续输入/删除时调用。
     *
     * 先 sample 当前帧，再从当前 caret 位置和当前文字可见比例重定向：
     * - 新 motion 的 origin = 当前 sample 的 caretRect（从屏幕真实位置开始）。
     * - 新 motion 的 target = [newTargetCaretRect]。
     * - 已有 unit（key 存在于当前 channels）：from = 当前 fraction，to 不变。
     * - 新 inserted unit：from=0, to=1。
     * - 新 deleted unit：from=1, to=0。
     * - 不在新 keys 里的旧 unit：丢弃（已被 timeline 收口或不再由 motion 接管）。
     *
     * 不能把已有文字重新从 0 或 1 开始播 — 已有 unit 的 from 用当前 fraction。
     *
     * @param newOriginCaretRect 新笔的 old caret rect（用于 fallback，当前 motion 已 finished 时用）。
     * @param newTargetCaretRect 新笔的 new caret rect。
     * @param newInsertedUnitKeys 新笔的 inserted unit keys。
     * @param newDeletedUnitKeys 新笔的 deleted unit keys。
     * @param frameTimeNanos 当前帧时间戳。
     * @param durationNanos 新笔时长。<=0 表示瞬时完成。
     */
    @Suppress("LongParameterList")
    fun redirectTo(
        newOriginCaretRect: Rect,
        newTargetCaretRect: Rect,
        newInsertedUnitKeys: Set<Long>,
        newDeletedUnitKeys: Set<Long>,
        frameTimeNanos: Long,
        durationNanos: Long,
    ): ComposeEditMotion {
        val currentSample = sample(frameTimeNanos)
        val origin = if (currentSample.finished) newOriginCaretRect else currentSample.caretRect
        val newChannels = mutableMapOf<Long, UnitChannel>()
        for (key in newInsertedUnitKeys) {
            val currentFraction = currentSample.unitClipFractions[key]
            newChannels[key] =
                if (currentFraction != null) {
                    // 已存在：从当前 fraction 继续到 1
                    UnitChannel(from = currentFraction, to = 1f)
                } else {
                    // 新 unit：从 0 吐到 1
                    UnitChannel(from = 0f, to = 1f)
                }
        }
        for (key in newDeletedUnitKeys) {
            val currentFraction = currentSample.unitClipFractions[key]
            newChannels[key] =
                if (currentFraction != null) {
                    // 已存在：从当前 fraction 继续到 0
                    UnitChannel(from = currentFraction, to = 0f)
                } else {
                    // 新 unit：从 1 吞到 0
                    UnitChannel(from = 1f, to = 0f)
                }
        }
        return ComposeEditMotion(
            originCaretRect = origin,
            targetCaretRect = newTargetCaretRect,
            unitChannels = newChannels,
            startedAtNanos = frameTimeNanos,
            durationNanos = durationNanos,
        )
    }

    /**
     * motion 是否已完成 — 与 sample().finished 语义一致，但不产生 Sample。
     */
    fun isFinished(frameTimeNanos: Long): Boolean {
        if (durationNanos <= 0L) return true
        return frameTimeNanos - startedAtNanos >= durationNanos
    }

    companion object {
        /**
         * 为一笔插入编辑创建 motion — caret 从 origin 向 target 移动，文字 0→1 吐字。
         *
         * @param originCaretRect 编辑前 caret rect。
         * @param targetCaretRect 编辑后 caret rect。
         * @param insertedUnitKeys 新插入的 unit keys。
         * @param frameTimeNanos 当前帧时间戳。
         * @param durationNanos 动画时长。<=0 表示瞬时完成。
         */
        fun forInsert(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            insertedUnitKeys: Set<Long>,
            frameTimeNanos: Long,
            durationNanos: Long,
        ): ComposeEditMotion =
            ComposeEditMotion(
                originCaretRect = originCaretRect,
                targetCaretRect = targetCaretRect,
                unitChannels = insertedUnitKeys.associateWith { UnitChannel(0f, 1f) },
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )

        /**
         * 为一笔删除编辑创建 motion — caret 从 origin 向 target 移动，文字 1→0 吞字。
         *
         * @param originCaretRect 编辑前 caret rect（被删文字右侧）。
         * @param targetCaretRect 编辑后 caret rect（被删文字左侧）。
         * @param deletedUnitKeys 被删除的 unit keys。
         * @param frameTimeNanos 当前帧时间戳。
         * @param durationNanos 动画时长。<=0 表示瞬时完成。
         */
        fun forDelete(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            deletedUnitKeys: Set<Long>,
            frameTimeNanos: Long,
            durationNanos: Long,
        ): ComposeEditMotion =
            ComposeEditMotion(
                originCaretRect = originCaretRect,
                targetCaretRect = targetCaretRect,
                unitChannels = deletedUnitKeys.associateWith { UnitChannel(1f, 0f) },
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )

        /**
         * 为一笔混合编辑（Move/替换）创建 motion — caret 从 origin 向 target 移动，
         * inserted units 0→1，deleted units 1→0，共用同一只钟。
         */
        @Suppress("LongParameterList")
        fun forEdit(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            insertedUnitKeys: Set<Long>,
            deletedUnitKeys: Set<Long>,
            frameTimeNanos: Long,
            durationNanos: Long,
        ): ComposeEditMotion {
            val channels = mutableMapOf<Long, UnitChannel>()
            for (key in insertedUnitKeys) channels[key] = UnitChannel(0f, 1f)
            for (key in deletedUnitKeys) channels[key] = UnitChannel(1f, 0f)
            return ComposeEditMotion(
                originCaretRect = originCaretRect,
                targetCaretRect = targetCaretRect,
                unitChannels = channels,
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )
        }

        /**
         * selection-only 移动 — caret 从 origin 向 target 移动，无文字 units。
         */
        fun forSelectionMove(
            originCaretRect: Rect,
            targetCaretRect: Rect,
            frameTimeNanos: Long,
            durationNanos: Long,
        ): ComposeEditMotion =
            ComposeEditMotion(
                originCaretRect = originCaretRect,
                targetCaretRect = targetCaretRect,
                unitChannels = emptyMap(),
                startedAtNanos = frameTimeNanos,
                durationNanos = durationNanos,
            )
    }
}

/**
 * 线性插值两个 Rect。
 */
private fun lerpRect(
    from: Rect,
    to: Rect,
    t: Float,
): Rect {
    if (t <= 0f) return from
    if (t >= 1f) return to
    return Rect(
        left = from.left + (to.left - from.left) * t,
        top = from.top + (to.top - from.top) * t,
        right = from.right + (to.right - from.right) * t,
        bottom = from.bottom + (to.bottom - from.bottom) * t,
    )
}
