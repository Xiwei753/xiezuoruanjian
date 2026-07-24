package com.xiwei.sujian.ui.compose.navigation

import android.os.Parcelable
import androidx.navigation3.runtime.NavKey
import kotlinx.parcelize.Parcelize

sealed interface SujianRoute : Parcelable, NavKey {
    @Parcelize
    data object Works : SujianRoute

    @Parcelize
    data class Project(val projectId: String) : SujianRoute

    @Parcelize
    data class Chapter(val projectId: String, val volumeId: String, val chapterId: String) : SujianRoute

    @Parcelize
    data object StarMap : SujianRoute

    @Parcelize
    data object Stats : SujianRoute

    @Parcelize
    data object Settings : SujianRoute

    @Parcelize
    data class SettingsDetail(val section: SettingsSection) : SujianRoute
}

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
