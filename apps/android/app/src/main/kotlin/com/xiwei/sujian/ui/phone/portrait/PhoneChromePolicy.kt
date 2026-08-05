package com.xiwei.sujian.ui.phone.portrait

import androidx.navigation3.runtime.NavKey
import com.xiwei.sujian.model.SyncIndicatorState

data class PhoneChromeSpec(
    val title: String?,
    val showBack: Boolean,
    val appBarTransparent: Boolean,
    val showSync: Boolean,
    val showSearch: Boolean,
    val showSettings: Boolean,
    val showBottomBar: Boolean,
)

object PhoneChromePolicy {
    fun resolve(
        route: NavKey?,
        selectedRoot: PhoneRoot,
        workspaceLocation: WorkspaceLocation,
        syncState: SyncIndicatorState,
    ): PhoneChromeSpec = when (route) {
        is PhoneSettingsRoute.Settings -> PhoneChromeSpec(
            title = "设置",
            showBack = true,
            appBarTransparent = false,
            showSync = false,
            showSearch = false,
            showSettings = false,
            showBottomBar = false,
        )
        is PhoneRootRoute.Root -> resolveRootChrome(selectedRoot, workspaceLocation)
        else -> resolveRootChrome(selectedRoot, workspaceLocation)
    }

    private fun resolveRootChrome(
        selectedRoot: PhoneRoot,
        workspaceLocation: WorkspaceLocation,
    ): PhoneChromeSpec {
        val isWorks = selectedRoot == PhoneRoot.Works
        val isEditor = isWorks && workspaceLocation is WorkspaceLocation.Editor
        val showBack = isWorks && workspaceLocation !is WorkspaceLocation.ProjectList
        return PhoneChromeSpec(
            title = when (selectedRoot) {
                PhoneRoot.Works -> when (workspaceLocation) {
                    is WorkspaceLocation.ProjectList -> "素笺写作"
                    is WorkspaceLocation.ChapterTree -> null
                    is WorkspaceLocation.Editor -> null
                }
                PhoneRoot.StarMap -> "素笺写作"
                PhoneRoot.Stats -> "素笺写作"
            },
            showBack = showBack,
            appBarTransparent = isEditor,
            showSync = true,
            showSearch = true,
            showSettings = true,
            showBottomBar = !isEditor,
        )
    }
}
