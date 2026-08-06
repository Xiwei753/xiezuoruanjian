package com.xiwei.sujian.editor.v2.motion

import com.xiwei.sujian.model.Immutable

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
 * AndroidTextAnimationEngine、SujianEditorView。
 *
 * `applyProfileToPipeline()` 只处理 input type、行数、选择、复制粘贴、换行等
 * profile 内容，不再直接写动画开关。
 */
@Immutable
data class TargetMotionConstraint(
    val forceStatic: Boolean = false,
    val allowText: Boolean = true,
    val allowCursor: Boolean = true,
) {
    /**
     * 把约束应用到全局策略 — 返回受约束的策略。
     * forceStatic 时关闭文字和光标动画。
     */
    fun apply(policy: EditorMotionPolicy): EditorMotionPolicy {
        val textEnabled = policy.textEnabled && allowText && !forceStatic
        val cursorEnabled = policy.cursorEnabled && allowCursor && !forceStatic
        val coordinated = policy.coordinated && textEnabled && cursorEnabled
        return policy.copy(
            textEnabled = textEnabled,
            cursorEnabled = cursorEnabled,
            coordinated = coordinated,
        )
    }
}
