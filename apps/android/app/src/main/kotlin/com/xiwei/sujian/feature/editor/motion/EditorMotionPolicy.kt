package com.xiwei.sujian.feature.editor.motion

import androidx.compose.runtime.Immutable

/**
 * #595 三：不可变动画策略 — 文字、协同、时长和 reduce-motion 的唯一事实源。
 *
 * 数据流：SettingsRepository → StateFlow&lt;EditorMotionPolicy&gt; → EditorViewModel /
 * WritingEditorSurface → EditorWindowHost.applyMotionPolicy → Rust session +
 * AndroidEditorPipeline 同一时刻应用。
 *
 * 初始值与 Core 默认一致（typingAnimationEnabled=true, coordinated=true），
 * 不让 Kotlin UI 状态临时默认为 false。
 *
 * 时长语义（Issue #732 评论 5763493968 第4节收口）：
 * - coordinated=true：完整模式，统一用 [textDurationMillis] 作为这一笔 [ComposeEditMotion] 的时长；
 *   独立的 [textEnabled]/[cursorEnabled]/[cursorDurationMillis] 只在 coordinated=false 时生效。
 * - coordinated=false：[textEnabled]/[cursorEnabled]/[cursorDurationMillis] 各自独立生效。
 * - reduceMotion=true：所有动画降级为静态更新（等价于 textEnabled=false），
 *   但编辑器仍正常工作。优先级最高。
 *
 * Issue #728 评论 5754045689：重新由 [ComposeEditMotion] 统一画 caret —
 * [cursorEnabled] / [cursorDurationMillis] 重新暴露，控制 caret 动画开关和时长。
 * 一笔编辑一只钟：text edit 时 caret 和文字共用 textDurationMillis；
 * selection-only 移动（无文字变化）时 caret 单独用 cursorDurationMillis。
 */
@Immutable
data class EditorMotionPolicy(
    val textEnabled: Boolean = true,
    val textDurationMillis: Long = 100L,
    val cursorEnabled: Boolean = true,
    val cursorDurationMillis: Long = 100L,
    val coordinated: Boolean = true,
    val reduceMotion: Boolean = false,
) {
    /**
     * 策略层归一 — 优先级：reduceMotion &gt; coordinated &gt; 用户独立设置。
     *
     * - reduceMotion=true：全静态（textEnabled=false, coordinated=false），
     *   编辑器仍正常工作，但所有动画降级为即时更新。优先级最高。
     * - coordinated=true：完整模式，直接返回 this —
     *   coordinated 本身就是完整模式，不靠改 textEnabled/cursorEnabled 才成立。
     *   统一用 [textDurationMillis] 作为这一笔 [ComposeEditMotion] 的时长；
     *   独立的 [textEnabled]/[cursorEnabled]/[cursorDurationMillis] 只在 coordinated=false 时生效。
     * - 其余：尊重用户独立设置（textEnabled / coordinated 各自生效）。
     *
     * Issue #732 评论 5763493968 第4节：删除 `coordinated -> copy(textEnabled = true, cursorEnabled = true)`
     * 归一 — coordinated=true 时直接返回 this，不靠改 textEnabled/cursorEnabled 才成立。
     */
    fun effective(): EditorMotionPolicy =
        when {
            reduceMotion -> copy(textEnabled = false, cursorEnabled = false, coordinated = false)
            else -> this
        }
}
