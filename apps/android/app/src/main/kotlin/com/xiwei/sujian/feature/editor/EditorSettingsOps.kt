package com.xiwei.sujian.feature.editor

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

fun EditorViewModel.reloadSettings() {
    editorScope.launch {
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
        _uiState.value =
            _uiState.value.copy(
                settings =
                    EditorSettingsState(
                        fontSize = effectiveFontSize,
                        lineSpacingMultiplier = settings.editorLineSpacingMultiplier,
                        autoIndentEnabled = settings.autoIndentEnabled,
                        autoIndentWidth = settings.autoIndentWidth,
                        typingAnimationEnabled = settings.editorTypingAnimationEnabled,
                        typingAnimationDurationMs = settings.editorTypingAnimationDurationMs.toLong(),
                        smoothCursorEnabled = settings.editorSmoothCursorEnabled,
                        smoothCursorDurationMs = settings.editorSmoothCursorDurationMs.toLong(),
                        coordinatedTextCursorAnimationEnabled = settings.editorCoordinatedTextCursorAnimationEnabled,
                        reduceMotion = isSystemReduceMotionEnabled(),
                        autoSaveEnabled = settings.autoSaveEnabled,
                        autoSaveDelayMs = settings.autoSaveDelayMs,
                    ),
            )
    }
}

fun EditorViewModel.onSettingsChanged() {
    reloadSettings()
}
