package com.xiwei.sujian.ui.compose.workspace

import android.os.Parcelable
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.layout.ThreePaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldNavigator
import androidx.compose.runtime.Stable
import kotlinx.parcelize.Parcelize

/**
 * 唯一工作区导航目的地键 — 由 [WorkspaceNavigationState] 持有的
 * Material3 Adaptive navigator 使用；导航位置从当前 destination 推导。
 */
sealed interface WorkspacePaneKey : Parcelable {
    @Parcelize
    data object ProjectList : WorkspacePaneKey

    @Parcelize
    data class ChapterTree(val projectId: String) : WorkspacePaneKey

    @Parcelize
    data class Editor(
        val projectId: String,
        val volumeId: String,
        val chapterId: String,
    ) : WorkspacePaneKey
}

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
internal val WorkspacePaneKey.role: ThreePaneScaffoldRole
    get() =
        when (this) {
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
internal fun deriveWorkspaceLocation(paneKey: WorkspacePaneKey?): WorkspaceLocation =
    when (paneKey) {
        null -> WorkspaceLocation.ProjectList
        WorkspacePaneKey.ProjectList -> WorkspaceLocation.ProjectList
        is WorkspacePaneKey.ChapterTree -> WorkspaceLocation.ChapterTree(paneKey.projectId)
        is WorkspacePaneKey.Editor ->
            WorkspaceLocation.Editor(
                projectId = paneKey.projectId,
                volumeId = paneKey.volumeId,
                chapterId = paneKey.chapterId,
            )
    }

sealed interface SessionRestoreState {
    data object Loading : SessionRestoreState

    /** 会话恢复目的地 — 唯一用于一次性构建 navigator 初始历史，之后不再从业务字段重建导航。 */
    sealed interface Destination {
        data object ProjectList : Destination

        data class ChapterTree(val projectId: String) : Destination

        data class Editor(
            val projectId: String,
            val volumeId: String,
            val chapterId: String,
        ) : Destination
    }

    data class Ready(val destination: Destination) : SessionRestoreState
}

/**
 * 组合层可保存的唯一工作区导航状态。
 *
 * 持有唯一 Material3 Adaptive navigator，[currentLocation] 从 navigator 的当前 destination 推导，
 * 不另存页面位置副本。顶栏返回、系统返回、页面返回和预测返回必须统一调用 [back]。
 */
@Stable
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
class WorkspaceNavigationState(
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

    suspend fun navigateToEditor(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
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

/**
 * 从会话恢复目的地一次性构建 navigator 初始历史（唯一实现；测试复用同一契约）。
 * 恢复目的地由会话就绪后给出，之后导航只使用 navigator 自己保存/恢复的历史，
 * 不再从业务字段反复重建。
 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
internal fun buildInitialHistory(
    destination: SessionRestoreState.Destination,
): List<androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem<WorkspacePaneKey>> =
    when (destination) {
        is SessionRestoreState.Destination.ProjectList ->
            listOf(
                androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem(
                    WorkspacePaneKey.ProjectList.role,
                    WorkspacePaneKey.ProjectList,
                ),
            )
        is SessionRestoreState.Destination.ChapterTree ->
            listOf(
                androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem(
                    WorkspacePaneKey.ProjectList.role,
                    WorkspacePaneKey.ProjectList,
                ),
                androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem(
                    WorkspacePaneKey.ChapterTree(destination.projectId).role,
                    WorkspacePaneKey.ChapterTree(destination.projectId),
                ),
            )
        is SessionRestoreState.Destination.Editor ->
            listOf(
                androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem(
                    WorkspacePaneKey.ProjectList.role,
                    WorkspacePaneKey.ProjectList,
                ),
                androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem(
                    WorkspacePaneKey.ChapterTree(destination.projectId).role,
                    WorkspacePaneKey.ChapterTree(destination.projectId),
                ),
                androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem(
                    WorkspacePaneKey.Editor(
                        destination.projectId,
                        destination.volumeId,
                        destination.chapterId,
                    ).role,
                    WorkspacePaneKey.Editor(
                        destination.projectId,
                        destination.volumeId,
                        destination.chapterId,
                    ),
                ),
            )
    }

/**
 * 从已持久化的业务选择推导恢复目的地：正文需要 project+volume+chapter 齐备，
 * 否则回退到章节树；无项目时回退到作品列表。
 */
internal fun deriveRestoreDestination(
    projectId: String?,
    volumeId: String?,
    chapterId: String?,
): SessionRestoreState.Destination =
    when {
        projectId == null -> SessionRestoreState.Destination.ProjectList
        volumeId != null && chapterId != null ->
            SessionRestoreState.Destination.Editor(projectId, volumeId, chapterId)
        else -> SessionRestoreState.Destination.ChapterTree(projectId)
    }
