package com.xiwei.sujian.feature.editor.motion

import androidx.compose.runtime.Immutable

/**
 * #595 四：target 级动画约束 — 不是第二个动画状态写入者，只是约束条件。
 *
 * 最终策略只在一个地方计算：
 * ```text
 * effectivePolicy = globalPolicy
 *     .apply(profileConstraint)
 *     .apply(systemReduceMotion)
 * ```
 *
 * 随后一次性传给 Rust session animation_enabled / duration、
 * [ComposeEditorVisualState] / [EditorTextFieldDrawLayer]。
 *
 * `applyProfileToPipeline()` 只处理 input type、行数、选择、复制粘贴、换行等
 * profile 内容，不再直接写动画开关。
 *
 * Issue #725：自绘 caret 已删除，allowCursor 不再生效；光标动画由系统 BasicTextField 处理。
 */
@Immutable
data class TargetMotionConstraint(
    val forceStatic: Boolean = false,
    val allowText: Boolean = true,
) {
    /**
     * 把约束应用到全局策略 — 返回受约束的策略。
     * forceStatic 时关闭文字动画。
     */
    fun apply(policy: EditorMotionPolicy): EditorMotionPolicy {
        val textEnabled = policy.textEnabled && allowText && !forceStatic
        val coordinated = policy.coordinated && textEnabled
        return policy.copy(
            textEnabled = textEnabled,
            coordinated = coordinated,
        )
    }
}
