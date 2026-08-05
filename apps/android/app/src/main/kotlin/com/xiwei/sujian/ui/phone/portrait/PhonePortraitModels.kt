package com.xiwei.sujian.ui.phone.portrait

import android.os.Parcelable
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.layout.ThreePaneScaffoldRole
import androidx.navigation3.runtime.NavKey
import kotlinx.parcelize.Parcelize
import kotlinx.serialization.Serializable

enum class PhoneRoot {
    Works,
    StarMap,
    Stats,
}

/**
 * 唯一工作区导航目的地键 — 由 [PhoneWorkspaceNavigationState] 持有的
 * Material3 Adaptive navigator 使用；导航位置从当前 destination 推导。
 */
sealed interface WorkspacePaneKey {
    data object ProjectList : WorkspacePaneKey
    data class ChapterTree(val projectId: String) : WorkspacePaneKey
    data class Editor(
        val projectId: String,
        val volumeId: String,
        val chapterId: String,
    ) : WorkspacePaneKey
}

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
internal val WorkspacePaneKey.role: ThreePaneScaffoldRole
    get() = when (this) {
        WorkspacePaneKey.ProjectList -> ListDetailPaneScaffoldRole.List
        is WorkspacePaneKey.ChapterTree -> ListDetailPaneScaffoldRole.Detail
        is WorkspacePaneKey.Editor -> ListDetailPaneScaffoldRole.Extra
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

/** 从导航目的地推导工作区位置 — 唯一事实来源是 navigator 的当前 destination。 */
internal fun deriveWorkspaceLocation(paneKey: WorkspacePaneKey?): WorkspaceLocation = when (paneKey) {
    null -> WorkspaceLocation.ProjectList
    WorkspacePaneKey.ProjectList -> WorkspaceLocation.ProjectList
    is WorkspacePaneKey.ChapterTree -> WorkspaceLocation.ChapterTree(paneKey.projectId)
    is WorkspacePaneKey.Editor -> WorkspaceLocation.Editor(
        projectId = paneKey.projectId,
        volumeId = paneKey.volumeId,
        chapterId = paneKey.chapterId,
    )
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
