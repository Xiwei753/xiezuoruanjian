package com.xiwei.sujian.ui.compose.navigation

import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.ContentTransform
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.scene.Scene
import androidx.navigation3.ui.NavDisplay
import com.xiwei.sujian.R
import com.xiwei.sujian.data.SyncCoordinator
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.designsystem.component.SujianIconButton
import com.xiwei.sujian.designsystem.component.SujianSnackbar
import com.xiwei.sujian.designsystem.component.SujianTopAppBar
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncTrigger
import com.xiwei.sujian.platform.api.WindowSizeClass
import com.xiwei.sujian.runtime.LocalSujianAppDependencies
import com.xiwei.sujian.ui.compose.LocalAndroidCapabilities
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.settings.SettingsRoute
import com.xiwei.sujian.ui.compose.settings.settingsCategories
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen
import com.xiwei.sujian.ui.compose.starmap.StarMapViewModel
import com.xiwei.sujian.ui.compose.stats.StatsScreen
import com.xiwei.sujian.ui.compose.workspace.ProjectWorkspaceScreen
import com.xiwei.sujian.ui.phone.portrait.PhonePortraitStateHolder
import com.xiwei.sujian.ui.phone.portrait.PhonePortraitEvent
import com.xiwei.sujian.ui.phone.portrait.PhoneRoot
import com.xiwei.sujian.ui.phone.portrait.PhoneSettingsRoute
import com.xiwei.sujian.ui.phone.portrait.PhoneSettingsScreen
import com.xiwei.sujian.ui.phone.portrait.PhoneUiPreferences
import com.xiwei.sujian.ui.phone.portrait.PhoneWorkspaceHost
import com.xiwei.sujian.ui.phone.portrait.PhoneWorkspaceNavigationState
import com.xiwei.sujian.ui.phone.portrait.WorkspaceLocation
import com.xiwei.sujian.ui.phone.portrait.WorkspacePaneKey
import com.xiwei.sujian.ui.phone.portrait.WorkspaceSessionViewModel
import com.xiwei.sujian.ui.phone.portrait.deriveWorkspaceLocation
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

enum class SujianDestination(
    val labelResId: Int,
    val selectedIcon: ImageVector,
    val unselectedIcon: ImageVector,
) {
    Works(
        labelResId = R.string.title_projects,
        selectedIcon = SujianIcons.AutoStories,
        unselectedIcon = SujianIcons.AutoStoriesOutlined,
    ),
    StarMap(
        labelResId = R.string.title_starmap,
        selectedIcon = SujianIcons.Hub,
        unselectedIcon = SujianIcons.HubOutlined,
    ),
    Stats(
        labelResId = R.string.title_stats,
        selectedIcon = SujianIcons.BarChart,
        unselectedIcon = SujianIcons.BarChartOutlined,
    ),
    Settings(
        labelResId = R.string.action_settings,
        selectedIcon = SujianIcons.Settings,
        unselectedIcon = SujianIcons.SettingsOutlined,
    ),
}

private fun SujianRoute.toTopDestination(): SujianDestination = when (this) {
    is SujianRoute.Works -> SujianDestination.Works
    is SujianRoute.StarMap -> SujianDestination.StarMap
    is SujianRoute.Stats -> SujianDestination.Stats
    is SujianRoute.Settings -> SujianDestination.Settings
}

private fun SujianDestination.toRoute(): SujianRoute = when (this) {
    SujianDestination.Works -> SujianRoute.Works
    SujianDestination.StarMap -> SujianRoute.StarMap
    SujianDestination.Stats -> SujianRoute.Stats
    SujianDestination.Settings -> SujianRoute.Settings
}

@Stable
class StarMapTopBarState {
    var title by androidx.compose.runtime.mutableStateOf("")
        private set
    var onBack by androidx.compose.runtime.mutableStateOf<(() -> Unit)?>(null)
        private set
    var actions by androidx.compose.runtime.mutableStateOf<(@Composable () -> Unit)?>(null)
        private set

    fun update(
        title: String,
        onBack: (() -> Unit)?,
        actions: (@Composable () -> Unit)?,
    ) {
        this.title = title
        this.onBack = onBack
        this.actions = actions
    }

    fun clear() {
        title = ""
        onBack = null
        actions = null
    }
}

/**
 * 统一自适应导航 — 手机竖屏和宽屏共用同一份导航历史和目的地。
 *
 * - 窄窗口（Compact）：底部 NavigationBar
 * - 宽窗口（Medium/Expanded）：侧边 NavigationRail
 * - 同一 ViewModel、同一 Repository、同一返回历史
 * - 旋转/分屏/折叠后当前内容和状态不丢
 */
@Suppress(
    "LongMethod",
    "CyclomaticComplexMethod",
    "CognitiveComplexMethod",
    // UI 统一重构核心函数：合并 NavigationBar / NavigationRail / NavigationSuiteScaffold
    // 三套布局的导航项渲染、路由切换、返回栈管理和动画过渡。
    // 拆分需要更大范围的 UI 架构重构，当前阶段整体抑制。
)
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun SujianNavigationSuite(
    appState: SujianAppState,
    initialDestination: String? = null,
    modifier: Modifier = Modifier,
) {
    val capabilities = LocalAndroidCapabilities.current
    val isCompact = capabilities.windowSizeClass == WindowSizeClass.Compact

    val initialStack: List<SujianRoute> = when (initialDestination) {
        "settings" -> listOf(SujianRoute.Works, SujianRoute.Settings)
        "starmap" -> listOf(SujianRoute.Works, SujianRoute.StarMap)
        "stats" -> listOf(SujianRoute.Works, SujianRoute.Stats)
        else -> listOf(SujianRoute.Works)
    }
    val backStack = rememberNavBackStack(*initialStack.toTypedArray())
    val currentRoute = backStack.lastOrNull() as? SujianRoute ?: SujianRoute.Works
    val currentTopDestination = currentRoute.toTopDestination()
    val snackbarHostState = remember { SnackbarHostState() }
    val starMapTopBarState = remember { StarMapTopBarState() }
    val starMapVm: StarMapViewModel = viewModel(factory = StarMapViewModel.Factory(LocalContext.current))
    var settingsDetailSection by remember { mutableStateOf<SettingsSection?>(null) }

    val deps = LocalSujianAppDependencies.current
    val syncState by deps.syncStatusRepository.state.collectAsState()
    val coroutineScope = rememberCoroutineScope()

    LaunchedEffect(currentTopDestination) {
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.navigation(currentTopDestination.name)
        if (currentTopDestination != SujianDestination.StarMap) {
            starMapTopBarState.clear()
        }
        if (currentTopDestination != SujianDestination.Settings) {
            settingsDetailSection = null
        }
        if (currentTopDestination == SujianDestination.Settings && settingsDetailSection == null) {
            settingsDetailSection = SettingsSection.Appearance
        }
    }

    val topBarTitle = when (currentTopDestination) {
        SujianDestination.Works -> {
            if (appState.currentProjectId != null) {
                appState.currentProjectTitle.ifEmpty { stringResource(id = R.string.title_projects) }
            } else {
                stringResource(id = R.string.title_projects)
            }
        }
        SujianDestination.StarMap -> {
            starMapTopBarState.title.ifEmpty { stringResource(id = R.string.title_starmap) }
        }
        SujianDestination.Stats -> stringResource(id = R.string.title_stats)
        SujianDestination.Settings -> stringResource(id = R.string.action_settings)
    }

    val showBack = currentTopDestination != SujianDestination.Works ||
        (currentTopDestination == SujianDestination.Works && appState.currentProjectId != null) ||
        (currentTopDestination == SujianDestination.StarMap && starMapTopBarState.onBack != null)
    val topBarNavigationIcon = when {
        currentTopDestination == SujianDestination.Settings -> SujianIcons.ArrowBack
        showBack && currentTopDestination != SujianDestination.Works -> SujianIcons.ArrowBack
        currentTopDestination == SujianDestination.StarMap && starMapTopBarState.onBack != null -> SujianIcons.ArrowBack
        else -> null
    }
    val topBarOnNavigationClick: (() -> Unit)? = when {
        currentTopDestination == SujianDestination.Settings -> {
            { backStack.removeLastOrNull() }
        }
        currentTopDestination == SujianDestination.StarMap -> starMapTopBarState.onBack
        else -> null
    }
    val topBarActions: @Composable () -> Unit = {
        if (currentTopDestination == SujianDestination.StarMap) {
            starMapTopBarState.actions?.invoke()
        }
        if (currentTopDestination == SujianDestination.Works) {
            SujianIconButton(
                onClick = {
                    coroutineScope.launch {
                        deps.syncCoordinator.runSync(SyncTrigger.Manual)
                    }
                },
                icon = when (syncState) {
                    SyncIndicatorState.Unconfigured -> SujianIcons.CloudOff
                    SyncIndicatorState.Syncing -> SujianIcons.CloudSync
                    SyncIndicatorState.Synced -> SujianIcons.CloudDone
                    SyncIndicatorState.Failed -> SujianIcons.CloudError
                },
                contentDescription = "同步",
            )
        }
    }

    val navDisplayContent: @Composable () -> Unit = {
        NavDisplay(
            backStack = backStack,
            onBack = {
                val handled = backStack.size > 1
                if (handled) {
                    backStack.removeLastOrNull()
                }
                com.xiwei.sujian.diagnostics.DiagnosticsEvents.navBack(handled)
                handled
            },
            transitionSpec = navForwardTransition,
            popTransitionSpec = navPopTransition,
            predictivePopTransitionSpec = navPredictivePopTransition,
            entryProvider = { key: NavKey ->
                when (key) {
                    is SujianRoute -> NavEntry(key) { route ->
                        when (route) {
                            is SujianRoute.Works -> ProjectWorkspaceScreen(
                                appState = appState,
                            )
                            is SujianRoute.StarMap -> StarMapScreen(
                                topBarState = starMapTopBarState,
                                viewModel = starMapVm,
                                modifier = Modifier.testTag(SujianSemanticIds.StarMapScreen),
                            )
                            is SujianRoute.Stats -> StatsScreen()
                            is SujianRoute.Settings -> SettingsRoute(
                                detailSection = settingsDetailSection,
                                onDetailSectionChange = { settingsDetailSection = it },
                            )
                        }
                    }
                    else -> NavEntry(key) {}
                }
            },
        )
    }

    if (isCompact) {
        // 窄窗口：底部 NavigationBar
        Scaffold(
            modifier = modifier.fillMaxSize(),
            topBar = {
                SujianTopAppBar(
                    title = topBarTitle,
                    navigationIcon = topBarNavigationIcon,
                    onNavigationClick = topBarOnNavigationClick,
                    actions = topBarActions,
                )
            },
            bottomBar = {
                NavigationBar(
                    windowInsets = WindowInsets(0.dp),
                ) {
                    SujianDestination.entries.forEach { destination ->
                        NavigationBarItem(
                            selected = currentTopDestination == destination,
                            onClick = { navigateToTopDestination(backStack, destination) },
                            icon = {
                                Icon(
                                    imageVector = if (currentTopDestination == destination) {
                                        destination.selectedIcon
                                    } else {
                                        destination.unselectedIcon
                                    },
                                    contentDescription = stringResource(id = destination.labelResId),
                                )
                            },
                            label = { Text(text = stringResource(id = destination.labelResId)) },
                            modifier = navItemModifier(destination),
                        )
                    }
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
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(innerPadding)
                    .imePadding(),
            ) {
                navDisplayContent()
            }
        }
    } else {
        // 宽窗口：侧边 NavigationRail
        Scaffold(
            modifier = modifier.fillMaxSize(),
            topBar = {
                SujianTopAppBar(
                    title = topBarTitle,
                    navigationIcon = topBarNavigationIcon,
                    onNavigationClick = topBarOnNavigationClick,
                    actions = topBarActions,
                )
            },
            snackbarHost = {
                SnackbarHost(hostState = snackbarHostState) { data ->
                    SujianSnackbar(data = data)
                }
            },
            containerColor = MaterialTheme.colorScheme.background,
            contentWindowInsets = WindowInsets.safeDrawing,
        ) { innerPadding ->
            Row(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(innerPadding)
                    .imePadding(),
            ) {
                NavigationRail(
                    modifier = Modifier.fillMaxHeight(),
                    windowInsets = WindowInsets(0.dp),
                ) {
                    SujianDestination.entries.forEach { destination ->
                        NavigationRailItem(
                            selected = currentTopDestination == destination,
                            onClick = { navigateToTopDestination(backStack, destination) },
                            icon = {
                                Icon(
                                    imageVector = if (currentTopDestination == destination) {
                                        destination.selectedIcon
                                    } else {
                                        destination.unselectedIcon
                                    },
                                    contentDescription = stringResource(id = destination.labelResId),
                                )
                            },
                            label = { Text(text = stringResource(id = destination.labelResId)) },
                            modifier = navItemModifier(destination),
                        )
                    }
                }
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxHeight(),
                ) {
                    navDisplayContent()
                }
            }
        }
    }
}

private fun navItemModifier(destination: SujianDestination): Modifier {
    val semanticTag = when (destination) {
        SujianDestination.Works -> SujianSemanticIds.NavigationWorks
        SujianDestination.StarMap -> SujianSemanticIds.NavigationStarMap
        SujianDestination.Stats -> null
        SujianDestination.Settings -> SujianSemanticIds.NavigationSettings
    }
    return if (semanticTag != null) Modifier.testTag(semanticTag) else Modifier
}

private fun navigateToTopDestination(
    backStack: NavBackStack<NavKey>,
    destination: SujianDestination,
) {
    val target = destination.toRoute()
    if (backStack.isEmpty()) {
        backStack.add(SujianRoute.Works)
    }
    if (backStack.first() != SujianRoute.Works) {
        backStack.clear()
        backStack.add(SujianRoute.Works)
    }
    while (backStack.size > 1 && backStack.last() != target) {
        backStack.removeLastOrNull()
    }
    if (backStack.last() == target) return
    backStack.add(target)
}

private val navForwardTransition: AnimatedContentTransitionScope<Scene<NavKey>>.() -> ContentTransform = {
    val slideIn = slideInHorizontally(animationSpec = tween(220)) { fullWidth -> fullWidth / 8 }
    val slideOut = slideOutHorizontally(animationSpec = tween(220)) { fullWidth -> -fullWidth / 8 }
    (fadeIn(animationSpec = tween(180)) + slideIn) togetherWith
        (fadeOut(animationSpec = tween(150)) + slideOut)
}

private val navPopTransition: AnimatedContentTransitionScope<Scene<NavKey>>.() -> ContentTransform = {
    val slideIn = slideInHorizontally(animationSpec = tween(220)) { fullWidth -> -fullWidth / 8 }
    val slideOut = slideOutHorizontally(animationSpec = tween(220)) { fullWidth -> fullWidth / 8 }
    (fadeIn(animationSpec = tween(150)) + slideIn) togetherWith
        (fadeOut(animationSpec = tween(180)) + slideOut)
}

private val navPredictivePopTransition:
    (AnimatedContentTransitionScope<Scene<NavKey>>, Int) -> ContentTransform = { _, swipeEdge ->
    when (swipeEdge) {
        androidx.navigationevent.NavigationEvent.EDGE_LEFT -> {
            val enter = slideInHorizontally(animationSpec = tween(300)) { fullWidth -> -fullWidth / 3 }
            val exit = slideOutHorizontally(animationSpec = tween(300)) { fullWidth -> fullWidth / 3 }
            (fadeIn(animationSpec = tween(300)) + enter) togetherWith
                (fadeOut(animationSpec = tween(300)) + exit)
        }
        androidx.navigationevent.NavigationEvent.EDGE_RIGHT -> {
            val enter = slideInHorizontally(animationSpec = tween(300)) { fullWidth -> fullWidth / 3 }
            val exit = slideOutHorizontally(animationSpec = tween(300)) { fullWidth -> -fullWidth / 3 }
            (fadeIn(animationSpec = tween(300)) + enter) togetherWith
                (fadeOut(animationSpec = tween(300)) + exit)
        }
        else -> fadeIn(animationSpec = tween(300)) togetherWith fadeOut(animationSpec = tween(300))
    }
}

internal fun predictiveBackStateFraction(progress: Float): Float =
    PREDICTIVE_BACK_EASING.transform(progress) * SINGLE_PANE_PROGRESS_RATIO

private val PREDICTIVE_BACK_EASING: androidx.compose.animation.core.CubicBezierEasing =
    androidx.compose.animation.core.CubicBezierEasing(0.1f, 0.1f, 0f, 1f)

internal const val SINGLE_PANE_PROGRESS_RATIO: Float = 0.1f

/**
 * 普通手机竖屏判定 — 同时满足：Compact 宽度、当前高度大于宽度、
 * 无折叠特征（foldPosture None）、设备类别为普通 phone。
 *
 * #597：不再用于分流到不同 suite，仅用于判断导航栏类型。
 * 保留 internal 可见性供测试验证。
 */
internal fun resolvePhonePortraitPolicy(
    windowSizeClass: WindowSizeClass,
    screenWidthDp: Int,
    screenHeightDp: Int,
    foldPosture: com.xiwei.sujian.platform.api.FoldPosture,
    deviceCategory: com.xiwei.sujian.platform.api.DeviceCategory,
): Boolean =
    windowSizeClass == WindowSizeClass.Compact &&
        screenHeightDp > screenWidthDp &&
        foldPosture == com.xiwei.sujian.platform.api.FoldPosture.None &&
        deviceCategory == com.xiwei.sujian.platform.api.DeviceCategory.Phone
