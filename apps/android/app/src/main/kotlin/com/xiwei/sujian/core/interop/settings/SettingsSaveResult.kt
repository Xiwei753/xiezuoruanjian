package com.xiwei.sujian.core.interop.settings

sealed interface SettingsSaveResult {
    data object Success : SettingsSaveResult

    data class Failed(val failures: List<SaveFailure>) : SettingsSaveResult
}

data class SaveFailure(
    val field: SaveField,
    val revision: Long,
)

enum class SaveField {
    LOCAL_SETTINGS,
    FONT_SIZE,
    SYNC_CONFIG,
    SYNC_SECRETS,
}
