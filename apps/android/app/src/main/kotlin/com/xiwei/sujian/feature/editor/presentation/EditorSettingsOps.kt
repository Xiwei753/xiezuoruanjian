package com.xiwei.sujian.feature.editor.presentation

// ! # 编辑器设置操作（从 EditorViewModel 拆分）
// !
// ! 设置加载/应用、系统减少动画检测。

import kotlinx.coroutines.launch

fun EditorViewModel.isSystemReduceMotionEnabled(): Boolean {
    return try {
        val scale =
            android.provider.Settings.Global.getFloat(
                getApplication<android.app.Application>().contentResolver,
                android.provider.Settings.Global.ANIMATOR_DURATION_SCALE,
                1f,
            )
        scale == 0f
    } catch (_: Exception) {
        false
    }
}

/**
 * #630 评论 5327560790: 从 Core 持久化设置读取权威 [EditorSettingsState] 快照 — suspend 单入口。
 *
 * [reloadSettings] 和 [initialize] 首次注入都调它，保证 typography snapshot 来自持久化
 * 权威设置而非默认值。
 */
@Suppress("RedundantSuspendModifier")
suspend fun EditorViewModel.loadEditorSettingsSnapshot(): EditorSettingsState {
    val settings = settingsRepository.getLocalSettings()
    val syncable = settingsRepository.getSyncableSettings()
    val effectiveFontSize =
        if (syncable.fontSize > 0.0) {
            syncable.fontSize.toFloat()
        } else if (settings.editorFontSize > 0.0f) {
            settings.editorFontSize
        } else {
            16f
        }
    return EditorSettingsState(
        fontSize = effectiveFontSize,
        lineSpacingMultiplier = settings.editorLineSpacingMultiplier,
        autoIndentEnabled = settings.autoIndentEnabled,
        autoIndentWidth = settings.autoIndentWidth,
        typingAnimationEnabled = settings.editorTypingAnimationEnabled,
        typingAnimationDurationMs = settings.editorTypingAnimationDurationMs.toLong(),
        // Issue #728 评论 5754045689：smooth cursor 设置 —
        // Core 持久化尚未有独立 cursor 字段，默认跟 typing animation 一致
        // （#728 设计中 caret 和文字共用同一只钟）。
        smoothCursorEnabled = settings.editorTypingAnimationEnabled,
        smoothCursorDurationMs = settings.editorTypingAnimationDurationMs.toLong(),
        coordinatedTextCursorAnimationEnabled = settings.editorCoordinatedTextCursorAnimationEnabled,
        reduceMotion = isSystemReduceMotionEnabled(),
        autoSaveEnabled = settings.autoSaveEnabled,
        autoSaveDelayMs = settings.autoSaveDelayMs,
    )
}

fun EditorViewModel.reloadSettings() {
    editorScope.launch {
        val snapshot = loadEditorSettingsSnapshot()
        _uiState.value =
            _uiState.value.copy(
                settings = snapshot,
                settingsReady = true,
            )
    }
}

fun EditorViewModel.onSettingsChanged() {
    reloadSettings()
}
