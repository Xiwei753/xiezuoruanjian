package com.xiwei.sujian.ui.phone.portrait

import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.activity.ComponentActivity
import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.ContentTransform
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
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
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.scene.Scene
import androidx.navigation3.ui.NavDisplay
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
    val snackbarHostState = remember { SnackbarHostState() }
    val starMapTopBarState = remember { StarMapTopBarState() }

    val backStack = rememberNavBackStack(PhoneRootRoute.Root)
    val currentRoute = backStack.lastOrNull()
    val chromeSpec = stateHolder.chromeSpec(currentRoute)

    val activity = LocalActivity.current as? ComponentActivity
    BackHandler {
        val handled = handlePhoneBack(backStack, stateHolder)
        if (!handled) {
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
                onBack = {
                    val handled = handlePhoneBack(backStack, stateHolder)
                    if (!handled) {
                        activity?.onBackPressedDispatcher?.onBackPressed()
                    }
                },
                onSettings = {
                    if (backStack.lastOrNull() !is PhoneSettingsRoute.Settings) {
                        backStack.add(PhoneSettingsRoute.Settings)
                    }
                },
                onSearch = { },
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
        NavDisplay(
            backStack = backStack,
            onBack = {
                val handled = handlePhoneBack(backStack, stateHolder)
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.navBack(handled)
                handled
            },
            transitionSpec = phoneForwardTransition,
            popTransitionSpec = phonePopTransition,
            entryProvider = { key: NavKey ->
                when (key) {
                    is PhoneRootRoute -> NavEntry(key) {
                        PhoneRootContent(
                            stateHolder = stateHolder,
                            sessionViewModel = sessionViewModel,
                            workspaceRepository = workspaceRepository,
                            innerPadding = innerPadding,
                            starMapTopBarState = starMapTopBarState,
                            backStack = backStack,
                            onUnhandledBack = {
                                activity?.onBackPressedDispatcher?.onBackPressed()
                            },
                            modifier = Modifier.fillMaxSize().imePadding(),
                        )
                    }
                    is PhoneSettingsRoute -> NavEntry(key) {
                        PhoneSettingsScreen(
                            expandedSections = stateHolder.expandedSettingsSections,
                            onToggleSection = { stateHolder.onEvent(PhonePortraitEvent.ToggleSettingsSection(it)) },
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(innerPadding)
                                .imePadding(),
                        )
                    }
                    else -> NavEntry(key) {}
                }
            },
        )
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

private fun handlePhoneBack(
    backStack: MutableList<NavKey>,
    stateHolder: PhonePortraitStateHolder,
): Boolean {
    if (backStack.lastOrNull() is PhoneSettingsRoute) {
        backStack.removeLastOrNull()
        return true
    }
    when (val location = stateHolder.workspaceLocation) {
        is WorkspaceLocation.Editor -> {
            stateHolder.onEvent(PhonePortraitEvent.OpenProject(location.projectId))
            return true
        }
        is WorkspaceLocation.ChapterTree -> {
            stateHolder.onEvent(PhonePortraitEvent.SelectRoot(PhoneRoot.Works))
            return true
        }
        is WorkspaceLocation.ProjectList -> return false
    }
}

@Composable
private fun PhoneRootContent(
    stateHolder: PhonePortraitStateHolder,
    sessionViewModel: WorkspaceSessionViewModel,
    workspaceRepository: WorkspaceRepository,
    innerPadding: androidx.compose.foundation.layout.PaddingValues,
    starMapTopBarState: StarMapTopBarState,
    backStack: MutableList<NavKey>,
    onUnhandledBack: () -> Unit,
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
                    onBack = {
                        val handled = handlePhoneBack(backStack, stateHolder)
                        if (!handled) onUnhandledBack()
                    },
                    onWorkspaceLocationChanged = { location ->
                        when (location) {
                            is WorkspaceLocation.ProjectList ->
                                stateHolder.onEvent(PhonePortraitEvent.SelectRoot(PhoneRoot.Works))
                            is WorkspaceLocation.ChapterTree ->
                                stateHolder.onEvent(PhonePortraitEvent.OpenProject(location.projectId))
                            is WorkspaceLocation.Editor ->
                                stateHolder.onEvent(PhonePortraitEvent.OpenChapter(
                                    location.projectId, location.volumeId, location.chapterId))
                        }
                    },
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

private val phoneForwardTransition: AnimatedContentTransitionScope<Scene<NavKey>>.() -> ContentTransform = {
    val slideIn = slideInHorizontally(animationSpec = tween(220)) { fullWidth -> fullWidth / 8 }
    val slideOut = slideOutHorizontally(animationSpec = tween(220)) { fullWidth -> -fullWidth / 8 }
    (fadeIn(animationSpec = tween(180)) + slideIn) togetherWith
        (fadeOut(animationSpec = tween(150)) + slideOut)
}

private val phonePopTransition: AnimatedContentTransitionScope<Scene<NavKey>>.() -> ContentTransform = {
    val slideIn = slideInHorizontally(animationSpec = tween(220)) { fullWidth -> -fullWidth / 8 }
    val slideOut = slideOutHorizontally(animationSpec = tween(220)) { fullWidth -> fullWidth / 8 }
    (fadeIn(animationSpec = tween(150)) + slideIn) togetherWith
        (fadeOut(animationSpec = tween(180)) + slideOut)
}
