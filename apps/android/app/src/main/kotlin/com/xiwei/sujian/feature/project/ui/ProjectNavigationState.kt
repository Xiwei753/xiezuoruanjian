package com.xiwei.sujian.feature.project.ui

import android.os.Parcelable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.snapshots.SnapshotStateList
import com.xiwei.sujian.app.state.ActiveDocumentGate
import kotlinx.parcelize.Parcelize

/**
 * 唯一工作区导航目的地键 — 业务身份（#625 第二段）。
 *
 * 不再携带 Material3 Adaptive 的 [androidx.compose.material3.adaptive.layout.ThreePaneScaffoldRole]，
 * "当前在哪个业务位置"与"屏幕上同时画哪些区域"彻底分开：
 * - 业务位置（ProjectList / ChapterTree / Editor）由 [WorkspaceNavigator] 历史栈唯一持有；
 * - 屏幕布局（窄屏单栏 / 大屏工作台）由 [com.xiwei.sujian.app.presentation.layout.AndroidLayoutSpec]
 *   的 `workspaceLayoutMode` 决定，[ProjectWorkspaceScreen] 消费。
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

sealed interface WorkspaceLocation {
    data object ProjectList : WorkspaceLocation

    data class ChapterTree(val projectId: String) : WorkspaceLocation

    data class Editor(
        val projectId: String,
        val volumeId: String,
        val chapterId: String,
    ) : WorkspaceLocation
}

/** 从导航目的地推导工作区位置 — 唯一事实来源是 [WorkspaceNavigator] 的当前 destination。 */
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
 * 纯业务工作区导航器（#625 第二段）— 持有 [WorkspacePaneKey] 历史栈，
 * 不再依赖 Material3 Adaptive 的 [androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldNavigator]。
 *
 * 职责：
 * - 维护业务位置历史栈（[history]）；
 * - 暴露当前业务位置（[currentDestination] / [currentLocation]）；
 * - 提供 navigateTo / back / seekBack / replaceInitialHistory 业务方法；
 * - canNavigateBack 由历史栈长度决定。
 *
 * seekBack 是预测返回手势进度回调 — 业务级实现只保留入口签名（手势动画由
 * [ProjectWorkspaceScreen] 的 AnimatedContent 过渡承担，不需要 Material scaffold seek）。
 * 真正的预测返回手势由 [com.xiwei.sujian.app.navigation.SujianNavigationSuite] 的
 * PredictiveBackHandler 接管，本类只提供 seekBack 空实现 + 注释。
 */
@Stable
internal class WorkspaceNavigator {
    private val _history: SnapshotStateList<WorkspacePaneKey> = mutableStateListOf()
    val history: List<WorkspacePaneKey> get() = _history

    val currentDestination: WorkspacePaneKey?
        get() = _history.lastOrNull()

    val currentLocation: WorkspaceLocation
        get() = deriveWorkspaceLocation(currentDestination)

    val canNavigateBack: Boolean
        get() = _history.size > 1

    suspend fun navigateTo(key: WorkspacePaneKey) {
        _history.add(key)
    }

    /** 弹出一级业务历史；已在根页时返回 false。 */
    suspend fun back(): Boolean {
        if (!canNavigateBack) return false
        _history.removeAt(_history.size - 1)
        return true
    }

    /**
     * 预测返回手势进度 — 业务级空实现。
     *
     * #625 第二段：解耦 Material scaffold 后，预测返回的视觉过渡由
     * [ProjectWorkspaceScreen] 的 AnimatedContent 与 NavDisplay 的 predictivePopTransitionSpec
     * 承担，不再需要 ThreePaneScaffoldNavigator.seekBack 驱动 pane 位移。
     * 保留入口签名以兼容 [com.xiwei.sujian.app.navigation.SujianWorkspaceBackEffects]
     * 的 PredictiveBackHandler 调用契约。
     */
    suspend fun seekBack(
        @Suppress("UNUSED_PARAMETER") progress: Float,
    ) {
        // 业务级空实现 — 视觉过渡由 AnimatedContent 承担。
    }

    /**
     * 一次性替换初始历史（会话恢复）— 之后导航只使用 [_history] 自己保存/恢复的历史，
     * 不再从业务字段反复重建。
     */
    fun replaceInitialHistory(initialHistory: List<WorkspacePaneKey>) {
        if (_history.isEmpty() && initialHistory.isNotEmpty()) {
            _history.addAll(initialHistory)
        }
    }
}

/**
 * 组合层可保存的唯一工作区导航状态。
 *
 * #625 第二段：持有纯业务 [WorkspaceNavigator]，[currentLocation] 从 navigator 的当前
 * destination 推导，不另存页面位置副本。顶栏返回、系统返回、页面返回和预测返回必须统一调用 [back]。
 *
 * 不再暴露 Material3 Adaptive navigator 字段 — "当前在哪个业务位置"与"屏幕上同时画哪些区域"
 * 彻底分开，后者由 [com.xiwei.sujian.app.presentation.layout.AndroidLayoutSpec] 决定。
 */
@Stable
internal class ProjectNavigationState(
    val navigator: WorkspaceNavigator,
) {
    val currentLocation: WorkspaceLocation
        get() = navigator.currentLocation

    val canNavigateBack: Boolean
        get() = navigator.canNavigateBack

    suspend fun navigateToProjectList() {
        navigator.navigateTo(WorkspacePaneKey.ProjectList)
    }

    suspend fun navigateToChapterTree(projectId: String) {
        navigator.navigateTo(WorkspacePaneKey.ChapterTree(projectId))
    }

    suspend fun navigateToEditor(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ) {
        navigator.navigateTo(WorkspacePaneKey.Editor(projectId, volumeId, chapterId))
    }

    /** 统一返回入口：弹出一级工作区导航；已在作品根页时返回 false。 */
    suspend fun back(): Boolean = navigator.back()

    /** 预测返回手势进度：把导航器 seek 到对应过渡进度；取消时传 0f 复位。 */
    suspend fun seekBack(progress: Float) {
        navigator.seekBack(progress)
    }
}

/**
 * #624 评论12 第1项：工作区统一返回入口 — 顶栏返回、系统返回全部走这里。
 *
 * 先经 [ActiveDocumentGate.flushActiveDocument] 把活动正文保存到磁盘（保存失败
 * 返回 false，导航保持 Editor 目的地），保存成功才真正弹出工作区导航。
 * 旧实现先导航离开正文再在 LaunchedEffect 里补保存 — 保存失败只能阻止
 * closeTarget，阻止不了导航本身。
 */
internal suspend fun ProjectNavigationState.guardedBack(): Boolean {
    if (!ActiveDocumentGate.flushActiveDocument()) return false
    return back()
}

/**
 * 从会话恢复目的地一次性构建 navigator 初始历史（唯一实现；测试复用同一契约）。
 * 恢复目的地由会话就绪后给出，之后导航只使用 navigator 自己保存/恢复的历史，
 * 不再从业务字段反复重建。
 *
 * #625 第二段：返回 `List<WorkspacePaneKey>`（业务身份历史），
 * 不再返回 [androidx.compose.material3.adaptive.layout.ThreePaneScaffoldDestinationItem]。
 */
internal fun buildInitialHistory(destination: SessionRestoreState.Destination): List<WorkspacePaneKey> =
    when (destination) {
        is SessionRestoreState.Destination.ProjectList ->
            listOf(WorkspacePaneKey.ProjectList)
        is SessionRestoreState.Destination.ChapterTree ->
            listOf(
                WorkspacePaneKey.ProjectList,
                WorkspacePaneKey.ChapterTree(destination.projectId),
            )
        is SessionRestoreState.Destination.Editor ->
            listOf(
                WorkspacePaneKey.ProjectList,
                WorkspacePaneKey.ChapterTree(destination.projectId),
                WorkspacePaneKey.Editor(
                    destination.projectId,
                    destination.volumeId,
                    destination.chapterId,
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
