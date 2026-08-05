package com.xiwei.sujian.ui.phone.portrait

import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.ui.compose.navigation.SettingsSection

sealed interface PhonePortraitEvent {
    data class SelectRoot(val root: PhoneRoot) : PhonePortraitEvent
    data object OpenSettings : PhonePortraitEvent
    data class ToggleSettingsSection(val section: SettingsSection) : PhonePortraitEvent
    data object OpenGlobalSearch : PhonePortraitEvent
}
