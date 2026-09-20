package com.xiwei.sujian.feature.editor.visual

/**
 * Issue #725 评论 5750735497：纯文字吞吐时间线 —
 * 替代旧"用屏幕自绘光标位置当进度尺"的 [CursorTrack] 机制。
 *
 * 背景：旧实现里文字吞吐（吐字/吞字）的可见进度由屏幕自绘光标的 X 坐标驱动 —
 * `clipFraction = (cursor.left - glyph.left) / glyph.width`。
 * 这把文字动画和屏幕 caret 状态机绑死：应用层必须维护一套 AndroidX
 * `TransformedTextFieldState.selectionWedgeAffinity` 的镜像才能算对光标位置，
 * 而 wedge affinity 是 AndroidX 私有状态，应用层永远猜不准，导致光标抽搐/换行错位。
 *
 * Issue #725 决策：**停止自绘屏幕 caret**。系统 caret 始终由 BasicTextField 自己画。
 * 文字吞吐动画保留，但改由纯文字时间线驱动 — 每个 unit 自己保存一个
 * [clipFraction] 通道（[TimedFloat]），不再依赖任何光标几何。
 *
 * - Inserted unit：[clipFraction] 从 0→1（字从左向右吐出）。
 * - Deleted ghost：[clipFraction] 从 1→0（字从右向左被吞掉）。
 * - 同一笔 patch 内多个 unit 按顺序分配各自的时间片（[startedAtNanos] 错开）。
 * - 快速连续输入/删除时，已经存在的 unit 继续自己的 [startedAtNanos]，不能被新输入重置。
 *
 * 这个 track 只输出 [clipFraction]，不产生、也不拥有可见 caret。
 *
 * @param clipFraction 单个 unit 的可见 fraction 通道（0..1）。
 *   - Inserted：from=0, to=1
 *   - DeletedGhost：from=1, to=0
 *   与 [VisualTextUnit.alpha] 通道并行存在：alpha 仍用于边缘柔化和非 coordinated 场景，
 *   clipFraction 用于空间裁切决定整字出现/消失。
 * @param startedAtNanos 通道开始时间戳（来自 Compose frame clock）。
 *   已存在的 unit 继续自己的 startedAtNanos，不被新输入重置。
 * @param durationNanos 通道持续时长。
 */
data class ComposeTextRevealTrack(
    val clipFraction: TimedFloat,
) {
    /**
     * 采样当前帧的 clip fraction（0..1）。
     *
     * - elapsed <= 0：返回 [clipFraction.from]（尚未开始）。
     * - elapsed >= duration：返回 [clipFraction.to]（已完成）。
     * - duration <= 0：瞬时跳到 [clipFraction.to]。
     * - 中间：线性插值。
     */
    fun sampleFraction(frameTimeNanos: Long): Float {
        val track = clipFraction
        if (track.durationNanos <= 0L) return track.to
        val elapsed = frameTimeNanos - track.startedAtNanos
        if (elapsed <= 0L) return track.from
        if (elapsed >= track.durationNanos) return track.to
        val progress = elapsed.toFloat() / track.durationNanos.toFloat()
        return track.from + (track.to - track.from) * progress
    }

    /**
     * 通道是否已完成（sample 后会返回 [clipFraction.to]）。
     */
    fun isFinished(frameTimeNanos: Long): Boolean {
        val track = clipFraction
        if (track.durationNanos <= 0L) return true
        return frameTimeNanos - track.startedAtNanos >= track.durationNanos
    }

    companion object {
        /**
         * 为 Inserted unit 创建吐字 track：clipFraction 0→1。
         */
        fun forInsert(
            frameTimeNanos: Long,
            durationNanos: Long,
        ): ComposeTextRevealTrack =
            ComposeTextRevealTrack(
                clipFraction =
                    TimedFloat(
                        from = 0f,
                        to = 1f,
                        startedAtNanos = frameTimeNanos,
                        durationNanos = durationNanos,
                    ),
            )

        /**
         * 为 DeletedGhost unit 创建吞字 track：clipFraction 1→0。
         *
         * @param currentFraction 当前实际可见 fraction（快速删除时可能不是 1），
         *   从当前值继续到 0，避免整字突然完整出现再被吞掉。
         */
        fun forDelete(
            frameTimeNanos: Long,
            durationNanos: Long,
            currentFraction: Float = 1f,
        ): ComposeTextRevealTrack =
            ComposeTextRevealTrack(
                clipFraction =
                    TimedFloat(
                        from = currentFraction,
                        to = 0f,
                        startedAtNanos = frameTimeNanos,
                        durationNanos = durationNanos,
                    ),
            )
    }
}
