package com.xiwei.sujian.ui.compose.navigation

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AutoStories
import androidx.compose.material.icons.filled.BarChart
import androidx.compose.material.icons.filled.Hub
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.outlined.AutoStories
import androidx.compose.material.icons.outlined.BarChart
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen
import com.xiwei.sujian.ui.compose.stats.StatsScreen
import com.xiwei.sujian.ui.compose.settings.SettingsRoute as ComposeSettingsRoute
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

@Composable
fun SujianNavigationSuite(
    appState: SujianAppState,
    modifier: Modifier = Modifier,
) {
    NavigationSuiteScaffold(
        navigationSuiteItems = {
            SujianDestination.entries.forEach { destination ->
                item(
                    selected = appState.currentDestination == destination,
                    onClick = { appState.navigateTo(destination) },
                    icon = {
                        Icon(
                            imageVector = if (appState.currentDestination == destination) {
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
        when (appState.currentDestination) {
            SujianDestination.Works -> ProjectWorkspaceScreen(appState = appState)
            SujianDestination.StarMap -> StarMapScreen()
            SujianDestination.Stats -> StatsScreen()
            SujianDestination.Settings -> ComposeSettingsRoute(
                onNavigateBack = { appState.navigateTo(SujianDestination.Works) },
            )
        }
    }
}
