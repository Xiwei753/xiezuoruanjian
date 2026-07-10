package com.xiwei.sujian.ui.compose.navigation

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.NavigationSuiteScaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import com.xiwei.sujian.ui.compose.workspace.ProjectWorkspaceScreen

enum class SujianDestination(val label: String) {
    Works("作品"),
    StarMap("星图"),
    Stats("统计"),
    Settings("设置")
}

@Composable
fun SujianNavigationSuite(modifier: Modifier = Modifier) {
    var currentDestination by rememberSaveable { mutableStateOf(SujianDestination.Works) }

    NavigationSuiteScaffold(
        navigationSuiteItems = {
            SujianDestination.entries.forEach { destination ->
                item(
                    selected = currentDestination == destination,
                    onClick = { currentDestination = destination },
                    label = { Text(destination.label) }
                )
            }
        },
        modifier = modifier.fillMaxSize()
    ) {
        when (currentDestination) {
            SujianDestination.Works -> ProjectWorkspaceScreen()
            SujianDestination.StarMap -> Text("星图（待实现）")
            SujianDestination.Stats -> Text("统计（待实现）")
            SujianDestination.Settings -> Text("设置（待实现）")
        }
    }
}
