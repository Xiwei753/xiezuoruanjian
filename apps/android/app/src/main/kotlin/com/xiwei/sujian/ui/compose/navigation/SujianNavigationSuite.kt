package com.xiwei.sujian.ui.compose.navigation

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen
import com.xiwei.sujian.ui.compose.stats.StatsScreen
import com.xiwei.sujian.ui.compose.settings.SettingsScreen
import com.xiwei.sujian.ui.compose.workspace.ProjectWorkspaceScreen

enum class SujianDestination(val label: String) {
    Works("作品"),
    StarMap("星图"),
    Stats("统计"),
    Settings("设置")
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
                    label = { Text(destination.label) }
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
