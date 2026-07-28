package com.xiwei.sujian.ui.compose.workbench.panel

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.workspace.ProjectListScreen

@Composable
fun ProjectNavigatorPanel(
    appState: SujianAppState,
    onSelectProject: (projectId: String, projectTitle: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    ProjectListScreen(
        appState = appState,
        onSelectProject = onSelectProject,
        modifier = modifier,
    )
}
