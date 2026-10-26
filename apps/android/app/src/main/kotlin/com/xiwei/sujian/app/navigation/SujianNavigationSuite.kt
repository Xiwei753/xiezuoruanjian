package com.xiwei.sujian.app.navigation

import android.annotation.SuppressLint
import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.ui.NavDisplay
import com.xiwei.sujian.R
import com.xiwei.sujian.app.SujianAppState
import com.xiwei.sujian.app.WorkspaceUiEvent
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.di.SujianAppDependencies
import com.xiwei.sujian.app.state.ActiveDocumentGate
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.platform.window.AospFoldFeatureInfo
import com.xiwei.sujian.feature.project.ui.EditorWorkspaceCallbacks
import com.xiwei.sujian.feature.project.ui.ProjectNavigationState
import com.xiwei.sujian.feature.project.ui.ProjectWorkspaceScreen
import com.xiwei.sujian.feature.project.ui.SujianChromeSpec
import com.xiwei.sujian.feature.project.ui.WorkspaceLocation
import com.xiwei.sujian.feature.project.ui.WorkspaceNavigator
import com.xiwei.sujian.feature.project.ui.buildInitialHistory
import com.xiwei.sujian.feature.project.ui.deriveRestoreDestination
import com.xiwei.sujian.feature.search.data.model.SearchContext
import com.xiwei.sujian.feature.search.data.model.SearchOriginScreens
import com.xiwei.sujian.feature.search.ui.SearchScreen
import com.xiwei.sujian.feature.settings.ui.SettingsRoute
import com.xiwei.sujian.feature.starmap.ui.StarMapScreen
import com.xiwei.sujian.feature.stats.ui.StatsScreen
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
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
        is SujianRoute.Search ->
            runCatching { SujianDestination.valueOf(topLevel) }
                .getOrDefault(SujianDestination.Works)
    }

internal fun SujianDestination.toRoute(): SujianRoute =
    when (this) {
        SujianDestination.Works -> SujianRoute.Works
        SujianDestination.StarMap -> SujianRoute.StarMap()
        SujianDestination.Stats -> SujianRoute.Stats
    }

private fun rememberInitialNavStack(initialDestination: String?): Pair<SujianDestination, List<SujianRoute>> =
    when (initialDestination) {
        "settings" -> SujianDestination.Works to listOf(SujianRoute.Works, SujianRoute.Settings)
        "starmap" -> SujianDestination.StarMap to listOf(SujianRoute.StarMap())
        "stats" -> SujianDestination.Stats to listOf(SujianRoute.Stats)
        else -> SujianDestination.Works to listOf(SujianRoute.Works)
    }

/**
 * 导航内容上下文 — 打包传递，避免函数参数超出门禁阈值。
 *
 * 宽/窄工作区模式（[isWideLayout]）与作品卡最小宽度按 Android 本端窗口尺寸在导航套件层
 * 统一决定；pane 收起状态与 chrome 也在这里统一持有，工作区与外层壳层消费同一份。
 */
private data class SujianNavContext(
    val appState: SujianAppState,
    val workspaceNavState: ProjectNavigationState,
    val isWideLayout: Boolean,
    val projectCardMinWidthDp: Float,
    val chapterTreeCollapsed: Boolean,
    val toolPaneCollapsed: Boolean,
    val onToggleChapterTree: () -> Unit,
    val onToggleToolPane: () -> Unit,
    val chrome: SujianChromeSpec,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SujianNavDisplayContent(
    topLevelBackStack: SujianTopLevelBackStack,
    context: SujianNavContext,
    deps: SujianAppDependencies,
    syncState: com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState,
) {
    // 每个 top-level 栈绑定自己的 decorated entries（独立 SaveableStateHolder +
    // ViewModelStore decorator）。三个 rememberDecoratedNavEntries 始终在组合中，
    // inactive tab 状态不丢失；NavDisplay 直接消费 entries。
    // entryProvider 按 appState / workspaceNavState 稳定保存，
    // 底栏切换不再顺手重建三套 NavEntry provider。
    val entryProvider =
        rememberSujianEntryProvider(
            context,
            topLevelBackStack,
            deps,
            syncState,
        )

    NavDisplay(
        entries = topLevelBackStack.decoratedEntries(entryProvider),
        onBack = {
            val handled = topLevelBackStack.removeLastOrNull()
            com.xiwei.sujian.core.interop.diagnostics.AppDiagnosticsEvents.navBack(handled)
            handled
        },
        transitionSpec = navForwardTransition,
        popTransitionSpec = navPopTransition,
        predictivePopTransitionSpec = navPredictivePopTransition,
    )
}

/**
 * 工作区 NavEntry provider — 按 [appState] / [workspaceNavState] 稳定保存，
 * 避免每次重组都重建三个 back stack 的 provider。
 */
@Composable
private fun rememberSujianEntryProvider(
    context: SujianNavContext,
    topLevelBackStack: SujianTopLevelBackStack,
    deps: SujianAppDependencies,
    syncState: com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState,
): (NavKey) -> NavEntry<NavKey> {
    // 在 Composable 层创建 EditorWorkspaceCallbacks —
    // 由导航套件层注入同步/设置/搜索回调，不让 project UI 反向依赖 navigation route 类型。
    // rememberCoroutineScope 必须在 Composable 里调用，所以回调创建在这里而不是 rememberWorksEntry。
    val coroutineScope = rememberCoroutineScope()
    val editorCallbacks =
        remember(coroutineScope, deps, topLevelBackStack, context) {
            EditorWorkspaceCallbacks(
                onSync = { coroutineScope.launch { deps.syncCoordinator.runFullSync(SyncTrigger.Manual) } },
                onSettings = { topLevelBackStack.add(SujianRoute.Settings) },
                onSearch = {
                    topLevelBackStack.openSearch(
                        appState = context.appState,
                        workspaceLocation = context.workspaceNavState.currentLocation,
                    )
                },
            )
        }
    return remember(context, editorCallbacks) {
        { key: NavKey ->
            when (key) {
                is SujianRoute ->
                    when (key) {
                        is SujianRoute.Works ->
                            rememberWorksEntry(key, context, topLevelBackStack, deps, editorCallbacks)
                        is SujianRoute.StarMap ->
                            NavEntry(key, metadata = noPageTransitionMetadata) {
                                StarMapScreen(
                                    starmapId = key.starmapId,
                                    projectId = key.projectId,
                                    target = key.selectionTarget(),
                                    onOpenStarmap = { starmapId, projectId ->
                                        topLevelBackStack.add(
                                            SujianRoute.StarMap(
                                                starmapId = starmapId,
                                                projectId = projectId,
                                            ),
                                        )
                                    },
                                    modifier = Modifier.testTag(SujianSemanticIds.StarMapScreen),
                                )
                            }
                        is SujianRoute.Stats ->
                            NavEntry(key, metadata = noPageTransitionMetadata) { route ->
                                StatsScreen()
                            }
                        // Settings 使用 noPageTransitionMetadata，
                        // 禁止旧 Works + 新 Settings 双页 crossfade。
                        // 过渡由 SettingsRoute 自身 graphicsLayer 绘制层动画提供。
                        is SujianRoute.Settings ->
                            NavEntry(key, metadata = noPageTransitionMetadata) { route ->
                                SettingsRoute(
                                    onOpenSearch = { section ->
                                        topLevelBackStack.openSearch(
                                            appState = context.appState,
                                            workspaceLocation = context.workspaceNavState.currentLocation,
                                            settingsSection = section?.searchContextValue(),
                                            originScreenOverride = SearchOriginScreens.Settings,
                                        )
                                    },
                                )
                            }
                        is SujianRoute.Search ->
                            NavEntry(key, metadata = noPageTransitionMetadata) { route ->
                                SearchScreen(
                                    searchContext = key.toSearchContext(),
                                    scope = key.scope,
                                    onOpenResult = { hit ->
                                        openSearchResult(
                                            hit = hit,
                                            appState = context.appState,
                                            workspaceNavState = context.workspaceNavState,
                                            topLevelBackStack = topLevelBackStack,
                                            deps = deps,
                                        )
                                    },
                                )
                            }
                    }
                else -> NavEntry(key) {}
            }
        }
    }
}

/**
 * Works 路由的 NavEntry — 提取以降低 [rememberSujianEntryProvider] 认知复杂度。
 * ProjectWorkspaceScreen 消费本端宽窄模式（isWideLayout）与 chrome — 大屏 Editor 位置画 WideWritingWorkspace。
 *
 * 把 pane 收起/chrome 打包成 WorkbenchPresentationState 传给 ProjectWorkspaceScreen。
 */
private fun rememberWorksEntry(
    key: SujianRoute.Works,
    context: SujianNavContext,
    topLevelBackStack: SujianTopLevelBackStack,
    deps: SujianAppDependencies,
    editorCallbacks: EditorWorkspaceCallbacks,
): NavEntry<NavKey> =
    NavEntry(key, metadata = noPageTransitionMetadata) { route ->
        val workbenchPresentation =
            com.xiwei.sujian.feature.project.ui.WorkbenchPresentationState(
                chapterTreeCollapsed = context.chapterTreeCollapsed,
                toolPaneCollapsed = context.toolPaneCollapsed,
                onToggleChapterTree = context.onToggleChapterTree,
                onToggleToolPane = context.onToggleToolPane,
                chrome = context.chrome,
            )
        ProjectWorkspaceScreen(
            appState = context.appState,
            workspaceNavState = context.workspaceNavState,
            isWideLayout = context.isWideLayout,
            projectCardMinWidthDp = context.projectCardMinWidthDp,
            workbenchPresentation = workbenchPresentation,
            editorCallbacks = editorCallbacks,
        )
    }

/**
 * 一级（底栏/侧栏）切换动效 — 只动画”新页面绘制层”。
 *
 * 不恢复已删除的双页面整页 crossfade：切换期间旧页立即退出，新页只在
 * graphicsLayer 上做 140ms 淡入（0.9 → 1.0），不触发新页面重新布局。
 * noPageTransitionMetadata 保留，Works/StarMap/Stats 不重新挂回全局 transitionSpec。
 */
@Composable
private fun SujianTopLevelSwitchMotion(
    destination: SujianDestination,
    content: @Composable () -> Unit,
) {
    var previousDestination by remember { mutableStateOf<SujianDestination?>(null) }
    // 判断本次组合是否属于一级切换（首次组合/原地重复选择不算）。
    // 注意：previousDestination 在 LaunchedEffect 里更新，重组期间读到的是旧值，
    // 正好用于决定新页面第一帧的初始 alpha。
    val switching = previousDestination != null && previousDestination != destination
    // 按 destination 重建 Animatable：切换时新页面第一帧就直接以 0.9 绘制，
    // 避免评论二原片段“先以 1.0 画一帧再 snapTo(0.9)”的瞬时全透明帧闪现。
    val contentAlpha =
        remember(destination) {
            Animatable(if (switching) 0.9f else 1f)
        }

    LaunchedEffect(destination) {
        previousDestination = destination
        if (!switching) return@LaunchedEffect
        contentAlpha.animateTo(
            targetValue = 1f,
            animationSpec = tween(durationMillis = 140),
        )
    }

    Box(
        modifier =
            Modifier
                .fillMaxSize()
                .graphicsLayer { alpha = contentAlpha.value },
    ) {
        content()
    }
}

/** 工作区 navigator — 在导航套件层创建的唯一实例（返回历史始终同一份）。
 *
 * 改用纯业务 [WorkspaceNavigator]，不再依赖 Material3 Adaptive
 * 的 rememberListDetailPaneScaffoldNavigator。"当前在哪个业务位置"与"屏幕上同时画
 * 哪些区域"彻底分开 — 后者由导航套件按本端窗口尺寸决定，[ProjectWorkspaceScreen] 消费。 */
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
    val navigator = remember { WorkspaceNavigator() }
    // 一次性注入会话恢复初始历史 — 之后导航只使用 navigator 自己保存/恢复的历史。
    LaunchedEffect(initialHistory) {
        navigator.replaceInitialHistory(initialHistory)
    }
    return remember { ProjectNavigationState(navigator) }
}

/**
 * 工作区返回处理 — 系统返回/预测返回（正文→章节树→作品列表）。
 * NavDisplay 只在全局栈可弹出（如设置页）时处理返回；Works 根时由这里接管。
 *
 * PredictiveBackHandler 无条件参与组合（官方要求），只用 enabled 控制：
 * Works 路由且可返回时生效。手势完成后先 flushActiveDocument（保存活动正文），
 * 保存成功再真正导航离开（back）；保存失败不导航，保持 Editor 目的地。
 * 手势取消时 CancellationException 直接向上传播。 */
@Composable
@SuppressLint("NoCollectCallFound")
// NoCollectCallFound 是静态启发式：只认 lambda 内的直接 progressFlow.collect；
// collect 在 runPredictiveWorkspaceBack 内（progressFlow.collect { … }），其语义
// （手势 progress → 保存 → 返回/复位）由 PredictiveWorkspaceBackTest 真实覆盖。
private fun SujianWorkspaceBackEffects(
    currentRoute: SujianRoute,
    workspaceNavState: ProjectNavigationState,
) {
    PredictiveBackHandler(
        enabled = currentRoute is SujianRoute.Works && workspaceNavState.canNavigateBack,
    ) { progressFlow ->
        runPredictiveWorkspaceBack(
            progressFlow = progressFlow,
            onFlushActiveDocument = ActiveDocumentGate::flushActiveDocument,
            onBack = workspaceNavState::back,
        )
    }
}

/**
 * 工作区预测/系统返回的单一执行体（纯逻辑，便于单测）。
 *
 * progressFlow 收集到结束用来等待预测返回手势完成，手势完成后先
 * onFlushActiveDocument 保存活动正文 — 成功才 onBack 导航离开；失败不导航。
 * 手势取消（CancellationException）直接向上传播。
 */
internal suspend fun runPredictiveWorkspaceBack(
    progressFlow: kotlinx.coroutines.flow.Flow<androidx.activity.BackEventCompat>,
    onFlushActiveDocument: suspend () -> Boolean,
    onBack: () -> Unit,
) {
    // 空收集器：只消费 flow 元素，不做任何处理
    // 这里不需要处理每个元素，只需要等待 flow 完成
    // ensureActive() 用于检查当前协程是否被取消，确保取消异常能正确传播
    progressFlow.collect {
        @Suppress("EXPERIMENTAL_API_USAGE")
        kotlinx.coroutines.currentCoroutineContext().ensureActive()
    }
    if (onFlushActiveDocument()) {
        onBack()
    }
}

/** 路由副作用 — 导航诊断事件上报。 */
@Composable
private fun SujianRouteEffects(
    currentRoute: SujianRoute,
    currentTopDestination: SujianDestination,
) {
    LaunchedEffect(currentRoute) {
        com.xiwei.sujian.core.interop.diagnostics.AppDiagnosticsEvents.navigation(currentTopDestination.name)
    }
    // nav.top_level_switch 改由目的地变化的 LaunchedEffect 异步记录，
    // 不在 onTopLevelSelected 交互回调里同步做诊断格式化/落队列。
    // 首次组合（previous 为 null）与原地重复选择（前后相同）都不记录，
    // 与 resolveTopLevelSwitchInteraction 的保真度语义一致。
    var previousTopDestination by remember { mutableStateOf<SujianDestination?>(null) }
    LaunchedEffect(currentTopDestination) {
        val previous = previousTopDestination
        previousTopDestination = currentTopDestination
        if (previous == null || previous == currentTopDestination) return@LaunchedEffect
        withContext(Dispatchers.IO) {
            com.xiwei.sujian.core.interop.diagnostics.AppDiagnosticsEvents.navTopLevelSwitch(
                previous.name,
                currentTopDestination.name,
            )
        }
    }
}

/**
 * 顶层导航套件 — 宽/窄工作区模式、一级导航放置、顶栏 chrome 全部按 Android 本端
 * 窗口尺寸与当前路由决定；不再向 Core 请求布局模式或页面动作契约。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SujianNavigationSuite(
    appState: SujianAppState,
    modifier: Modifier = Modifier,
    initialDestination: String? = null,
    foldingFeatures: List<AospFoldFeatureInfo> = emptyList(),
) {
    val deps = LocalSujianAppDependencies.current
    // 宽/窄工作区模式与一级导航放置：完全依据本端窗口尺寸（含折叠分离铰链几何）。
    val windowLayout = rememberSujianWindowLayout(foldingFeatures)
    val (initialTopLevel, initialStackRoutes) = rememberInitialNavStack(initialDestination)
    val topLevelBackStack = rememberSujianTopLevelBackStack(initialTopLevel, initialStackRoutes)
    val backStack = topLevelBackStack.backStack
    val currentRoute = backStack.lastOrNull() as? SujianRoute ?: SujianRoute.Works
    val currentTopDestination = currentRoute.toTopDestination()
    val snackbarHostState = remember { SnackbarHostState() }

    // pane 收起状态是 Android 局部 UI 状态，在导航套件层持有，工作区与外层壳层消费同一份。
    var chapterTreeCollapsed by rememberSaveable { mutableStateOf(false) }
    var toolPaneCollapsed by rememberSaveable { mutableStateOf(false) }
    val onToggleChapterTree: () -> Unit = remember { { chapterTreeCollapsed = !chapterTreeCollapsed } }
    val onToggleToolPane: () -> Unit = remember { { toolPaneCollapsed = !toolPaneCollapsed } }

    // 收集 ViewModel 抛出的 UI 事件，错误走 Snackbar 展示。
    SujianUiEventEffect(appState, snackbarHostState)

    val syncState by deps.syncStatusRepository.state.collectAsState()
    val coroutineScope = rememberCoroutineScope()

    val workspaceNavState = rememberSujianWorkspaceNavState(appState)

    // 顶栏 chrome（返回/标题/透明底/一级导航可见性/右侧动作）由本端按路由与工作区位置决定。
    val chrome =
        resolveSujianChrome(
            currentRoute = currentRoute,
            workspaceLocation = workspaceNavState.currentLocation,
            canWorkspaceNavigateBack = workspaceNavState.canNavigateBack,
        )

    SujianWorkspaceBackEffects(currentRoute, workspaceNavState)
    SujianRouteEffects(currentRoute, currentTopDestination)
    val onOpenSearch: () -> Unit = {
        topLevelBackStack.openSearch(
            appState = appState,
            workspaceLocation = workspaceNavState.currentLocation,
        )
    }
    val env = SujianTopBarEnv(syncState, coroutineScope, deps, topLevelBackStack, onOpenSearch)
    val topBarInfo = rememberSujianTopBarInfo(currentRoute, appState, chrome, env, workspaceNavState)

    // 底栏点击只调 addTopLevel；同一已选中项由 addTopLevel 内部早退。
    val onTopLevelSelected: (SujianDestination) -> Unit = { destination ->
        topLevelBackStack.addTopLevel(destination)
    }

    SujianJankInteractionClearEffect(currentRoute, currentTopDestination, workspaceNavState)
    SujianProcessStateEffect(currentTopDestination, appState, syncState)

    val isEditorLocation =
        currentRoute is SujianRoute.Works &&
            workspaceNavState.currentLocation is WorkspaceLocation.Editor

    val navContext =
        rememberSujianNavContext(
            appState = appState,
            workspaceNavState = workspaceNavState,
            isWideLayout = windowLayout.isWorkbench,
            projectCardMinWidthDp = SujianWorkbenchDimensions.projectCardMinWidthDp,
            chapterTreeCollapsed = chapterTreeCollapsed,
            toolPaneCollapsed = toolPaneCollapsed,
            onToggleChapterTree = onToggleChapterTree,
            onToggleToolPane = onToggleToolPane,
            chrome = chrome,
        )
    val scaffoldState =
        rememberSujianNavScaffoldState(
            isEditorLocation = isEditorLocation,
            topBarInfo = topBarInfo,
            snackbarHostState = snackbarHostState,
            currentTopDestination = currentTopDestination,
            topLevelBackStack = topLevelBackStack,
            navContext = navContext,
            deps = deps,
            syncState = syncState,
        )

    androidx.compose.foundation.layout.Box(modifier = modifier) {
        SujianNavScaffoldContent(
            modifier = Modifier.fillMaxSize(),
            useBottomNavigation = windowLayout.useBottomNavigation,
            chrome = chrome,
            scaffoldChrome = scaffoldState.scaffoldChrome,
            selection = SujianTopLevelSelection(currentTopDestination, onTopLevelSelected),
            navDisplayContent = scaffoldState.navDisplayContent,
            containerColor = scaffoldState.containerColor,
        )
    }
}

/**
 * 构造 [SujianNavContext] — 提取以降低 [SujianNavigationSuite] 行数。
 */
@Composable
@Suppress("LongParameterList") // 8 参数达 threshold，函数级 suppress（既有先例）
private fun rememberSujianNavContext(
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    isWideLayout: Boolean,
    projectCardMinWidthDp: Float,
    chapterTreeCollapsed: Boolean,
    toolPaneCollapsed: Boolean,
    onToggleChapterTree: () -> Unit,
    onToggleToolPane: () -> Unit,
    chrome: SujianChromeSpec,
): SujianNavContext =
    SujianNavContext(
        appState = appState,
        workspaceNavState = workspaceNavState,
        isWideLayout = isWideLayout,
        projectCardMinWidthDp = projectCardMinWidthDp,
        chapterTreeCollapsed = chapterTreeCollapsed,
        toolPaneCollapsed = toolPaneCollapsed,
        onToggleChapterTree = onToggleChapterTree,
        onToggleToolPane = onToggleToolPane,
        chrome = chrome,
    )

/**
 * 构造外层 Scaffold 状态（chrome/containerColor/navDisplayContent）—
 * 提取以降低 [SujianNavigationSuite] 行数。
 */
@Composable
@Suppress("LongParameterList") // 8 参数达 threshold，函数级 suppress（既有先例）
private fun rememberSujianNavScaffoldState(
    isEditorLocation: Boolean,
    topBarInfo: SujianTopBarInfo,
    snackbarHostState: SnackbarHostState,
    currentTopDestination: SujianDestination,
    topLevelBackStack: SujianTopLevelBackStack,
    navContext: SujianNavContext,
    deps: SujianAppDependencies,
    syncState: com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState,
): SujianNavScaffoldState {
    val navDisplayContent: @Composable () -> Unit = {
        SujianTopLevelSwitchMotion(currentTopDestination) {
            SujianNavDisplayContent(topLevelBackStack, navContext, deps, syncState)
        }
    }
    // Editor 位置的顶栏全部归 Editor composable，
    // Scaffold 不再给任何 Editor 画顶栏（showOuterTopBar=!isEditorLocation）。
    val showOuterTopBar = !isEditorLocation
    // Editor 位置 Scaffold container 透明，让 Editor composable 透出；
    // 非 Editor 位置用 colorScheme.background。
    val containerColor =
        if (isEditorLocation) {
            androidx.compose.ui.graphics.Color.Transparent
        } else {
            androidx.compose.material3.MaterialTheme.colorScheme.background
        }
    val scaffoldChrome =
        SujianNavScaffoldChrome(
            topBarInfo = topBarInfo,
            showOuterTopBar = showOuterTopBar,
            snackbarHostState = snackbarHostState,
        )
    return SujianNavScaffoldState(
        navDisplayContent = navDisplayContent,
        containerColor = containerColor,
        scaffoldChrome = scaffoldChrome,
    )
}

/** 外层 Scaffold 状态打包 — 提取以降低 [SujianNavigationSuite] 行数。 */
private data class SujianNavScaffoldState(
    val navDisplayContent: @Composable () -> Unit,
    val containerColor: androidx.compose.ui.graphics.Color,
    val scaffoldChrome: SujianNavScaffoldChrome,
)

/** 收集 ViewModel 抛出的 UI 事件，错误走 Snackbar 展示。 */
@Composable
private fun SujianUiEventEffect(
    appState: SujianAppState,
    snackbarHostState: SnackbarHostState,
) {
    LaunchedEffect(appState) {
        appState.uiEvents.collect { event ->
            when (event) {
                is WorkspaceUiEvent.Error -> snackbarHostState.showSnackbar(event.message)
            }
        }
    }
}

/**
 * 判断本次 top-level destination 变化是否属于”一级切换”。
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

/** 用 PerformanceMetricsState 写 screen/interaction 上下文。
 * screen 值综合 currentRoute 与 workspaceNavState.currentLocation：
 * - Settings 路由 → "Settings"
 * - Works 路由 + Editor 位置 → "Editor"
 * - 其余（Works/StarMap/Stats）→ 顶栏目的地名称
 * 一级 tab 无整页动画，interaction 只标记当前切换帧（putSingleFrameState），不再 delay+remove。 */
@Composable
private fun SujianJankInteractionClearEffect(
    currentRoute: SujianRoute,
    currentTopDestination: SujianDestination,
    workspaceNavState: ProjectNavigationState,
) {
    val view = androidx.compose.ui.platform.LocalView.current
    var previousTopDestination by remember { mutableStateOf<SujianDestination?>(null) }
    LaunchedEffect(currentRoute, workspaceNavState.currentLocation) {
        val holder = androidx.metrics.performance.PerformanceMetricsState.getHolderForHierarchy(view)
        val state = holder?.state
        val screenValue = resolveScreenState(currentRoute, currentTopDestination, workspaceNavState)
        state?.putState("screen", screenValue)
        val interaction =
            resolveTopLevelSwitchInteraction(previousTopDestination, currentTopDestination)
        previousTopDestination = currentTopDestination
        if (interaction == null) return@LaunchedEffect
        // 一级 tab 无整页动画，只标记当前切换帧
        state?.putSingleFrameState("interaction", interaction)
    }
}

/**
 * screen 值的唯一计算点。
 * EditorWindowHost 不再持有 screen，SettingsRoute 不再写 screen，全部由此函数决定。
 */
internal fun resolveScreenState(
    currentRoute: SujianRoute,
    currentTopDestination: SujianDestination,
    workspaceNavState: ProjectNavigationState,
): String =
    when {
        currentRoute is SujianRoute.Settings -> "Settings"
        currentRoute is SujianRoute.Search -> "Search"
        currentRoute is SujianRoute.Works &&
            workspaceNavState.currentLocation is WorkspaceLocation.Editor -> "Editor"
        else -> currentTopDestination.name
    }

/**
 * 进程状态摘要唯一写入点。
 *
 * 在”顶级页面切换 / 进入退出正文 / 同步状态变化”三种关键状态变化时更新
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
 * 搜索路由 → Core SearchContext。
 *
 * 路由字段在打开搜索页时固化，保证返回后 origin 状态不被后续选择变化改写。
 */
private fun SujianRoute.Search.toSearchContext(): SearchContext =
    SearchContext(
        originScreen = originScreen,
        currentProjectId = projectId,
        currentVolumeId = volumeId,
        currentChapterId = chapterId,
        currentStarmapId = starmapId,
        currentSettingsSection = settingsSection,
    )

/**
 * 同步状态摘要字符串。提取为 internal 便于单测正反验证。
 */
internal fun syncIndicatorSummary(syncState: SyncIndicatorState): String =
    when (syncState) {
        SyncIndicatorState.Syncing -> "syncing"
        SyncIndicatorState.Synced -> "synced"
        SyncIndicatorState.Failed -> "failed"
        SyncIndicatorState.Unconfigured -> "unconfigured"
    }
