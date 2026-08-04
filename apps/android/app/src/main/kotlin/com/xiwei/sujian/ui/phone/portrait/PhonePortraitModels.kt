package com.xiwei.sujian.ui.phone.portrait

import android.os.Parcelable
import androidx.navigation3.runtime.NavKey
import kotlinx.parcelize.Parcelize
import kotlinx.serialization.Serializable

enum class PhoneRoot {
    Works,
    StarMap,
    Stats,
}

sealed interface WorkspaceLocation {
    data object ProjectList : WorkspaceLocation
    data class ChapterTree(val projectId: String) : WorkspaceLocation
    data class Editor(
        val projectId: String,
        val volumeId: String,
        val chapterId: String,
    ) : WorkspaceLocation
}

enum class SyncIndicatorState {
    Unconfigured,
    Syncing,
    Synced,
    Failed,
}

data class PhonePortraitUiState(
    val selectedRoot: PhoneRoot,
    val workspaceLocation: WorkspaceLocation,
    val expandedSettingsSections: Set<SettingsSection>,
    val syncState: SyncIndicatorState,
)

sealed interface PhoneRootRoute : Parcelable, NavKey {
    @Parcelize
    @Serializable
    data object Root : PhoneRootRoute
}

sealed interface PhoneSettingsRoute : Parcelable, NavKey {
    @Parcelize
    @Serializable
    data object Settings : PhoneSettingsRoute
}

@Serializable
enum class SettingsSection {
    Appearance,
    Editor,
    Save,
    Sync,
    Ai,
    Diagnostics,
    Laboratory,
    About,
}
