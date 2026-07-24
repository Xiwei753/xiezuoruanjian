package com.xiwei.sujian.ui.compose.navigation

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.fillMaxSize
import com.xiwei.sujian.designsystem.icon.SujianIcons
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.stringResource
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.ui.NavDisplay
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.theme.LocalSujianMotion
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.settings.SettingsRoute
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen
import com.xiwei.sujian.ui.compose.stats.StatsScreen
import com.xiwei.sujian.ui.compose.workspace.ProjectWorkspaceScreen

enum class SujianDestination(
    val labelResId: Int,
    val selectedIcon: ImageVector,
    val unselectedIcon: ImageVector,
) {
    Works(
        labelResId = R.string.title_projects,
        selectedIcon = SujianIcons.AutoStories,
        unselectedIcon = SujianIcons.AutoStoriesOutlined,
    ),
    StarMap(
        labelResId = R.string.title_starmap,
        selectedIcon = SujianIcons.Hub,
        unselectedIcon = SujianIcons.HubOutlined,
    ),
    Stats(
        labelResId = R.string.title_stats,
        selectedIcon = SujianIcons.BarChart,
        unselectedIcon = SujianIcons.BarChartOutlined,
    ),
    Settings(
        labelResId = R.string.action_settings,
        selectedIcon = SujianIcons.Settings,
        unselectedIcon = SujianIcons.SettingsOutlined,
    ),
}

private fun SujianRoute.toTopDestination(): SujianDestination = when (this) {
    is SujianRoute.Works -> SujianDestination.Works
    is SujianRoute.Project -> SujianDestination.Works
    is SujianRoute.Chapter -> SujianDestination.Works
    is SujianRoute.StarMap -> SujianDestination.StarMap
    is SujianRoute.Stats -> SujianDestination.Stats
    is SujianRoute.Settings -> SujianDestination.Settings
    is SujianRoute.SettingsDetail -> SujianDestination.Settings
}

@Composable
fun SujianNavigationSuite(
    appState: SujianAppState,
    initialDestination: String? = null,
    modifier: Modifier = Modifier,
) {
    val initialRoute = when (initialDestination) {
        "settings" -> SujianRoute.Settings
        "starmap" -> SujianRoute.StarMap
        "stats" -> SujianRoute.Stats
        else -> SujianRoute.Works
    }
    val backStack = rememberNavBackStack(initialRoute as NavKey)
    val currentRoute = backStack.lastOrNull() as? SujianRoute ?: SujianRoute.Works
    val currentTopDestination = currentRoute.toTopDestination()
    val motion = LocalSujianMotion.current
    val configuration = LocalConfiguration.current
    val isWideScreen = configuration.screenWidthDp >= 600

    NavigationSuiteScaffold(
        navigationSuiteItems = {
            SujianDestination.entries.forEach { destination ->
                item(
                    selected = currentTopDestination == destination,
                    onClick = {
                        val targetRoute = when (destination) {
                            SujianDestination.Works -> SujianRoute.Works
                            SujianDestination.StarMap -> SujianRoute.StarMap
                            SujianDestination.Stats -> SujianRoute.Stats
                            SujianDestination.Settings -> SujianRoute.Settings
                        }
                        if (destination == SujianDestination.Works) {
                            appState.clearProjectSelection()
                        }
                        backStack.clear()
                        backStack.add(targetRoute)
                    },
                    icon = {
                        Icon(
                            imageVector = if (currentTopDestination == destination) {
                                destination.selectedIcon
                            } else {
                                destination.unselectedIcon
                            },
                            contentDescription = stringResource(id = destination.labelResId),
                        )
                    },
                    label = {
                        Text(text = stringResource(id = destination.labelResId))
                    },
                )
            }
        },
        modifier = modifier.fillMaxSize(),
    ) {
        NavDisplay(
            backStack = backStack,
            onBack = {
                if (backStack.size > 1) {
                    backStack.removeLastOrNull()
                    val newTop = backStack.lastOrNull() as? SujianRoute
                    when {
                        newTop is SujianRoute.Chapter -> {
                            appState.selectChapter(newTop.volumeId, newTop.chapterId)
                        }
                        newTop is SujianRoute.Project -> {
                            appState.selectProject(newTop.projectId)
                            appState.clearChapterSelection()
                        }
                        newTop is SujianRoute.Works -> {
                            appState.clearProjectSelection()
                        }
                        newTop is SujianRoute.Settings -> {
                        }
                        newTop is SujianRoute.SettingsDetail -> {
                        }
                        else -> {
                            appState.clearProjectSelection()
                        }
                    }
                    true
                } else false
            },
            entryProvider = { key: NavKey ->
                when (key) {
                    is SujianRoute -> NavEntry(key) {
                        AnimatedContent(
                            targetState = key,
                            transitionSpec = {
                                fadeIn(animationSpec = tween(motion.standardDurationMs, delayMillis = motion.quickDurationMs)) togetherWith
                                    fadeOut(animationSpec = tween(motion.quickDurationMs))
                            },
                            label = "nav-transition",
                        ) { route ->
                            when (route) {
                                is SujianRoute.Works -> ProjectWorkspaceScreen(
                                    appState = appState,
                                    onNavigateToProject = { projectId ->
                                        backStack.add(SujianRoute.Project(projectId))
                                    },
                                    onNavigateToChapter = { projectId, volumeId, chapterId ->
                                        backStack.add(SujianRoute.Project(projectId))
                                        backStack.add(SujianRoute.Chapter(projectId, volumeId, chapterId))
                                    },
                                )
                                is SujianRoute.Project -> {
                                    LaunchedEffect(route.projectId) {
                                        appState.selectProject(route.projectId)
                                        appState.clearChapterSelection()
                                    }
                                    ProjectWorkspaceScreen(
                                        appState = appState,
                                        projectIdOverride = route.projectId,
                                        onNavigateToProject = { projectId ->
                                            backStack.add(SujianRoute.Project(projectId))
                                        },
                                        onNavigateToChapter = { projectId, volumeId, chapterId ->
                                            backStack.add(SujianRoute.Chapter(projectId, volumeId, chapterId))
                                        },
                                    )
                                }
                                is SujianRoute.Chapter -> {
                                    LaunchedEffect(route.projectId, route.volumeId, route.chapterId) {
                                        appState.selectProject(route.projectId)
                                        appState.selectChapter(route.volumeId, route.chapterId)
                                    }
                                    ProjectWorkspaceScreen(
                                        appState = appState,
                                        projectIdOverride = route.projectId,
                                        volumeIdOverride = route.volumeId,
                                        chapterIdOverride = route.chapterId,
                                        onNavigateToProject = { projectId ->
                                            backStack.add(SujianRoute.Project(projectId))
                                        },
                                        onNavigateToChapter = { projectId, volumeId, chapterId ->
                                            backStack.add(SujianRoute.Chapter(projectId, volumeId, chapterId))
                                        },
                                    )
                                }
                                is SujianRoute.StarMap -> StarMapScreen()
                                is SujianRoute.Stats -> StatsScreen()
                                is SujianRoute.Settings -> {
                                    if (isWideScreen) {
                                        var section by remember { mutableStateOf(SettingsSection.Appearance) }
                                        SettingsRoute(
                                            selectedSection = section,
                                            onSectionChange = { section = it },
                                            onNavigateBack = {
                                                if (backStack.size > 1) {
                                                    backStack.removeLastOrNull()
                                                }
                                            },
                                        )
                                    } else {
                                        SettingsRoute(
                                            onNavigateToDetail = { section ->
                                                backStack.add(SujianRoute.SettingsDetail(section))
                                            },
                                            onNavigateBack = {
                                                if (backStack.size > 1) {
                                                    backStack.removeLastOrNull()
                                                }
                                            },
                                        )
                                    }
                                }
                                is SujianRoute.SettingsDetail -> {
                                    if (isWideScreen) {
                                        var section by remember { mutableStateOf(route.section) }
                                        SettingsRoute(
                                            selectedSection = section,
                                            onSectionChange = { section = it },
                                            initialSection = route.section,
                                        )
                                    } else {
                                        SettingsRoute(
                                            onNavigateBack = {
                                                if (backStack.size > 1) {
                                                    backStack.removeLastOrNull()
                                                }
                                            },
                                            initialSection = route.section,
                                        )
                                    }
                                }
                            }
                        }
                    }
                    else -> NavEntry(key) {}
                }
            },
        )
    }
}
