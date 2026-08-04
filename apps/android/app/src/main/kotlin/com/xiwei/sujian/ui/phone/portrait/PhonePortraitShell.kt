package com.xiwei.sujian.ui.phone.portrait

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.designsystem.component.SujianSnackbar
import com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState
import com.xiwei.sujian.ui.compose.stats.StatsScreen
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen

@Composable
fun PhonePortraitShell(
    stateHolder: PhonePortraitStateHolder,
    sessionViewModel: WorkspaceSessionViewModel,
    workspaceRepository: WorkspaceRepository,
    modifier: Modifier = Modifier,
) {
    val syncState by stateHolder.syncStatusStore.state.collectAsState()
    val chromeSpec = stateHolder.chromeSpec
    val snackbarHostState = remember { SnackbarHostState() }
    val starMapTopBarState = remember { StarMapTopBarState() }

    val activity = androidx.activity.compose.LocalActivity.current as? androidx.activity.ComponentActivity
    BackHandler {
        if (!stateHolder.handleSystemBack()) {
            activity?.onBackPressedDispatcher?.onBackPressed()
        }
    }

    val contentColor = if (chromeSpec.appBarTransparent) {
        Color.Transparent
    } else {
        MaterialTheme.colorScheme.background
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            PhoneTopBar(
                spec = chromeSpec,
                onBack = { stateHolder.onEvent(PhonePortraitEvent.Back) },
                onSettings = { stateHolder.onEvent(PhonePortraitEvent.OpenSettings) },
                onSearch = { stateHolder.onEvent(PhonePortraitEvent.OpenGlobalSearch) },
                onSync = { stateHolder.onEvent(PhonePortraitEvent.ManualSync) },
                syncState = syncState,
            )
        },
        bottomBar = {
            if (chromeSpec.showBottomBar) {
                PhoneBottomBar(
                    selectedRoot = stateHolder.selectedRoot,
                    onRootSelected = { stateHolder.onEvent(PhonePortraitEvent.SelectRoot(it)) },
                )
            }
        },
        snackbarHost = {
            SnackbarHost(hostState = snackbarHostState) { data ->
                SujianSnackbar(data = data)
            }
        },
        containerColor = contentColor,
        contentWindowInsets = WindowInsets.safeDrawing,
    ) { innerPadding ->
        val route = stateHolder.currentRoute
        when (route) {
            is PhoneSettingsRoute.Settings -> {
                PhoneSettingsScreen(
                    expandedSections = stateHolder.expandedSettingsSections,
                    onToggleSection = { stateHolder.onEvent(PhonePortraitEvent.ToggleSettingsSection(it)) },
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(innerPadding)
                        .imePadding(),
                )
            }
            else -> {
                PhoneRootContent(
                    stateHolder = stateHolder,
                    sessionViewModel = sessionViewModel,
                    workspaceRepository = workspaceRepository,
                    innerPadding = innerPadding,
                    starMapTopBarState = starMapTopBarState,
                    modifier = Modifier.fillMaxSize().imePadding(),
                )
            }
        }
    }

    val coordinator = com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator.current
    if (coordinator != null) {
        com.xiwei.sujian.editor.v2.compose.AnimatedTextEditorSlot(
            coordinator = coordinator,
            modifier = Modifier.fillMaxSize(),
            visible = stateHolder.selectedRoot == PhoneRoot.Works || stateHolder.selectedRoot == PhoneRoot.StarMap,
        )
    }
}

@Composable
private fun PhoneRootContent(
    stateHolder: PhonePortraitStateHolder,
    sessionViewModel: WorkspaceSessionViewModel,
    workspaceRepository: WorkspaceRepository,
    innerPadding: androidx.compose.foundation.layout.PaddingValues,
    starMapTopBarState: StarMapTopBarState,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier.padding(innerPadding),
    ) {
        when (stateHolder.selectedRoot) {
            PhoneRoot.Works -> {
                PhoneWorkspaceHost(
                    sessionViewModel = sessionViewModel,
                    workspaceRepository = workspaceRepository,
                    onOpenProject = { stateHolder.onEvent(PhonePortraitEvent.OpenProject(it)) },
                    onOpenChapter = { projectId, volumeId, chapterId ->
                        stateHolder.onEvent(PhonePortraitEvent.OpenChapter(projectId, volumeId, chapterId))
                    },
                    onBack = { stateHolder.onEvent(PhonePortraitEvent.Back) },
                    modifier = Modifier.fillMaxSize(),
                )
            }
            PhoneRoot.StarMap -> {
                StarMapScreen(
                    topBarState = starMapTopBarState,
                    modifier = Modifier.fillMaxSize(),
                )
            }
            PhoneRoot.Stats -> {
                StatsScreen(
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }
}
