@file:Suppress("StringLiteralDuplication") // #597 技术债：协议字符串天然重复

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
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.scene.Scene
import androidx.navigation3.ui.NavDisplay
import com.xiwei.sujian.data.SyncCoordinator
import com.xiwei.sujian.data.SyncStatusRepository
import com.xiwei.sujian.runtime.LocalSujianAppDependencies
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.designsystem.component.SujianSnackbar
import com.xiwei.sujian.model.SyncTrigger
import com.xiwei.sujian.ui.compose.navigation.StarMapTopBarState
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen
import com.xiwei.sujian.ui.compose.starmap.StarMapViewModel
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
 @Suppress("CognitiveComplexMethod", "CyclomaticComplexMethod", "LongMethod") // #597 技术债：待重构拆分

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun PhonePortraitShell(
    stateHolder: PhonePortraitStateHolder,
    sessionViewModel: WorkspaceSessionViewModel,
    workspaceRepository: WorkspaceRepository,
    modifier: Modifier = Modifier,
) {
    val deps = LocalSujianAppDependencies.current
    val syncState by deps.syncStatusRepository.state.collectAsState()
    val snackbarHostState = remember { SnackbarHostState() }
    val coroutineScope = rememberCoroutineScope()

    val restoreState = sessionViewModel.restoreState

    if (restoreState !is SessionRestoreState.Ready) {
        Scaffold(
            modifier = modifier.fillMaxSize(),
            containerColor = MaterialTheme.colorScheme.background,
            contentWindowInsets = WindowInsets.safeDrawing,
        ) { innerPadding -> Box(Modifier.padding(innerPadding)) }
        return
    }

    // 会话就绪后一次性建立完整历史，之后只使用 navigator 自己保存/恢复的历史。
    val initialHistory = remember(restoreState.destination) {
        buildInitialHistory(restoreState.destination)
    }
    val navigator = rememberListDetailPaneScaffoldNavigator(initialDestinationHistory = initialHistory)
    val workspaceNavState = remember { PhoneWorkspaceNavigationState(navigator) }

    val backStack = rememberNavBackStack(PhoneRootRoute.Root)
    val currentRoute = backStack.lastOrNull()
    val chromeSpec = stateHolder.chromeSpec(currentRoute, workspaceNavState.currentLocation, syncState)

    val activity = LocalActivity.current as? ComponentActivity
    val starMapVm: StarMapViewModel = viewModel(factory = StarMapViewModel.Factory(LocalContext.current))
    val starMapTopBarState = remember { StarMapTopBarState() }

    val handleBack: suspend () -> Boolean = {
        when {
            backStack.lastOrNull() is PhoneSettingsRoute -> {
                backStack.removeLastOrNull()
                true
            }
            stateHolder.selectedRoot == PhoneRoot.StarMap && starMapTopBarState.onBack != null -> {
                starMapTopBarState.onBack?.invoke()
                true
            }
            stateHolder.selectedRoot == PhoneRoot.Works -> {
                workspaceNavState.back()
            }
            else -> false
        }
    }

    PredictiveBackHandler(
        enabled = stateHolder.selectedRoot == PhoneRoot.Works &&
            backStack.lastOrNull() !is PhoneSettingsRoute.Settings &&
            workspaceNavState.canNavigateBack,
    ) { progressEvents ->
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("shell", "start")
        try {
            progressEvents.collect { event ->
                if (event.progress != 0f) {
                    workspaceNavState.seekBack(
                        com.xiwei.sujian.ui.compose.navigation.predictiveBackStateFraction(event.progress),
                    )
                }
            }
            val handled = handleBack()
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("shell", if (handled) "complete" else "unhandled")
        } catch (e: CancellationException) {
            com.xiwei.sujian.diagnostics.DiagnosticsEvents.predictiveBack("shell", "cancel")
            withContext(NonCancellable) {
                workspaceNavState.seekBack(0f)
            }
            throw e
        }
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            PhoneTopBar(
                spec = chromeSpec,
                onBack = {
                    coroutineScope.launch {
                        if (!handleBack()) {
                            activity?.onBackPressedDispatcher?.onBackPressed()
                        }
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
                        deps.syncCoordinator.runSync(SyncTrigger.Manual)
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
                coroutineScope.launch {
                    handleBack()
                }
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.navBack(true)
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
                            starMapVm = starMapVm,
                            starMapTopBarState = starMapTopBarState,
                            innerPadding = innerPadding,
                            editorTopSafeArea = innerPadding.calculateTopPadding(),
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

}
 @Suppress("LongParameterList") // #597 技术债：待重构拆分

@Composable
private fun PhoneRootContent(
    stateHolder: PhonePortraitStateHolder,
    workspaceNavState: PhoneWorkspaceNavigationState,
    sessionViewModel: WorkspaceSessionViewModel,
    workspaceRepository: WorkspaceRepository,
    starMapVm: StarMapViewModel,
    starMapTopBarState: StarMapTopBarState,
    innerPadding: PaddingValues,
    editorTopSafeArea: androidx.compose.ui.unit.Dp,
    modifier: Modifier = Modifier,
) {
    val isEditor = stateHolder.selectedRoot == PhoneRoot.Works &&
        workspaceNavState.currentLocation is WorkspaceLocation.Editor
    val contentPadding = if (isEditor) {
        PaddingValues(bottom = innerPadding.calculateBottomPadding())
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
                    editorTopSafeArea = editorTopSafeArea,
                    modifier = Modifier.fillMaxSize(),
                )
            }
            PhoneRoot.StarMap -> {
                StarMapScreen(
                    topBarState = starMapTopBarState,
                    viewModel = starMapVm,
                    modifier = Modifier.fillMaxSize(),
                )
            }
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
