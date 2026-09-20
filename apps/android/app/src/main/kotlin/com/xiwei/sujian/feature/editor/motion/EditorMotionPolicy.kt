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
     * 策略层归一 — 优先级：reduceMotion > coordinated > 用户独立设置。
     *
     * - reduceMotion=true：全静态（textEnabled=false, cursorEnabled=false, coordinated=false），
     *   编辑器仍正常工作，但所有动画降级为即时更新。优先级最高。
     * - coordinated=true：文字和光标是一套协同动画，强制 textEnabled=true, cursorEnabled=true，
     *   防止旧持久化状态（coordinated=true 但 textEnabled=false / cursorEnabled=false）
     *   升级后把独立开关藏掉却仍暗中关闭一半动画。coordinated 标记保持 true。
     * - 其余：尊重用户独立设置（textEnabled / cursorEnabled / coordinated 各自生效）。
     *
     * Issue #723 评论 5749023316 缺口2：原来只处理 reduceMotion，没处理 coordinated=true
     * 时的归一。EditorSettings.kt 在 onCheckedChange(false->true) 时会把两个旧字段写 true
     * （清理持久化值），但运行语义必须由本方法自己保证——旧设置、同步回来的设置也不会重新打架。
     */
    fun effective(): EditorMotionPolicy =
        when {
            reduceMotion -> copy(textEnabled = false, cursorEnabled = false, coordinated = false)
            coordinated -> copy(textEnabled = true, cursorEnabled = true)
            else -> this
        }
}
