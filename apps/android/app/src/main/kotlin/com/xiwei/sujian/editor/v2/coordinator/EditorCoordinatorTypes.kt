package com.xiwei.sujian.editor.v2.coordinator

import com.xiwei.sujian.editor.v2.motion.EditorMotionPolicy

/**
 * 编辑器动画设置（生产路径：设置页 → Editor Host → 输入事务 → 动画协调器）。
 *
 * #595 三：合并为 [EditorMotionPolicy] 的桥接类型，加入 coordinated 和 reduceMotion。
 * 初始值与 Core 默认一致（typingAnimationEnabled=true, smoothCursorEnabled=true,
 * coordinated=true），不让 Kotlin UI 状态临时默认为 false。
 */
data class EditorAnimationSettings(
    val typingAnimationEnabled: Boolean = true,
    val typingAnimationDurationMs: Long = 100L,
    val smoothCursorEnabled: Boolean = true,
    val smoothCursorDurationMs: Long = 80L,
    val coordinated: Boolean = true,
    val reduceMotion: Boolean = false,
) {
    fun toMotionPolicy(): EditorMotionPolicy = EditorMotionPolicy(
        textEnabled = typingAnimationEnabled,
        textDurationMillis = typingAnimationDurationMs,
        cursorEnabled = smoothCursorEnabled,
        cursorDurationMillis = smoothCursorDurationMs,
        coordinated = coordinated,
        reduceMotion = reduceMotion,
    )

    companion object {
        fun fromMotionPolicy(policy: EditorMotionPolicy): EditorAnimationSettings = EditorAnimationSettings(
            typingAnimationEnabled = policy.textEnabled,
            typingAnimationDurationMs = policy.textDurationMillis,
            smoothCursorEnabled = policy.cursorEnabled,
            smoothCursorDurationMs = policy.cursorDurationMillis,
            coordinated = policy.coordinated,
            reduceMotion = policy.reduceMotion,
        )
    }
}

enum class SessionResetSource {
    LOCAL_CONTENT_CHANGED,
    EXTERNAL,
    CHAPTER_SWITCH
}
