package com.xiwei.sujian.app.navigation

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
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
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
import androidx.compose.ui.graphics.Color
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
import androidx.window.core.layout.WindowSizeClass
import com.xiwei.sujian.R
import com.xiwei.sujian.app.LocalSujianAppDependencies
import com.xiwei.sujian.app.SujianAppDependencies
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.component.SujianSnackbar
import com.xiwei.sujian.core.designsystem.component.SujianTopAppBar
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.feature.project.ui.ProjectNavigationState
import com.xiwei.sujian.feature.project.ui.ProjectWorkspaceScreen
import com.xiwei.sujian.feature.project.ui.buildInitialHistory
import com.xiwei.sujian.feature.project.ui.deriveRestoreDestination
import com.xiwei.sujian.feature.settings.ui.SettingsRoute
import com.xiwei.sujian.feature.starmap.ui.StarMapPlaceholderScreen
import com.xiwei.sujian.feature.stats.ui.StatsScreen
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
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
private class SujianTopBarInfo(
    val title: String,
    val navigationIcon: ImageVector?,
    val onNavigationClick: (() -> Unit)?,
    val actions: @Composable () -> Unit,
    val containerColor: Color,
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
): String =
    when (currentRoute) {
        is SujianRoute.Works -> {
            if (appState.currentProjectId != null) {
                appState.currentProjectTitle.ifEmpty { stringResource(id = R.string.title_projects) }
            } else {
                stringResource(id = R.string.title_projects)
            }
        }
        is SujianRoute.StarMap -> stringResource(id = R.string.title_starmap)
        is SujianRoute.Stats -> stringResource(id = R.string.title_stats)
        is SujianRoute.Settings -> stringResource(id = R.string.action_settings)
    }

/** 顶栏返回逻辑 — 先生成唯一的返回动作，再决定是否显示图标（#597 评论问题三）。
 * 工作区内的返回（正文→章节树→作品列表）统一走 [ProjectNavigationState.back]，
 * 与系统返回、页面返回共用同一个工作区 navigator 历史（返回历史始终同一份）。 */
@Composable
private fun rememberSujianTopBarNavigation(
    currentRoute: SujianRoute,
    env: SujianTopBarEnv,
    workspaceNavState: ProjectNavigationState,
): Pair<ImageVector?, (() -> Unit)?> {
    val onNavigationClick: (() -> Unit)? =
        when (currentRoute) {
            is SujianRoute.Settings -> {
                { env.backStack.removeLastOrNull() }
            }
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
            is SujianRoute.StarMap -> null
            is SujianRoute.Stats -> null
        }
    val navigationIcon = if (onNavigationClick != null) SujianIcons.ArrowBack else null
    return navigationIcon to onNavigationClick
}

/** 顶栏右侧操作 — 顺序由 [SujianChromePolicy] 决定：
 * 作品页依次提供 同步状态、搜索、设置（视觉从右往左为 设置/搜索/同步）；
 * 写作区只保留需要的图标层（同步、设置）。 */

private fun rememberSujianTopBarActions(
    currentRoute: SujianRoute,
    chrome: SujianChromeSpec,
    env: SujianTopBarEnv,
): @Composable () -> Unit =
    {
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
                            semanticId = SujianSemanticIds.NavigationSearch,
                        )
                    SujianChromeAction.Sync ->
                        SujianIconButton(
                            onClick = rememberSujianManualSyncOnClick(env),
                            icon =
                                when (env.syncState) {
                                    SyncIndicatorState.Unconfigured -> SujianIcons.CloudOff
                                    SyncIndicatorState.Syncing -> SujianIcons.CloudSync
                                    SyncIndicatorState.Synced -> SujianIcons.CloudDone
                                    SyncIndicatorState.Failed -> SujianIcons.CloudError
                                },
                            contentDescription = stringResource(id = R.string.cd_sync_manual),
                            semanticId = SujianSemanticIds.NavigationSync,
                        )
                }
            }
        }
    }

/** #600：手动同步 onClick — 提取为独立函数降低 rememberSujianTopBarActions 认知复杂度。 */
private fun rememberSujianManualSyncOnClick(env: SujianTopBarEnv): () -> Unit =
    {
        env.coroutineScope.launch {
            // sync 已改为 per-project — 手动同步针对当前活动作品。
            val pid = com.xiwei.sujian.core.interop.project.ActiveProjectGate.currentProjectId()
            if (pid != null) {
                env.deps.syncCoordinator.runSync(SyncTrigger.Manual, pid)
            }
        }
    }

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun SujianNavDisplayContent(
    backStack: NavBackStack<NavKey>,
    appState: SujianAppState,
    settingsDetailSection: SettingsSection?,
    workspaceNavState: ProjectNavigationState,
    onSettingsDetailSectionChange: (SettingsSection?) -> Unit,
) {
    NavDisplay(
        backStack = backStack,
        onBack = {
            val handled = backStack.size > 1
            if (handled) {
                backStack.removeLastOrNull()
            }
            com.xiwei.sujian.app.diagnostics.DiagnosticsEvents.navBack(handled)
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
                                StarMapPlaceholderScreen(
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
        modifier = Modifier.testTag(SujianSemanticIds.NavigationBar),
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
                containerColor = topBarInfo.containerColor,
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

/** 宽窗口一级导航 — NavigationRail 容器（#597 九：NavigationRail 稳定语义 ID）。 */
@Composable
private fun SujianWideRail(
    currentTopDestination: SujianDestination,
    backStack: NavBackStack<NavKey>,
) {
    NavigationRail(
        modifier = Modifier.fillMaxHeight().testTag(SujianSemanticIds.NavigationRail),
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
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun SujianWideNavScaffold(
    modifier: Modifier,
    topBarInfo: SujianTopBarInfo,
    snackbarHostState: SnackbarHostState,
    rail: (@Composable () -> Unit)?,
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
                containerColor = topBarInfo.containerColor,
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
            rail?.invoke()
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
private fun rememberSujianWorkspaceNavState(appState: SujianAppState): ProjectNavigationState {
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
    return remember { ProjectNavigationState(navigator) }
}

/** 工作区返回处理 — 系统返回/预测返回（正文→章节树→作品列表）。
 * NavDisplay 只在全局栈可弹出（如设置页）时处理返回；Works 根时由这里接管。 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun SujianWorkspaceBackEffects(
    currentRoute: SujianRoute,
    workspaceNavState: ProjectNavigationState,
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

/** 路由副作用 — 导航诊断、设置详情默认分类。 */
@Composable
private fun SujianRouteEffects(
    currentRoute: SujianRoute,
    currentTopDestination: SujianDestination,
    settingsDetailSection: SettingsSection?,
    onSettingsDetailSectionChange: (SettingsSection?) -> Unit,
) {
    LaunchedEffect(currentRoute) {
        com.xiwei.sujian.app.diagnostics.DiagnosticsEvents.navigation(currentTopDestination.name)
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
    env: SujianTopBarEnv,
    workspaceNavState: ProjectNavigationState,
): SujianTopBarInfo {
    val topBarNavigation =
        rememberSujianTopBarNavigation(currentRoute, env, workspaceNavState)
    return SujianTopBarInfo(
        title =
            if (chrome.showTitle) {
                rememberSujianTopBarTitle(currentRoute, appState)
            } else {
                ""
            },
        navigationIcon = topBarNavigation.first,
        onNavigationClick = topBarNavigation.second,
        actions = rememberSujianTopBarActions(currentRoute, chrome, env),
        containerColor =
            if (chrome.appBarTransparent) {
                Color.Transparent
            } else {
                MaterialTheme.colorScheme.surface
            },
    )
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun SujianNavigationSuite(
    appState: SujianAppState,
    modifier: Modifier = Modifier,
    initialDestination: String? = null,
) {
    val windowSizeClass = currentWindowAdaptiveInfo().windowSizeClass
    val isCompact =
        !windowSizeClass.isWidthAtLeastBreakpoint(
            WindowSizeClass.WIDTH_DP_MEDIUM_LOWER_BOUND,
        )
    val initialStack = rememberInitialNavStack(initialDestination)
    val backStack = rememberNavBackStack(*initialStack.toTypedArray())
    val currentRoute = backStack.lastOrNull() as? SujianRoute ?: SujianRoute.Works
    val currentTopDestination = currentRoute.toTopDestination()
    val snackbarHostState = remember { SnackbarHostState() }
    var settingsDetailSection by remember { mutableStateOf<SettingsSection?>(null) }

    val deps = LocalSujianAppDependencies.current
    val syncState by deps.syncStatusRepository.state.collectAsState()
    val coroutineScope = rememberCoroutineScope()

    val workspaceNavState = rememberSujianWorkspaceNavState(appState)

    // #597 正文一：写作区隐藏一级导航（底栏/侧栏）、顶栏透明且不显示标题；
    // 正文三：返回箭头与唯一返回动作来自同一个决策。
    val chrome =
        SujianChromePolicy.resolve(
            route = currentRoute,
            workspaceLocation = workspaceNavState.currentLocation,
            canWorkspaceNavigateBack = workspaceNavState.canNavigateBack,
        )

    SujianWorkspaceBackEffects(currentRoute, workspaceNavState, coroutineScope)

    SujianRouteEffects(
        currentRoute,
        currentTopDestination,
        settingsDetailSection,
    ) { settingsDetailSection = it }

    val env = SujianTopBarEnv(syncState, coroutineScope, deps, backStack)
    val topBarInfo =
        rememberSujianTopBarInfo(
            currentRoute,
            appState,
            chrome,
            env,
            workspaceNavState,
        )

    val navDisplayContent: @Composable () -> Unit = {
        SujianNavDisplayContent(
            backStack,
            appState,
            settingsDetailSection,
            workspaceNavState,
        ) { settingsDetailSection = it }
    }

    if (isCompact) {
        SujianCompactNavScaffold(
            modifier = modifier,
            topBarInfo = topBarInfo,
            snackbarHostState = snackbarHostState,
            // #597 正文一：进入正文后隐藏底栏；设置页从顶栏进入，也不再显示底栏。
            bottomBar =
                if (chrome.showPrimaryNavigation) {
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
            // #597 正文一：宽窗口同一套规则 — Settings/Editor 不创建 NavigationRail。
            rail =
                if (chrome.showPrimaryNavigation) {
                    {
                        SujianWideRail(currentTopDestination, backStack)
                    }
                } else {
                    null
                },
            navDisplayContent = navDisplayContent,
        )
    }
}

private fun Modifier.navItemModifier(destination: SujianDestination): Modifier {
    val semanticTag =
        when (destination) {
            SujianDestination.Works -> SujianSemanticIds.NavigationWorks
            SujianDestination.StarMap -> SujianSemanticIds.NavigationStarMap
            SujianDestination.Stats -> SujianSemanticIds.NavigationStats
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
