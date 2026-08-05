package com.xiwei.sujian.ui.phone.portrait

import androidx.navigation3.runtime.NavKey

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
        uiState: PhonePortraitUiState,
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
        is PhoneRootRoute.Root -> resolveRootChrome(uiState)
        else -> resolveRootChrome(uiState)
    }

    private fun resolveRootChrome(uiState: PhonePortraitUiState): PhoneChromeSpec {
        val isEditor = uiState.workspaceLocation is WorkspaceLocation.Editor
        return PhoneChromeSpec(
            title = when (uiState.selectedRoot) {
                PhoneRoot.Works -> when (uiState.workspaceLocation) {
                    is WorkspaceLocation.ProjectList -> "素笺写作"
                    is WorkspaceLocation.ChapterTree -> null
                    is WorkspaceLocation.Editor -> null
                }
                PhoneRoot.StarMap -> "素笺写作"
                PhoneRoot.Stats -> "素笺写作"
            },
            showBack = uiState.workspaceLocation !is WorkspaceLocation.ProjectList,
            appBarTransparent = isEditor,
            showSync = true,
            showSearch = true,
            showSettings = true,
            showBottomBar = !isEditor,
        )
    }
}
