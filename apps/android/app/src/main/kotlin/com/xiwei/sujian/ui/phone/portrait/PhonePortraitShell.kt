package com.xiwei.sujian.ui.phone.portrait

import androidx.activity.compose.LocalActivity
import androidx.activity.ComponentActivity
import androidx.activity.compose.PredictiveBackHandler
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
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.statusBars
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.scene.Scene
import androidx.navigation3.ui.NavDisplay
import com.xiwei.sujian.data.SyncStatusRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.designsystem.component.SujianSnackbar
import com.xiwei.sujian.model.SyncIndicatorState
import kotlinx.coroutines.launch

@Composable
fun PhonePortraitShell(
    stateHolder: PhonePortraitStateHolder,
    workspaceNavState: PhoneWorkspaceNavigationState,
    sessionViewModel: WorkspaceSessionViewModel,
    workspaceRepository: WorkspaceRepository,
    modifier: Modifier = Modifier,
) {
    val syncState by SyncStatusRepository.state.collectAsState()
    val snackbarHostState = remember { SnackbarHostState() }
    val coroutineScope = rememberCoroutineScope()

    val backStack = rememberNavBackStack(PhoneRootRoute.Root)
    val currentRoute = backStack.lastOrNull()
    val chromeSpec = stateHolder.chromeSpec(currentRoute, workspaceNavState.currentLocation, syncState)

    val activity = LocalActivity.current as? ComponentActivity

    val isEditor = stateHolder.selectedRoot == PhoneRoot.Works &&
        workspaceNavState.currentLocation is WorkspaceLocation.Editor

    val handleBack: () -> Boolean = {
        when {
            backStack.lastOrNull() is PhoneSettingsRoute -> {
                backStack.removeLastOrNull()
                true
            }
            stateHolder.selectedRoot == PhoneRoot.Works -> {
                workspaceNavState.back()
            }
            else -> false
        }
    }

    PredictiveBackHandler(enabled = stateHolder.selectedRoot == PhoneRoot.Works &&
        workspaceNavState.currentLocation !is WorkspaceLocation.ProjectList) { progressEvents ->
        try {
            progressEvents.collect { event ->
                if (event.progress != 0f) {
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("shell", "progress")
                }
            }
            val handled = handleBack()
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("shell", if (handled) "complete" else "unhandled")
        } catch (e: kotlinx.coroutines.CancellationException) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("shell", "cancel")
            throw e
        }
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            PhoneTopBar(
                spec = chromeSpec,
                onBack = {
                    val handled = handleBack()
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
                onSync = {
                    coroutineScope.launch {
                        SyncStatusRepository.manualSync()
                    }
                },
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
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
    ) { innerPadding ->
        NavDisplay(
            backStack = backStack,
            onBack = {
                val handled = handleBack()
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
                            workspaceNavState = workspaceNavState,
                            sessionViewModel = sessionViewModel,
                            workspaceRepository = workspaceRepository,
                            innerPadding = innerPadding,
                            onBack = {
                                val handled = handleBack()
                                if (!handled) {
                                    activity?.onBackPressedDispatcher?.onBackPressed()
                                }
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
            visible = stateHolder.selectedRoot == PhoneRoot.Works,
        )
    }
}

@Composable
private fun PhoneRootContent(
    stateHolder: PhonePortraitStateHolder,
    workspaceNavState: PhoneWorkspaceNavigationState,
    sessionViewModel: WorkspaceSessionViewModel,
    workspaceRepository: WorkspaceRepository,
    innerPadding: androidx.compose.foundation.layout.PaddingValues,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val isEditor = stateHolder.selectedRoot == PhoneRoot.Works &&
        workspaceNavState.currentLocation is WorkspaceLocation.Editor
    val contentPadding = if (isEditor) {
        androidx.compose.foundation.layout.PaddingValues(
            bottom = innerPadding.calculateBottomPadding()
        )
    } else {
        innerPadding
    }

    Box(
        modifier = modifier.padding(contentPadding),
    ) {
        when (stateHolder.selectedRoot) {
            PhoneRoot.Works -> {
                PhoneWorkspaceHost(
                    workspaceNavState = workspaceNavState,
                    sessionViewModel = sessionViewModel,
                    workspaceRepository = workspaceRepository,
                    onBack = onBack,
                    modifier = Modifier.fillMaxSize(),
                )
            }
            PhoneRoot.StarMap -> { }
            PhoneRoot.Stats -> {
                com.xiwei.sujian.ui.compose.stats.StatsScreen(
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
