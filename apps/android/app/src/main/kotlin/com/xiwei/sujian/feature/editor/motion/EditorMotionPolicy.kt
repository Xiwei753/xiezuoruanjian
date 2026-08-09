package com.xiwei.sujian.feature.editor.motion

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
 * 时长语义（#605 收口）：
 * - coordinated=true: textDurationMillis 控制整条编辑视觉事务；
 *   cursorDurationMillis 不参与 Insert/Delete/Replace/Composition；
 *   光标 progress = 主 timeline progress。
 * - coordinated=false: textDurationMillis / cursorDurationMillis 各自生效；
 *   光标使用独立 cursorTimeline。
 * - CURSOR_ONLY: 使用 cursorDurationMillis。
 * - reduceMotion=true: 所有动画降级为静态更新（等价于 textEnabled=false 且
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
    fun effective(): EditorMotionPolicy =
        if (reduceMotion) {
            copy(textEnabled = false, cursorEnabled = false, coordinated = false)
        } else {
            this
        }
}
