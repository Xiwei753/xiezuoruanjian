package com.xiwei.sujian.feature.settings.data.model

data class LocalSettings(
    val themeMode: String? = "system",
    val appearanceMode: String = "system",
    val colorSource: String = "built_in",
    val dynamicColorEnabled: Boolean = false,
    val selectedBuiltinThemeId: String = "",
    val selectedPaletteId: String = "",
    val locale: String? = null,
    val editorFontSize: Float = 16f,
    val editorLineSpacingMultiplier: Float = 1.5f,
    val autoSaveEnabled: Boolean = true,
    val autoSaveDelayMs: Long = 1500L,
    val autoIndentEnabled: Boolean = true,
    val autoIndentWidth: Float = 2.0f,
    val windowWidth: Double = 800.0,
    val windowHeight: Double = 600.0,
    val editorTypingAnimationEnabled: Boolean = true,
    val editorSmoothCursorEnabled: Boolean = true,
    val editorTypingAnimationDurationMs: Int = 100,
    val editorSmoothCursorDurationMs: Int = 80,
    val aiEnabled: Boolean = false,
    val statsDeviceId: String? = null,
    val desktopSidebarWidth: Double = 240.0,
    val desktopEditorWidth: Double = 0.0,
    val editorCoordinatedTextCursorAnimationEnabled: Boolean = true,
    val diagnosticsEnabled: Boolean = true,
    val diagnosticsVerbose: Boolean = true,
    val useSelfRenderEditorOnAndroid: Boolean = true,
    val experimentalFullscreenMode: Boolean = false,
)

data class SyncableSettings(
    val fontSize: Double = 0.0,
    val themeMode: String = "",
    @Deprecated("Use themePaletteJson instead") val monetColor: String = "",
    val themePaletteJson: String = "",
)
