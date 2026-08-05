package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldNavigator
import androidx.compose.runtime.Stable

/**
 * 组合层可保存的唯一工作区导航状态。
 *
 * 持有唯一 Material3 Adaptive navigator（由 PhonePortraitShell 创建并注入），
 * [currentLocation] 从 navigator 的当前 destination 推导，不另存页面位置副本。
 * 顶栏返回、系统返回、页面返回和预测返回必须统一调用 [back]。
 */
@Stable
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
class PhoneWorkspaceNavigationState(
    val navigator: ThreePaneScaffoldNavigator<WorkspacePaneKey>,
) {
    val currentLocation: WorkspaceLocation
        get() = deriveWorkspaceLocation(navigator.currentDestination?.contentKey)

    val canNavigateBack: Boolean
        get() = navigator.canNavigateBack()

    suspend fun navigateToProjectList() {
        navigator.navigateTo(ListDetailPaneScaffoldRole.List, WorkspacePaneKey.ProjectList)
    }

    suspend fun navigateToChapterTree(projectId: String) {
        navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, WorkspacePaneKey.ChapterTree(projectId))
    }

    suspend fun navigateToEditor(projectId: String, volumeId: String, chapterId: String) {
        navigator.navigateTo(
            ListDetailPaneScaffoldRole.Extra,
            WorkspacePaneKey.Editor(projectId, volumeId, chapterId),
        )
    }

    /** 统一返回入口：弹出一级工作区导航；已在作品根页时返回 false。 */
    suspend fun back(): Boolean {
        if (!navigator.canNavigateBack()) return false
        return navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
    }

    /** 预测返回手势进度：把导航器 seek 到对应过渡进度；取消时传 0f 复位。 */
    suspend fun seekBack(progress: Float) {
        navigator.seekBack(BackNavigationBehavior.PopUntilScaffoldValueChange, progress)
    }
}
