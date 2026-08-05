package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.navigation3.runtime.NavKey
import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.ui.compose.navigation.SettingsSection

@Stable
class PhonePortraitStateHolder(
    initialRoot: PhoneRoot = PhoneRoot.Works,
    private val onSaveExpandedSections: (Set<SettingsSection>) -> Unit,
    initialExpandedSections: Set<SettingsSection> = emptySet(),
) {
    var selectedRoot by mutableStateOf(if (initialRoot == PhoneRoot.StarMap) PhoneRoot.Works else initialRoot)
        private set

    var expandedSettingsSections by mutableStateOf(initialExpandedSections)
        private set

    fun chromeSpec(
        route: NavKey?,
        workspaceLocation: WorkspaceLocation,
        syncState: SyncIndicatorState,
    ): PhoneChromeSpec =
        PhoneChromePolicy.resolve(route, selectedRoot, workspaceLocation, syncState)

    fun onEvent(event: PhonePortraitEvent) {
        when (event) {
            is PhonePortraitEvent.SelectRoot -> onSelectRoot(event.root)
            is PhonePortraitEvent.ToggleSettingsSection -> onToggleSettingsSection(event.section)
            is PhonePortraitEvent.OpenSettings -> { }
            is PhonePortraitEvent.OpenGlobalSearch -> { }
        }
    }

    private fun onSelectRoot(root: PhoneRoot) {
        if (root == PhoneRoot.StarMap) return
        selectedRoot = root
    }

    private fun onToggleSettingsSection(section: SettingsSection) {
        val current = expandedSettingsSections
        val newSet = if (current.contains(section)) {
            current - section
        } else {
            current + section
        }
        expandedSettingsSections = newSet
        try { onSaveExpandedSections(newSet) } catch (_: Exception) { }
    }
}
