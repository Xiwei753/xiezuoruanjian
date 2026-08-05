package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.navigation3.runtime.NavKey
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

@Stable
class PhonePortraitStateHolder(
    val syncStatusStore: SyncStatusStore,
    private val onSaveExpandedSections: (Set<SettingsSection>) -> Unit,
    initialExpandedSections: Set<SettingsSection> = emptySet(),
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    var selectedRoot by mutableStateOf(PhoneRoot.Works)
        private set

    var workspaceLocation by mutableStateOf<WorkspaceLocation>(WorkspaceLocation.ProjectList)
        private set

    var expandedSettingsSections by mutableStateOf(initialExpandedSections)
        private set

    val uiState: PhonePortraitUiState
        get() = PhonePortraitUiState(
            selectedRoot = selectedRoot,
            workspaceLocation = workspaceLocation,
            expandedSettingsSections = expandedSettingsSections,
            syncState = syncStatusStore.state.value,
        )

    fun chromeSpec(route: NavKey?): PhoneChromeSpec =
        PhoneChromePolicy.resolve(route, uiState)

    fun onEvent(event: PhonePortraitEvent) {
        when (event) {
            is PhonePortraitEvent.SelectRoot -> onSelectRoot(event.root)
            is PhonePortraitEvent.OpenProject -> onOpenProject(event.projectId)
            is PhonePortraitEvent.OpenChapter -> onOpenChapter(event.projectId, event.volumeId, event.chapterId)
            is PhonePortraitEvent.Back -> { }
            is PhonePortraitEvent.OpenSettings -> { }
            is PhonePortraitEvent.ToggleSettingsSection -> onToggleSettingsSection(event.section)
            is PhonePortraitEvent.ManualSync -> onManualSync()
            is PhonePortraitEvent.SyncStateChanged -> { }
            is PhonePortraitEvent.OpenGlobalSearch -> { }
        }
    }

    private fun onSelectRoot(root: PhoneRoot) {
        selectedRoot = root
        if (root == PhoneRoot.Works) {
            workspaceLocation = WorkspaceLocation.ProjectList
        }
    }

    private fun onOpenProject(projectId: String) {
        if (selectedRoot == PhoneRoot.Works) {
            workspaceLocation = WorkspaceLocation.ChapterTree(projectId)
        }
    }

    private fun onOpenChapter(projectId: String, volumeId: String, chapterId: String) {
        if (selectedRoot == PhoneRoot.Works) {
            workspaceLocation = WorkspaceLocation.Editor(projectId, volumeId, chapterId)
        }
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

    private fun onManualSync() {
        scope.launch {
            syncStatusStore.manualSync()
        }
    }
}
