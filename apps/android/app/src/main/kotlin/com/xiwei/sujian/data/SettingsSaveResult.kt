package com.xiwei.sujian.data

sealed interface SettingsSaveResult {
    data object Success : SettingsSaveResult
    data class Failed(val field: SaveField) : SettingsSaveResult
}

enum class SaveField {
    LOCAL_SETTINGS,
    FONT_SIZE,
    SYNC_CONFIG,
    SYNC_SECRETS,
}