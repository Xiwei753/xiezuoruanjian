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
import androidx.compose.ui.platform.LocalContext
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
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.di.SujianAppDependencies
import com.xiwei.sujian.app.presentation.AndroidChromePolicy
import com.xiwei.sujian.app.presentation.AndroidLayoutSpec
import com.xiwei.sujian.app.presentation.AndroidNavigationPresentation
import com.xiwei.sujian.app.presentation.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.PresentationContractBridge
import com.xiwei.sujian.app.presentation.SujianChromeAction
import com.xiwei.sujian.app.presentation.SujianChromeSpec
import com.xiwei.sujian.app.presentation.rememberAndroidLayoutSpec
import com.xiwei.sujian.app.presentation.rememberWorkspaceActions
import com.xiwei.sujian.core.designsystem.component.SujianIconButton
import com.xiwei.sujian.core.designsystem.component.SujianSnackbar
import com.xiwei.sujian.core.designsystem.component.SujianTopAppBar
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.platform.window.AospFoldFeatureInfo
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
import kotlinx.coroutines.Dispatchers
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
}

private fun SujianRoute.toTopDestination(): SujianDestination =
    when (this) {
        is SujianRoute.Works -> SujianDestination.Works
        is SujianRoute.StarMap -> SujianDestination.StarMap
        is SujianRoute.Stats -> SujianDestination.Stats
        is SujianRoute.Settings -> SujianDestination.Works
    }

internal fun SujianDestination.toRoute(): SujianRoute =
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

/** 顶栏右侧操作 — 顺序由 Core screen contract 的 HeaderTrailing order 决定（#610）：
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
            val pid = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
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
    workspaceNavState: ProjectNavigationState,
    workspaceActions: AndroidWorkspaceActionSpec,
) {
    NavDisplay(
        backStack = backStack,
        onBack = {
            val handled = backStack.size > 1
            if (handled) {
                backStack.removeLastOrNull()
            }
            com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.navBack(handled)
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
                                    workspaceActions = workspaceActions,
                                )
                            is SujianRoute.StarMap ->
                                StarMapPlaceholderScreen(
                                    modifier = Modifier.testTag(SujianSemanticIds.StarMapScreen),
                                )
                            is SujianRoute.Stats -> StatsScreen()
                            is SujianRoute.Settings -> SettingsRoute()
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
    onTopLevelSelected: (SujianDestination) -> Unit,
) {
    NavigationBar(
        windowInsets = WindowInsets(0.dp),
        modifier = Modifier.testTag(SujianSemanticIds.NavigationBar),
    ) {
        SujianDestination.entries.forEach { destination ->
            NavigationBarItem(
                selected = currentTopDestination == destination,
                onClick = { onTopLevelSelected(destination) },
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
    onTopLevelSelected: (SujianDestination) -> Unit,
) {
    NavigationRail(
        modifier = Modifier.fillMaxHeight().testTag(SujianSemanticIds.NavigationRail),
        windowInsets = WindowInsets(0.dp),
    ) {
        SujianDestination.entries.forEach { destination ->
            NavigationRailItem(
                selected = currentTopDestination == destination,
                onClick = { onTopLevelSelected(destination) },
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

/** 工作区 navigator — 在导航套件层创建的唯一实例（#597：返回历史始终同一份）。
 * 布局指令（含折叠铰链 excludedBounds / pane 宽度）来自 AndroidAdaptiveLayoutPolicy（#610）。 */
@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun rememberSujianWorkspaceNavState(
    appState: SujianAppState,
    scaffoldDirective: androidx.compose.material3.adaptive.layout.PaneScaffoldDirective,
): ProjectNavigationState {
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
    val navigator =
        rememberListDetailPaneScaffoldNavigator(
            scaffoldDirective = scaffoldDirective,
            initialDestinationHistory = initialHistory,
        )
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

/** 路由副作用 — 导航诊断事件上报。 */
@Composable
private fun SujianRouteEffects(
    currentRoute: SujianRoute,
    currentTopDestination: SujianDestination,
) {
    LaunchedEffect(currentRoute) {
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.navigation(currentTopDestination.name)
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
    foldingFeatures: List<AospFoldFeatureInfo> = emptyList(),
) {
    val layoutSpec: AndroidLayoutSpec = rememberAndroidLayoutSpec(foldingFeatures)
    val initialStack = rememberInitialNavStack(initialDestination)
    val backStack = rememberNavBackStack(*initialStack.toTypedArray())
    val currentRoute = backStack.lastOrNull() as? SujianRoute ?: SujianRoute.Works
    val currentTopDestination = currentRoute.toTopDestination()
    val snackbarHostState = remember { SnackbarHostState() }
    val topLevelBackStack = rememberSujianTopLevelBackStack()

    val context = LocalContext.current
    val deps = LocalSujianAppDependencies.current
    val syncState by deps.syncStatusRepository.state.collectAsState()
    val coroutineScope = rememberCoroutineScope()

    val workspaceNavState = rememberSujianWorkspaceNavState(appState, layoutSpec.scaffoldDirective)

    val screenRole = AndroidChromePolicy.screenRoleFor(currentRoute, workspaceNavState.currentLocation)
    val screenPolicy = remember(screenRole) { PresentationContractBridge.resolveScreenPolicy(context, screenRole) }
    val workspaceActions = rememberWorkspaceActions(screenPolicy)
    val chrome =
        AndroidChromePolicy.resolve(
            screenRole = screenRole,
            screenPolicy = screenPolicy,
            workspaceLocation = workspaceNavState.currentLocation,
            canWorkspaceNavigateBack = workspaceNavState.canNavigateBack,
            contractShowsPrimaryNavigation = layoutSpec.contract?.showPrimaryNavigation ?: true,
        )

    SujianWorkspaceBackEffects(currentRoute, workspaceNavState, coroutineScope)
    SujianRouteEffects(currentRoute, currentTopDestination)

    val env = SujianTopBarEnv(syncState, coroutineScope, deps, backStack)
    val topBarInfo = rememberSujianTopBarInfo(currentRoute, appState, chrome, env, workspaceNavState)

    val onTopLevelSelected: (SujianDestination) -> Unit = { destination ->
        // 先完成 top-level 状态切换；nav.top_level_switch 诊断由目的地变化的
        // LaunchedEffect 异步记录，交互回调本身只改导航状态。
        topLevelBackStack.saveCurrent(backStack.toList())
        topLevelBackStack.addTopLevel(destination)
        val restored = topLevelBackStack.currentBackStack()
        backStack.clear()
        restored.forEach { backStack.add(it) }
    }

    SujianJankInteractionClearEffect(currentTopDestination)
    SujianProcessStateEffect(currentTopDestination, appState, syncState)

    val navDisplayContent: @Composable () -> Unit = {
        SujianNavDisplayContent(
            backStack,
            appState,
            workspaceNavState,
            workspaceActions,
        )
    }

    if (layoutSpec.navigationPresentation == AndroidNavigationPresentation.BottomBar) {
        SujianCompactNavScaffold(
            modifier = modifier,
            topBarInfo = topBarInfo,
            snackbarHostState = snackbarHostState,
            // #597 正文一：进入正文后隐藏底栏；设置页从顶栏进入，也不再显示底栏。
            // #610：一级导航可见性由 Core 布局契约（键盘/触控单栏时隐藏）决定。
            bottomBar =
                if (chrome.showPrimaryNavigation) {
                    {
                        SujianCompactBottomBar(currentTopDestination, onTopLevelSelected)
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
                        SujianWideRail(currentTopDestination, onTopLevelSelected)
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
    // 枚举覆盖全部分支，semanticTag 恒非空；testTag 需要非空 tag。
    return this.testTag(semanticTag)
}

/**
 * Issue #612 四：判断本次 top-level destination 变化是否属于“一级切换”。
 * 首次组合（[previousTopDestination] 为 null，应用启动）与原地重复选择（前后相同）
 * 都不算一级切换，返回 null 表示不写 interaction 上下文，避免把启动期帧误标为
 * top_level_switch（诊断保真度）。提取为 internal 便于单测正反验证。
 */
internal fun resolveTopLevelSwitchInteraction(
    previousTopDestination: SujianDestination?,
    currentTopDestination: SujianDestination,
): String? =
    if (previousTopDestination == null || previousTopDestination == currentTopDestination) {
        null
    } else {
        "top_level_switch"
    }

/** Issue #612 四：用 PerformanceMetricsState 写 screen/interaction 上下文；切换动画结束后移除 interaction。 */
@Composable
private fun SujianJankInteractionClearEffect(currentTopDestination: SujianDestination) {
    val view = androidx.compose.ui.platform.LocalView.current
    var previousTopDestination by remember { mutableStateOf<SujianDestination?>(null) }
    LaunchedEffect(currentTopDestination) {
        val holder = androidx.metrics.performance.PerformanceMetricsState.getHolderForHierarchy(view)
        val state = holder?.state
        state?.putState("screen", currentTopDestination.name)
        val interaction =
            resolveTopLevelSwitchInteraction(previousTopDestination, currentTopDestination)
        previousTopDestination = currentTopDestination
        if (interaction == null) return@LaunchedEffect
        state?.putState("interaction", interaction)
        kotlinx.coroutines.delay(350)
        state?.removeState("interaction")
    }
}

/**
 * Issue #612 三、3.2：进程状态摘要唯一写入点。
 *
 * 在“顶级页面切换 / 进入退出正文 / 同步状态变化”三种关键状态变化时更新
 * screen=…;editor=…;sync=…，让下次冷启动读取 ApplicationExitInfo 时知道进程
 * 死前停在哪个状态。三个 key 一起监听，保证任意一次变化后摘要都收敛到真实状态
 * （不会出现切页后把 sync 覆盖成占位值、而同步状态未变导致不再纠正的陈旧问题）。
 */
@Composable
private fun SujianProcessStateEffect(
    currentTopDestination: SujianDestination,
    appState: SujianAppState,
    syncState: com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState,
) {
    val context = LocalContext.current
    LaunchedEffect(currentTopDestination, appState.currentChapterId, syncState) {
        withContext(Dispatchers.IO) {
            com.xiwei.sujian.core.diagnostics.ProcessStateSummary.update(
                context,
                currentTopDestination.name,
                if (appState.currentChapterId != null) "1" else "0",
                syncIndicatorSummary(syncState),
            )
        }
    }
}

/**
 * Issue #612 三、3.2：同步状态摘要字符串。提取为 internal 便于单测正反验证。
 */
internal fun syncIndicatorSummary(syncState: SyncIndicatorState): String =
    when (syncState) {
        SyncIndicatorState.Syncing -> "syncing"
        SyncIndicatorState.Synced -> "synced"
        SyncIndicatorState.Failed -> "failed"
        SyncIndicatorState.Unconfigured -> "unconfigured"
    }

private val navForwardTransition: AnimatedContentTransitionScope<Scene<NavKey>>.() -> ContentTransform = {
    fadeIn(animationSpec = tween(180)) togetherWith fadeOut(animationSpec = tween(150))
}

private val navPopTransition: AnimatedContentTransitionScope<Scene<NavKey>>.() -> ContentTransform = {
    fadeIn(animationSpec = tween(150)) togetherWith fadeOut(animationSpec = tween(180))
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
