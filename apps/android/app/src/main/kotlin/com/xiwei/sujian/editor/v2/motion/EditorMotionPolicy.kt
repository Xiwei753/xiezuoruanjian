package com.xiwei.sujian.editor.v2.motion

import androidx.compose.runtime.Immutable

/**
 * #595 三：不可变动画策略 — 文字、光标、协同、时长和 reduce-motion 的唯一事实源。
 *
 * 数据流：SettingsRepository → StateFlow<EditorMotionPolicy> → EditorViewModel /
 * WritingEditorSurface → EditorWindowHost.applyMotionPolicy → Rust session +
 * AndroidEditorPipeline 同一时刻应用。
 *
 * 初始值与 Core 默认一致（typingAnimationEnabled=true, smoothCursorEnabled=true,
 * coordinated=true），不让 Kotlin UI 状态临时默认为 false。
 *
 * - [coordinated]=true 时，文字和光标使用同一视觉事务、同一首帧、同一 rebase snapshot。
 * - [coordinated]=false 时，光标可使用独立时长，但仍由同一个 View、同一个 renderer、
 *   同一个 VSync 时间源驱动。
 * - [reduceMotion]=true 时，所有动画降级为静态更新（等价于 textEnabled=false 且
 *   cursorEnabled=false），但编辑器仍正常工作。
 */
@Immutable
data class EditorMotionPolicy(
    val textEnabled: Boolean = true,
    val textDurationMillis: Long = 100L,
    val cursorEnabled: Boolean = true,
    val cursorDurationMillis: Long = 80L,
    val coordinated: Boolean = true,
    val reduceMotion: Boolean = false,
) {
    /**
     * reduce-motion 优先级最高：直接返回全静态策略。
     */
    fun effective(): EditorMotionPolicy = if (reduceMotion) {
        copy(textEnabled = false, cursorEnabled = false, coordinated = false)
    } else {
        this
    }
}
