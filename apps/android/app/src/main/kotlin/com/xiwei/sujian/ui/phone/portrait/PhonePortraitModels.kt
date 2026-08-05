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
