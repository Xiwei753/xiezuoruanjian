package com.xiwei.sujian.ui.phone.portrait

sealed interface PhonePortraitEvent {
    data class SelectRoot(val root: PhoneRoot) : PhonePortraitEvent
    data class OpenProject(val projectId: String) : PhonePortraitEvent
    data class OpenChapter(
        val projectId: String,
        val volumeId: String,
        val chapterId: String,
    ) : PhonePortraitEvent
    data object Back : PhonePortraitEvent
    data object OpenSettings : PhonePortraitEvent
    data class ToggleSettingsSection(val section: SettingsSection) : PhonePortraitEvent
    data object ManualSync : PhonePortraitEvent
    data class SyncStateChanged(val state: SyncIndicatorState) : PhonePortraitEvent
    data object OpenGlobalSearch : PhonePortraitEvent
}
