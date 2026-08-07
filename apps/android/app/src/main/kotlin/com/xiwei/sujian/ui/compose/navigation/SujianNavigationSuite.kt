package com.xiwei.sujian.ui.compose.navigation

import androidx.activity.compose.BackHandler
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
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
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
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldPredictiveBackHandler
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.scene.Scene
import androidx.navigation3.ui.NavDisplay
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianIconButton
import com.xiwei.sujian.designsystem.component.SujianSnackbar
import com.xiwei.sujian.designsystem.component.SujianTopAppBar
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.model.SyncIndicatorState
import com.xiwei.sujian.model.SyncTrigger
import com.xiwei.sujian.platform.api.WindowSizeClass
import com.xiwei.sujian.runtime.LocalSujianAppDependencies
import com.xiwei.sujian.runtime.SujianAppDependencies
import com.xiwei.sujian.ui.compose.LocalAndroidCapabilities
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.settings.SettingsRoute
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen
import com.xiwei.sujian.ui.compose.stats.StatsScreen
import com.xiwei.sujian.ui.compose.workspace.ProjectWorkspaceScreen
import com.xiwei.sujian.ui.compose.workspace.WorkspaceNavigationState
import com.xiwei.sujian.ui.compose.workspace.buildInitialHistory
import com.xiwei.sujian.ui.compose.workspace.deriveRestoreDestination
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

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
}

private fun SujianRoute.toTopDestination(): SujianDestination =
    when (this) {
        is SujianRoute.Works -> SujianDestination.Works
        is SujianRoute.StarMap -> SujianDestination.StarMap
        is SujianRoute.Stats -> SujianDestination.Stats
        is SujianRoute.Settings -> SujianDestination.Works
    }

private fun SujianDestination.toRoute(): SujianRoute =
    when (this) {
        SujianDestination.Works -> SujianRoute.Works
        SujianDestination.StarMap -> SujianRoute.StarMap
        SujianDestination.Stats -> SujianRoute.Stats
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

@Stable
private class SujianTopBarInfo(
    val title: String,
    val navigationIcon: ImageVector?,
    val onNavigationClick: (() -> Unit)?,
    val actions: @Composable () -> Unit,
    val transparent: Boolean,
)

/** 顶栏操作所需的环境依赖 — 打包传递，避免函数参数超出门禁阈值。 */
private data class SujianTopBarEnv(
    val syncState: SyncIndicatorState,
    val coroutineScope: CoroutineScope,
    val deps: SujianAppDependencies,
    val backStack: NavBackStack<NavKey>,
)

private fun rememberInitialNavStack(initialDestination: String?): List<SujianRoute> =
    when (initialDestination) {
        "settings" -> listOf(SujianRoute.Works, SujianRoute.Settings)
        "starmap" -> listOf(SujianRoute.Works, SujianRoute.StarMap)
        "stats" -> listOf(SujianRoute.Works, SujianRoute.Stats)
        else -> listOf(SujianRoute.Works)
    }

@Composable
private fun rememberSujianTopBarTitle(
    currentRoute: SujianRoute,
    appState: SujianAppState,
    starMapTopBarState: StarMapTopBarState,
): String =
    when (currentRoute) {
        is SujianRoute.Works -> {
            if (appState.currentProjectId != null) {
                appState.currentProjectTitle.ifEmpty { stringResource(id = R.string.title_projects) }
            } else {
                stringResource(id = R.string.title_projects)
            }
        }
        is SujianRoute.StarMap -> {
            starMapTopBarState.title.ifEmpty { stringResource(id = R.string.title_starmap) }
        }
        is SujianRoute.Stats -> stringResource(id = R.string.title_stats)
        is SujianRoute.Settings -> stringResource(id = R.string.action_settings)
    }

/** 顶栏返回逻辑 — 先生成唯一的返回动作，再决定是否显示图标（#597 评论问题三）。
 * 工作区内的返回（正文→章节树→作品列表）统一走 [WorkspaceNavigationState.back]，
 * 与系统返回、页面返回共用同一个工作区 navigator 历史（返回历史始终同一份）。 */
@Composable
private fun rememberSujianTopBarNavigation(
    currentRoute: SujianRoute,
    starMapTopBarState: StarMapTopBarState,
    env: SujianTopBarEnv,
    workspaceNavState: WorkspaceNavigationState,
): Pair<ImageVector?, (() -> Unit)?> {
    val onNavigationClick: (() -> Unit)? =
        when (currentRoute) {
            is SujianRoute.Settings -> {
                { env.backStack.removeLastOrNull() }
            }
            is SujianRoute.StarMap -> starMapTopBarState.onBack
            is SujianRoute.Works -> {
                if (workspaceNavState.canNavigateBack) {
                    {
                        env.coroutineScope.launch {
                            workspaceNavState.back()
                        }
                    }
                } else {
                    null
                }
            }
            is SujianRoute.Stats -> null
        }
    val navigationIcon = if (onNavigationClick != null) SujianIcons.ArrowBack else null
    return navigationIcon to onNavigationClick
}

/** 顶栏右侧操作 — 顺序由 [SujianChromePolicy] 决定：
 * 作品页依次提供 设置、搜索、同步状态；写作区只保留需要的图标层（设置、同步）。 */
private fun rememberSujianTopBarActions(
    currentRoute: SujianRoute,
    chrome: SujianChromeSpec,
    starMapTopBarState: StarMapTopBarState,
    env: SujianTopBarEnv,
): @Composable () -> Unit =
    {
        if (currentRoute is SujianRoute.StarMap) {
            starMapTopBarState.actions?.invoke()
        }
        if (currentRoute is SujianRoute.Works) {
            chrome.actions.forEach { action ->
                when (action) {
                    SujianChromeAction.Settings ->
                        SujianIconButton(
                            onClick = { env.backStack.add(SujianRoute.Settings) },
                            icon = SujianIcons.Settings,
                            contentDescription = stringResource(id = R.string.action_settings),
                            semanticId = SujianSemanticIds.NavigationSettings,
                        )
                    SujianChromeAction.Search ->
                        SujianIconButton(
                            onClick = { },
                            icon = SujianIcons.Search,
                            contentDescription = stringResource(id = R.string.cd_search_dev),
                            enabled = false,
                        )
                    SujianChromeAction.Sync ->
                        SujianIconButton(
                            onClick = {
                                env.coroutineScope.launch {
                                    env.deps.syncCoordinator.runSync(SyncTrigger.Manual)
                                }
                            },
                            icon =
                                when (env.syncState) {
                                    SyncIndicatorState.Unconfigured -> SujianIcons.CloudOff
                                    SyncIndicatorState.Syncing -> SujianIcons.CloudSync
                                    SyncIndicatorState.Synced -> SujianIcons.CloudDone
                                    SyncIndicatorState.Failed -> SujianIcons.CloudError
                                },
                            contentDescription = stringResource(id = R.string.cd_sync_manual),
                        )
                }
            }
        }
    }

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun SujianNavDisplayContent(
    backStack: NavBackStack<NavKey>,
    appState: SujianAppState,
    starMapTopBarState: StarMapTopBarState,
    settingsDetailSection: SettingsSection?,
    workspaceNavState: WorkspaceNavigationState,
    onSettingsDetailSectionChange: (SettingsSection?) -> Unit,
) {
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
                is SujianRoute ->
                    NavEntry(key) { route ->
                        when (route) {
                            is SujianRoute.Works ->
                                ProjectWorkspaceScreen(
                                    appState = appState,
                                    workspaceNavState = workspaceNavState,
                                )
                            is SujianRoute.StarMap ->
                                StarMapScreen(
                                    topBarState = starMapTopBarState,
                                    modifier = Modifier.testTag(SujianSemanticIds.StarMapScreen),
                                )
                            is SujianRoute.Stats -> StatsScreen()
                            is SujianRoute.Settings ->
                                SettingsRoute(
                                    detailSection = settingsDetailSection,
                                    onDetailSectionChange = onSettingsDetailSectionChange,
                                )
                        }
                    }
                else -> NavEntry(key) {}
            }
        },
    )
}

/** compact 底栏 — 一级入口只保留 作品/星图/统计（#597 评论问题一）。 */
@Composable
private fun SujianCompactBottomBar(
    currentTopDestination: SujianDestination,
    backStack: NavBackStack<NavKey>,
) {
    NavigationBar(
        windowInsets = WindowInsets(0.dp),
    ) {
        SujianDestination.entries.forEach { destination ->
            NavigationBarItem(
                selected = currentTopDestination == destination,
                onClick = { navigateToTopDestination(backStack, destination) },
                icon = {
                    Icon(
                        imageVector =
                            if (currentTopDestination == destination) {
                                destination.selectedIcon
                            } else {
                                destination.unselectedIcon
                            },
                        contentDescription = stringResource(id = destination.labelResId),
                    )
                },
                label = { Text(text = stringResource(id = destination.labelResId)) },
                modifier = Modifier.navItemModifier(destination),
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun SujianCompactNavScaffold(
    modifier: Modifier,
    topBarInfo: SujianTopBarInfo,
    snackbarHostState: SnackbarHostState,
    bottomBar: @Composable () -> Unit,
    navDisplayContent: @Composable () -> Unit,
) {
    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            SujianTopAppBar(
                title = topBarInfo.title,
                navigationIcon = topBarInfo.navigationIcon,
                onNavigationClick = topBarInfo.onNavigationClick,
                actions = topBarInfo.actions,
                transparent = topBarInfo.transparent,
            )
        },
        bottomBar = bottomBar,
        snackbarHost = {
            SnackbarHost(hostState = snackbarHostState) { data ->
                SujianSnackbar(data = data)
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
    ) { innerPadding ->
        Box(
            modifier =
                Modifier
                    .fillMaxSize()
                    .padding(innerPadding)
                    .imePadding(),
        ) {
            navDisplayContent()
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun SujianWideNavScaffold(
    modifier: Modifier,
    topBarInfo: SujianTopBarInfo,
    snackbarHostState: SnackbarHostState,
    currentTopDestination: SujianDestination,
    backStack: NavBackStack<NavKey>,
    navDisplayContent: @Composable () -> Unit,
) {
    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            SujianTopAppBar(
                title = topBarInfo.title,
                navigationIcon = topBarInfo.navigationIcon,
                onNavigationClick = topBarInfo.onNavigationClick,
                actions = topBarInfo.actions,
                transparent = topBarInfo.transparent,
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
            modifier =
                Modifier
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
                                imageVector =
                                    if (currentTopDestination == destination) {
                                        destination.selectedIcon
                                    } else {
                                        destination.unselectedIcon
                                    },
                                contentDescription = stringResource(id = destination.labelResId),
                            )
                        },
                        label = { Text(text = stringResource(id = destination.labelResId)) },
                        modifier = Modifier.navItemModifier(destination),
                    )
                }
            }
            Box(
                modifier =
                    Modifier
                        .weight(1f)
                        .fillMaxHeight(),
            ) {
                navDisplayContent()
            }
        }
    }
}

/** 工作区 navigator — 在导航套件层创建的唯一实例（#597：返回历史始终同一份）。 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun rememberSujianWorkspaceNavState(appState: SujianAppState): WorkspaceNavigationState {
    val initialDestination =
        remember(
            appState.currentProjectId,
            appState.currentVolumeId,
            appState.currentChapterId,
        ) {
            deriveRestoreDestination(
                appState.currentProjectId,
                appState.currentVolumeId,
                appState.currentChapterId,
            )
        }
    val initialHistory = remember(initialDestination) { buildInitialHistory(initialDestination) }
    val navigator = rememberListDetailPaneScaffoldNavigator(initialDestinationHistory = initialHistory)
    return remember { WorkspaceNavigationState(navigator) }
}

/** 工作区返回处理 — 系统返回/预测返回（正文→章节树→作品列表）。
 * NavDisplay 只在全局栈可弹出（如设置页）时处理返回；Works 根时由这里接管。 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun SujianWorkspaceBackEffects(
    currentRoute: SujianRoute,
    workspaceNavState: WorkspaceNavigationState,
    coroutineScope: CoroutineScope,
) {
    if (currentRoute is SujianRoute.Works) {
        ThreePaneScaffoldPredictiveBackHandler(
            navigator = workspaceNavState.navigator,
            backBehavior = BackNavigationBehavior.PopUntilScaffoldValueChange,
        )
        BackHandler(enabled = workspaceNavState.canNavigateBack) {
            coroutineScope.launch {
                workspaceNavState.back()
            }
        }
    }
}

/** 路由副作用 — 导航诊断、星图顶栏状态清理、设置详情默认分类。 */
@Composable
private fun SujianRouteEffects(
    currentRoute: SujianRoute,
    currentTopDestination: SujianDestination,
    starMapTopBarState: StarMapTopBarState,
    settingsDetailSection: SettingsSection?,
    onSettingsDetailSectionChange: (SettingsSection?) -> Unit,
) {
    LaunchedEffect(currentRoute) {
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.navigation(currentTopDestination.name)
        if (currentRoute !is SujianRoute.StarMap) {
            starMapTopBarState.clear()
        }
        if (currentRoute !is SujianRoute.Settings) {
            onSettingsDetailSectionChange(null)
        }
        if (currentRoute is SujianRoute.Settings && settingsDetailSection == null) {
            onSettingsDetailSectionChange(SettingsSection.Appearance)
        }
    }
}

/** 顶栏信息 — 标题/返回/操作/透明背景 全部由同一份 [SujianChromeSpec] 决策驱动。 */
@Composable
private fun rememberSujianTopBarInfo(
    currentRoute: SujianRoute,
    appState: SujianAppState,
    chrome: SujianChromeSpec,
    starMapTopBarState: StarMapTopBarState,
    env: SujianTopBarEnv,
    workspaceNavState: WorkspaceNavigationState,
): SujianTopBarInfo {
    val topBarNavigation =
        rememberSujianTopBarNavigation(currentRoute, starMapTopBarState, env, workspaceNavState)
    return SujianTopBarInfo(
        title =
            if (chrome.showTitle) {
                rememberSujianTopBarTitle(currentRoute, appState, starMapTopBarState)
            } else {
                ""
            },
        navigationIcon = topBarNavigation.first,
        onNavigationClick = topBarNavigation.second,
        actions =
            rememberSujianTopBarActions(
                currentRoute,
                chrome,
                starMapTopBarState,
                env,
            ),
        transparent = chrome.appBarTransparent,
    )
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun SujianNavigationSuite(
    appState: SujianAppState,
    modifier: Modifier = Modifier,
    initialDestination: String? = null,
) {
    val capabilities = LocalAndroidCapabilities.current
    val isCompact = capabilities.windowSizeClass == WindowSizeClass.Compact
    val initialStack = rememberInitialNavStack(initialDestination)
    val backStack = rememberNavBackStack(*initialStack.toTypedArray())
    val currentRoute = backStack.lastOrNull() as? SujianRoute ?: SujianRoute.Works
    val currentTopDestination = currentRoute.toTopDestination()
    val snackbarHostState = remember { SnackbarHostState() }
    val starMapTopBarState = remember { StarMapTopBarState() }
    var settingsDetailSection by remember { mutableStateOf<SettingsSection?>(null) }

    val deps = LocalSujianAppDependencies.current
    val syncState by deps.syncStatusRepository.state.collectAsState()
    val coroutineScope = rememberCoroutineScope()

    val workspaceNavState = rememberSujianWorkspaceNavState(appState)

    // #597 评论问题一：写作区隐藏底栏、顶栏透明且不显示标题；
    // 评论问题三：返回箭头与唯一返回动作来自同一个决策。
    val chrome =
        SujianChromePolicy.resolve(
            route = currentRoute,
            workspaceLocation = workspaceNavState.currentLocation,
            canWorkspaceNavigateBack = workspaceNavState.canNavigateBack,
            starMapHasBack = starMapTopBarState.onBack != null,
            isCompact = isCompact,
        )

    SujianWorkspaceBackEffects(currentRoute, workspaceNavState, coroutineScope)

    SujianRouteEffects(
        currentRoute,
        currentTopDestination,
        starMapTopBarState,
        settingsDetailSection,
    ) { settingsDetailSection = it }

    val env = SujianTopBarEnv(syncState, coroutineScope, deps, backStack)
    val topBarInfo =
        rememberSujianTopBarInfo(
            currentRoute,
            appState,
            chrome,
            starMapTopBarState,
            env,
            workspaceNavState,
        )

    val navDisplayContent: @Composable () -> Unit = {
        SujianNavDisplayContent(
            backStack,
            appState,
            starMapTopBarState,
            settingsDetailSection,
            workspaceNavState,
        ) { settingsDetailSection = it }
    }

    if (isCompact) {
        SujianCompactNavScaffold(
            modifier = modifier,
            topBarInfo = topBarInfo,
            snackbarHostState = snackbarHostState,
            // #597 评论问题一：进入正文后隐藏底栏；设置页从顶栏进入，也不再显示底栏。
            bottomBar =
                if (chrome.showBottomBar) {
                    {
                        SujianCompactBottomBar(currentTopDestination, backStack)
                    }
                } else {
                    {}
                },
            navDisplayContent = navDisplayContent,
        )
    } else {
        SujianWideNavScaffold(
            modifier = modifier,
            topBarInfo = topBarInfo,
            snackbarHostState = snackbarHostState,
            currentTopDestination = currentTopDestination,
            backStack = backStack,
            navDisplayContent = navDisplayContent,
        )
    }
}

private fun Modifier.navItemModifier(destination: SujianDestination): Modifier {
    val semanticTag =
        when (destination) {
            SujianDestination.Works -> SujianSemanticIds.NavigationWorks
            SujianDestination.StarMap -> SujianSemanticIds.NavigationStarMap
            SujianDestination.Stats -> null
        }
    return if (semanticTag != null) this.testTag(semanticTag) else this
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
