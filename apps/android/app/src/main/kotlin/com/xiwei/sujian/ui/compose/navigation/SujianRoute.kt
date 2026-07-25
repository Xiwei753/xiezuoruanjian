package com.xiwei.sujian.ui.compose.navigation

import android.os.Parcelable
import androidx.navigation3.runtime.NavKey
import kotlinx.parcelize.Parcelize
import kotlinx.serialization.Serializable

sealed interface SujianRoute : Parcelable, NavKey {
    @Parcelize
    @Serializable
    data object Works : SujianRoute

    @Parcelize
    @Serializable
    data class Project(val projectId: String) : SujianRoute

    @Parcelize
    @Serializable
    data class Chapter(val projectId: String, val volumeId: String, val chapterId: String) : SujianRoute

    @Parcelize
    @Serializable
    data object StarMap : SujianRoute

    @Parcelize
    @Serializable
    data object Stats : SujianRoute

    @Parcelize
    @Serializable
    data object Settings : SujianRoute

    @Parcelize
    @Serializable
    data class SettingsDetail(val section: SettingsSection) : SujianRoute
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
