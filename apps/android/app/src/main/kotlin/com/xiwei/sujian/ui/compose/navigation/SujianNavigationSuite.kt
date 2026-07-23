package com.xiwei.sujian.ui.compose.navigation

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AutoStories
import androidx.compose.material.icons.filled.BarChart
import androidx.compose.material.icons.filled.Hub
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.outlined.AutoStories
import androidx.compose.material.icons.outlined.BarChart
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
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
        selectedIcon = Icons.Filled.AutoStories,
        unselectedIcon = Icons.Outlined.AutoStories,
    ),
    StarMap(
        labelResId = R.string.title_starmap,
        selectedIcon = Icons.Filled.Hub,
        unselectedIcon = Icons.Outlined.Hub,
    ),
    Stats(
        labelResId = R.string.title_stats,
        selectedIcon = Icons.Filled.BarChart,
        unselectedIcon = Icons.Outlined.BarChart,
    ),
    Settings(
        labelResId = R.string.action_settings,
        selectedIcon = Icons.Filled.Settings,
        unselectedIcon = Icons.Outlined.Settings,
    ),
}

private data class SujianNavKey(val destination: SujianDestination) : NavKey

@Composable
fun SujianNavigationSuite(
    appState: SujianAppState,
    modifier: Modifier = Modifier,
) {
    val backStack = rememberNavBackStack(SujianNavKey(SujianDestination.Works))
    val currentKey = backStack.lastOrNull() as? SujianNavKey
    val currentDestination = currentKey?.destination ?: SujianDestination.Works
    val motion = LocalSujianMotion.current

    LaunchedEffect(appState.currentDestination) {
        val target = appState.currentDestination
        val topKey = backStack.lastOrNull() as? SujianNavKey
        if (topKey?.destination != target) {
            backStack.removeAll { (it as? SujianNavKey)?.destination != target }
            backStack.add(SujianNavKey(target))
        }
    }

    NavigationSuiteScaffold(
        navigationSuiteItems = {
            SujianDestination.entries.forEach { destination ->
                item(
                    selected = currentDestination == destination,
                    onClick = {
                        appState.navigateTo(destination)
                    },
                    icon = {
                        Icon(
                            imageVector = if (currentDestination == destination) {
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
            onBack = { backStack.removeLastOrNull() != null },
            entryProvider = { key: NavKey ->
                when (key) {
                    is SujianNavKey -> NavEntry(key) {
                        AnimatedContent(
                            targetState = key.destination,
                            transitionSpec = {
                                fadeIn(animationSpec = tween(motion.standardDurationMs, delayMillis = motion.quickDurationMs)) togetherWith
                                    fadeOut(animationSpec = tween(motion.quickDurationMs))
                            },
                            label = "nav-transition",
                        ) { destination ->
                            when (destination) {
                                SujianDestination.Works -> ProjectWorkspaceScreen(appState = appState)
                                SujianDestination.StarMap -> StarMapScreen()
                                SujianDestination.Stats -> StatsScreen()
                                SujianDestination.Settings -> SettingsRoute(
                                    onNavigateBack = { appState.navigateTo(SujianDestination.Works) },
                                )
                            }
                        }
                    }
                    else -> NavEntry(key) {}
                }
            },
        )
    }
}
