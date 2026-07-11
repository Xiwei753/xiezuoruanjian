package com.xiwei.sujian.ui.compose.navigation

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen
import com.xiwei.sujian.ui.compose.stats.StatsScreen
import com.xiwei.sujian.ui.compose.settings.SettingsScreen
import com.xiwei.sujian.ui.compose.workspace.ProjectWorkspaceScreen

enum class SujianDestination {
    Works,
    StarMap,
    Stats,
    Settings
}

@Composable
fun SujianNavigationSuite(
    appState: SujianAppState,
    modifier: Modifier = Modifier
) {
    NavigationSuiteScaffold(
        navigationSuiteItems = {
            SujianDestination.entries.forEach { destination ->
                item(
                    selected = appState.currentDestination == destination,
                    onClick = { appState.navigateTo(destination) },
                    icon = {},
                    label = {
                        Text(
                            when (destination) {
                                SujianDestination.Works -> stringResource(id = R.string.title_projects)
                                SujianDestination.StarMap -> stringResource(id = R.string.title_starmap)
                                SujianDestination.Stats -> stringResource(id = R.string.title_stats)
                                SujianDestination.Settings -> stringResource(id = R.string.action_settings)
                            }
                        )
                    }
                )
            }
        },
        modifier = modifier.fillMaxSize()
    ) {
        when (appState.currentDestination) {
            SujianDestination.Works -> ProjectWorkspaceScreen(appState = appState)
            SujianDestination.StarMap -> StarMapScreen()
            SujianDestination.Stats -> StatsScreen()
            SujianDestination.Settings -> SettingsScreen(
                onReturnFromSettings = { appState.navigateTo(SujianDestination.Works) }
            )
        }
    }
}
