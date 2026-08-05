package com.xiwei.sujian.editor.v2.coordinator

/**
 * 编辑器动画设置（生产路径：设置页 → Editor Host → 输入事务 → 动画协调器）。
 *
 * 由 WritingPane 从 EditorViewModel 的设置状态推入，立即作用于共享 Editor Host；
 * 新建会话（bindSession）时同样应用当前值。
 */
data class EditorAnimationSettings(
    val typingAnimationEnabled: Boolean = true,
    val typingAnimationDurationMs: Long = 100L,
    val smoothCursorEnabled: Boolean = true,
    val smoothCursorDurationMs: Long = 80L,
)

enum class SessionResetSource {
    LOCAL_CONTENT_CHANGED,
    EXTERNAL,
    CHAPTER_SWITCH
}
