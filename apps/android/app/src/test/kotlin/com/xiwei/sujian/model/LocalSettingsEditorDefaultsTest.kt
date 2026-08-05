package com.xiwei.sujian.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 三：LocalSettings 编辑器动画默认值契约 — 必须与 Core (writer_core settings)
 * 的 default_editor_* 函数一致，不让 Kotlin 磁盘模型临时默认为 false。
 *
 * 设置异步加载完成前，UI 使用 LocalSettings 默认值构建 EditorMotionPolicy。
 * 若默认值与 Core 不一致，会在首帧把错误策略推给 host，再推回真实值，
 * 造成动画闪烁或协同动画短暂关闭。
 */
class LocalSettingsEditorDefaultsTest {

    @Test
    fun typingAnimationEnabledDefaultsToTrue() {
        assertTrue(
            "LocalSettings.editorTypingAnimationEnabled default must be true (Core default)",
            LocalSettings().editorTypingAnimationEnabled,
        )
    }

    @Test
    fun smoothCursorEnabledDefaultsToTrue() {
        assertTrue(
            "LocalSettings.editorSmoothCursorEnabled default must be true (Core default)",
            LocalSettings().editorSmoothCursorEnabled,
        )
    }

    @Test
    fun coordinatedTextCursorAnimationEnabledDefaultsToTrue() {
        assertTrue(
            "LocalSettings.editorCoordinatedTextCursorAnimationEnabled default must be true (Core default)",
            LocalSettings().editorCoordinatedTextCursorAnimationEnabled,
        )
    }

    @Test
    fun typingAnimationDurationDefaultsToCoreValue() {
        assertEquals(
            "Core default_editor_typing_animation_duration_ms = 100",
            100,
            LocalSettings().editorTypingAnimationDurationMs,
        )
    }

    @Test
    fun smoothCursorDurationDefaultsToCoreValue() {
        assertEquals(
            "Core default_editor_smooth_cursor_duration_ms = 80",
            80,
            LocalSettings().editorSmoothCursorDurationMs,
        )
    }
}
