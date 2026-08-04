package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.navigation3.runtime.NavKey
import com.xiwei.sujian.data.SettingsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

@Stable
class PhonePortraitStateHolder(
    val syncStatusStore: SyncStatusStore,
    private val settingsRepository: SettingsRepository,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    private val _backStack = mutableListOf<NavKey>(PhoneRootRoute.Root)
    val backStack: MutableList<NavKey> get() = _backStack

    var selectedRoot by mutableStateOf(PhoneRoot.Works)
        private set

    var workspaceLocation by mutableStateOf<WorkspaceLocation>(WorkspaceLocation.ProjectList)
        private set

    var expandedSettingsSections by mutableStateOf<Set<SettingsSection>>(emptySet())
        private set

    val currentRoute: NavKey? get() = _backStack.lastOrNull()

    val uiState: PhonePortraitUiState
        get() = PhonePortraitUiState(
            selectedRoot = selectedRoot,
            workspaceLocation = workspaceLocation,
            expandedSettingsSections = expandedSettingsSections,
            syncState = syncStatusStore.state.value,
        )

    val chromeSpec: PhoneChromeSpec
        get() = PhoneChromePolicy.resolve(currentRoute, uiState)

    init {
        loadExpandedSettingsSections()
    }

    fun onEvent(event: PhonePortraitEvent) {
        when (event) {
            is PhonePortraitEvent.SelectRoot -> onSelectRoot(event.root)
            is PhonePortraitEvent.OpenProject -> onOpenProject(event.projectId)
            is PhonePortraitEvent.OpenChapter -> onOpenChapter(event.projectId, event.volumeId, event.chapterId)
            is PhonePortraitEvent.Back -> onBack()
            is PhonePortraitEvent.OpenSettings -> onOpenSettings()
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

    private fun onBack() {
        val route = currentRoute
        if (route is PhoneSettingsRoute.Settings) {
            _backStack.removeLastOrNull()
            return
        }
        when (workspaceLocation) {
            is WorkspaceLocation.Editor -> {
                val editor = workspaceLocation as WorkspaceLocation.Editor
                workspaceLocation = WorkspaceLocation.ChapterTree(editor.projectId)
            }
            is WorkspaceLocation.ChapterTree -> {
                workspaceLocation = WorkspaceLocation.ProjectList
            }
            is WorkspaceLocation.ProjectList -> { }
        }
    }

    private fun onOpenSettings() {
        if (currentRoute !is PhoneSettingsRoute.Settings) {
            _backStack.add(PhoneSettingsRoute.Settings)
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
        saveExpandedSettingsSections(newSet)
    }

    private fun onManualSync() {
        scope.launch {
            syncStatusStore.manualSync()
        }
    }

    private fun loadExpandedSettingsSections() {
        try {
            val saved = settingsRepository.getExpandedSettingsSections()
            expandedSettingsSections = saved
        } catch (_: Exception) { }
    }

    private fun saveExpandedSettingsSections(sections: Set<SettingsSection>) {
        try {
            settingsRepository.saveExpandedSettingsSections(sections)
        } catch (_: Exception) { }
    }

    fun handleSystemBack(): Boolean {
        val route = currentRoute
        if (route is PhoneSettingsRoute.Settings) {
            _backStack.removeLastOrNull()
            return true
        }
        if (route is PhoneRootRoute.Root) {
            when (workspaceLocation) {
                is WorkspaceLocation.Editor -> {
                    val editor = workspaceLocation as WorkspaceLocation.Editor
                    workspaceLocation = WorkspaceLocation.ChapterTree(editor.projectId)
                    return true
                }
                is WorkspaceLocation.ChapterTree -> {
                    workspaceLocation = WorkspaceLocation.ProjectList
                    return true
                }
                is WorkspaceLocation.ProjectList -> return false
            }
        }
        return false
    }
}
