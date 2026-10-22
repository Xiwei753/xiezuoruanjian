package com.xiwei.sujian.app.navigation

import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.di.SujianAppDependencies
import com.xiwei.sujian.feature.editor.ui.EditorSearchReveal
import com.xiwei.sujian.feature.editor.ui.EditorSearchRevealGate
import com.xiwei.sujian.feature.project.ui.ProjectNavigationState
import com.xiwei.sujian.feature.project.ui.WorkspaceLocation
import com.xiwei.sujian.feature.search.data.model.SearchHit
import com.xiwei.sujian.feature.search.data.model.SearchOriginScreens
import com.xiwei.sujian.feature.search.data.model.SearchScopes
import com.xiwei.sujian.feature.settings.ui.SettingsSectionRequest
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** 从当前位置打开全局搜索覆盖页，保留当前一级栈与工作区 origin 状态。 */
internal fun SujianTopLevelBackStack.openSearch(
    appState: SujianAppState,
    workspaceLocation: WorkspaceLocation,
    settingsSection: String? = null,
    originScreenOverride: String? = null,
    starmapId: String? = null,
) {
    val originScreen =
        originScreenOverride ?: resolveSearchOrigin(workspaceLocation, currentTopLevel)
    // 设置页入口只在 SearchContext 中带上 origin/section 做加权；
    // 查询范围始终是全部，保证设置结果靠前的同时其他命中仍然可见。
    val scope = SearchScopes.All
    add(
        SujianRoute.Search(
            originScreen = originScreen,
            scope = scope,
            topLevel = currentTopLevel.name,
            projectId = appState.currentProjectId,
            volumeId = appState.currentVolumeId,
            chapterId = appState.currentChapterId,
            starmapId = starmapId,
            settingsSection = settingsSection,
        ),
    )
}

internal fun resolveSearchOrigin(
    workspaceLocation: WorkspaceLocation,
    topLevel: SujianDestination,
): String =
    when (topLevel) {
        SujianDestination.StarMap -> SearchOriginScreens.Starmap
        SujianDestination.Stats -> SearchOriginScreens.General
        SujianDestination.Works ->
            when (workspaceLocation) {
                WorkspaceLocation.ProjectList -> SearchOriginScreens.Home
                is WorkspaceLocation.ChapterTree -> SearchOriginScreens.Project
                is WorkspaceLocation.Editor -> SearchOriginScreens.Writing
            }
    }

/** Android 设置分类 → Core `SearchContext.currentSettingsSection` 字符串。 */
internal fun SettingsSection.searchContextValue(): String =
    when (this) {
        SettingsSection.Appearance -> "appearance"
        SettingsSection.Editor -> "editor"
        SettingsSection.Save, SettingsSection.Sync -> "save_sync"
        SettingsSection.Ai -> "ai"
        SettingsSection.Diagnostics -> "diagnostics"
        SettingsSection.Recovery, SettingsSection.Laboratory, SettingsSection.About -> name.lowercase()
    }

/**
 * 搜索结果点击 — 先校验目标是否仍存在，再执行现有导航。
 *
 * 返回 false 时调用方把条目标记为「内容已不存在」，不弹出搜索页。
 */
internal suspend fun openSearchResult(
    hit: SearchHit,
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    topLevelBackStack: SujianTopLevelBackStack,
    deps: SujianAppDependencies,
): Boolean {
    val target = hit.target
    return when {
        hit.scope.startsWith("starmap") && target.starmapId != null -> {
            topLevelBackStack.closeSearchRoute()
            topLevelBackStack.addTopLevel(SujianDestination.StarMap)
            true
        }
        target.chapterId != null && target.projectId != null && target.volumeId != null ->
            openChapterSearchResult(
                hit = hit,
                projectId = target.projectId,
                volumeId = target.volumeId,
                chapterId = target.chapterId,
                appState = appState,
                workspaceNavState = workspaceNavState,
                topLevelBackStack = topLevelBackStack,
                deps = deps,
            )
        target.settingKey != null || target.settingSection != null -> {
            SettingsSectionRequest.request(settingsSectionFor(target.settingSection, target.settingKey))
            topLevelBackStack.closeSearchRoute()
            topLevelBackStack.addTopLevel(SujianDestination.Works)
            if (topLevelBackStack.worksStack.lastOrNull() !is SujianRoute.Settings) {
                topLevelBackStack.worksStack.add(SujianRoute.Settings)
            }
            true
        }
        target.projectId != null && target.volumeId != null ->
            openVolumeSearchResult(
                projectId = target.projectId,
                volumeId = target.volumeId,
                appState = appState,
                workspaceNavState = workspaceNavState,
                topLevelBackStack = topLevelBackStack,
                deps = deps,
            )
        target.projectId != null ->
            openProjectSearchResult(target.projectId, appState, workspaceNavState, topLevelBackStack, deps)
        target.starmapId != null -> {
            topLevelBackStack.closeSearchRoute()
            topLevelBackStack.addTopLevel(SujianDestination.StarMap)
            true
        }
        else -> false
    }
}

@Suppress("LongParameterList")
private suspend fun openChapterSearchResult(
    hit: SearchHit,
    projectId: String,
    volumeId: String,
    chapterId: String,
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    topLevelBackStack: SujianTopLevelBackStack,
    deps: SujianAppDependencies,
): Boolean {
    val chapterTitle =
        withContext(Dispatchers.IO) {
            runCatching {
                deps.projectRepository.getChapters(projectId, volumeId)
                    .firstOrNull { it.id == chapterId }
                    ?.title
            }.getOrNull()
        } ?: return false

    if (hit.scope == SearchScopes.ChapterBody && hit.isBodyHit) {
        EditorSearchRevealGate.request(
            EditorSearchReveal(
                projectId = projectId,
                volumeId = volumeId,
                chapterId = chapterId,
                byteStart = hit.byteStart,
                byteEnd = hit.byteEnd,
            ),
        )
    }

    topLevelBackStack.closeSearchRoute()
    topLevelBackStack.showWorksRoot()
    appState.selectProject(projectId)
    appState.selectChapter(volumeId, chapterId, chapterTitle)
    val current = workspaceNavState.currentLocation
    if (current != WorkspaceLocation.Editor(projectId, volumeId, chapterId)) {
        workspaceNavState.navigateToEditor(projectId, volumeId, chapterId)
    }
    return true
}

@Suppress("LongParameterList")
private suspend fun openVolumeSearchResult(
    projectId: String,
    volumeId: String,
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    topLevelBackStack: SujianTopLevelBackStack,
    deps: SujianAppDependencies,
): Boolean {
    val exists =
        withContext(Dispatchers.IO) {
            runCatching {
                deps.projectRepository.getVolumes(projectId).any { it.id == volumeId }
            }.getOrDefault(false)
        }
    if (!exists) return false
    topLevelBackStack.closeSearchRoute()
    topLevelBackStack.showWorksRoot()
    appState.selectProject(projectId)
    appState.clearChapterSelection()
    workspaceNavState.navigateToChapterTree(projectId)
    return true
}

@Suppress("LongParameterList")
private suspend fun openProjectSearchResult(
    projectId: String,
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    topLevelBackStack: SujianTopLevelBackStack,
    deps: SujianAppDependencies,
): Boolean {
    val exists =
        withContext(Dispatchers.IO) {
            runCatching {
                deps.projectRepository.getProjectSummaries().any { it.id == projectId }
            }.getOrDefault(false)
        }
    if (!exists) return false
    topLevelBackStack.closeSearchRoute()
    topLevelBackStack.showWorksRoot()
    appState.selectProject(projectId)
    appState.clearChapterSelection()
    workspaceNavState.navigateToChapterTree(projectId)
    return true
}

private fun SujianTopLevelBackStack.closeSearchRoute() {
    if (backStack.lastOrNull() is SujianRoute.Search) {
        removeLastOrNull()
    }
}

/**
 * 搜索结果要进入作品工作区时，一级栈回到 Works 根，避免旧的 Settings/Search
 * 覆盖页继续挡在目标章节树/正文前面；工作区自己的业务导航历史由
 * [ProjectNavigationState] 继续维护。
 */
private fun SujianTopLevelBackStack.showWorksRoot() {
    addTopLevel(SujianDestination.Works)
    while (worksStack.size > 1) {
        worksStack.removeAt(worksStack.size - 1)
    }
}

private fun settingsSectionFor(
    coreSection: String?,
    settingKey: String?,
): SettingsSection =
    when (coreSection) {
        "appearance" -> SettingsSection.Appearance
        "editor" -> SettingsSection.Editor
        "save_sync" ->
            if (settingKey?.contains("sync", ignoreCase = true) == true) {
                SettingsSection.Sync
            } else {
                SettingsSection.Save
            }
        "ai" -> SettingsSection.Ai
        "diagnostics" -> SettingsSection.Diagnostics
        else ->
            when {
                settingKey == "themeMode" || settingKey == "monetColor" -> SettingsSection.Appearance
                settingKey?.contains("sync", ignoreCase = true) == true -> SettingsSection.Sync
                else -> SettingsSection.Save
            }
    }
