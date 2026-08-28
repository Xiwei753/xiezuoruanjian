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
import com.xiwei.sujian.app.presentation.contract.PresentationContractBridge
import com.xiwei.sujian.app.presentation.layout.AndroidLayoutSpec
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.physicalSafeBounds
import com.xiwei.sujian.app.presentation.layout.rememberAndroidLayoutSpec
import com.xiwei.sujian.app.presentation.layout.rememberAndroidWorkbenchPlanState
import com.xiwei.sujian.app.presentation.layout.rememberAndroidWorkbenchViewportFrame
import com.xiwei.sujian.app.presentation.layout.rememberWorkbenchLayoutPlanner
import com.xiwei.sujian.app.presentation.screen.AndroidChromePolicy
import com.xiwei.sujian.app.presentation.screen.AndroidWorkspaceActionSpec
import com.xiwei.sujian.app.presentation.screen.SujianChromeSpec
import com.xiwei.sujian.app.presentation.screen.rememberProjectActions
import com.xiwei.sujian.app.state.ActiveDocumentGate
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.platform.window.AospFoldFeatureInfo
import com.xiwei.sujian.feature.project.ui.ProjectNavigationState
import com.xiwei.sujian.feature.project.ui.ProjectWorkspaceScreen
import com.xiwei.sujian.feature.project.ui.WorkspaceLocation
import com.xiwei.sujian.feature.project.ui.WorkspaceNavigator
import com.xiwei.sujian.feature.project.ui.buildInitialHistory
import com.xiwei.sujian.feature.project.ui.deriveRestoreDestination
import com.xiwei.sujian.feature.settings.ui.SettingsRoute
import com.xiwei.sujian.feature.starmap.ui.StarMapPlaceholderScreen
import com.xiwei.sujian.feature.stats.ui.StatsScreen
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
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

private fun rememberInitialNavStack(initialDestination: String?): Pair<SujianDestination, List<SujianRoute>> =
    when (initialDestination) {
        "settings" -> SujianDestination.Works to listOf(SujianRoute.Works, SujianRoute.Settings)
        "starmap" -> SujianDestination.StarMap to listOf(SujianRoute.StarMap)
        "stats" -> SujianDestination.Stats to listOf(SujianRoute.Stats)
        else -> SujianDestination.Works to listOf(SujianRoute.Works)
    }

/**
 * 导航内容上下文 — 打包传递，避免函数参数超出门禁阈值。
 *
 * #628 评论 5301021120 02:59:39Z 版：[workbenchPlan] 与 pane 收起状态在这里统一解析/持有 ——
 * 外层顶栏归属与工作区必须消费同一份 Rust 最终 mode，不允许 layoutSpec=Workbench 但
 * plan 已 SinglePane、外层还按 Workbench 隐藏顶栏的分裂状态。
 */
private data class SujianNavContext(
    val appState: SujianAppState,
    val workspaceNavState: ProjectNavigationState,
    val projectListActions: AndroidWorkspaceActionSpec,
    val projectWorkspaceActions: AndroidWorkspaceActionSpec,
    val layoutSpec: AndroidLayoutSpec,
    val workbenchPlan: AndroidWorkbenchLayoutPlan?,
    val workbenchSafeBounds: com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect,
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
    // #614 评论三：每个 top-level 栈绑定自己的 decorated entries（独立 SaveableStateHolder +
    // ViewModelStore decorator）。三个 rememberDecoratedNavEntries 始终在组合中，
    // inactive tab 状态不丢失；NavDisplay 直接消费 entries。
    // #618 五：entryProvider 按 appState / workspaceNavState / 两份静态动作 spec 稳定保存，
    // 底栏切换不再顺手重建三套 NavEntry provider。
    // #625 第二段：entryProvider 还消费 layoutSpec / chrome — 大屏 Editor 位置画 WideWritingWorkspace。
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
            com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.navBack(handled)
            handled
        },
        transitionSpec = navForwardTransition,
        popTransitionSpec = navPopTransition,
        predictivePopTransitionSpec = navPredictivePopTransition,
    )
}

/**
 * #618 五：工作区 NavEntry provider — 按 [appState] / [workspaceNavState] /
 * 两份静态动作 spec 稳定保存，避免每次重组都重建三个 back stack 的 provider。
 * 动作 spec 来自容器创建时解析的 [com.xiwei.sujian.app.presentation.screen.PresentationPolicyCatalog]，
 * 是稳定引用，因此 provider 只在真正需要时重建。
 */
@Composable
private fun rememberSujianEntryProvider(
    context: SujianNavContext,
    topLevelBackStack: SujianTopLevelBackStack,
    deps: SujianAppDependencies,
    syncState: com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState,
): (NavKey) -> NavEntry<NavKey> =
    remember(context) {
        { key: NavKey ->
            when (key) {
                is SujianRoute ->
                    when (key) {
                        is SujianRoute.Works ->
                            rememberWorksEntry(key, context, topLevelBackStack, deps)
                        is SujianRoute.StarMap ->
                            NavEntry(key, metadata = noPageTransitionMetadata) { route ->
                                StarMapPlaceholderScreen(
                                    modifier = Modifier.testTag(SujianSemanticIds.StarMapScreen),
                                )
                            }
                        is SujianRoute.Stats ->
                            NavEntry(key, metadata = noPageTransitionMetadata) { route ->
                                StatsScreen()
                            }
                        // #630 评论5324547885项3: Settings 使用 noPageTransitionMetadata，
                        // 禁止旧 Works + 新 Settings 双页 crossfade。
                        // 过渡由 SettingsRoute 自身 graphicsLayer 绘制层动画提供。
                        is SujianRoute.Settings ->
                            NavEntry(key, metadata = noPageTransitionMetadata) { route ->
                                SettingsRoute()
                            }
                    }
                else -> NavEntry(key) {}
            }
        }
    }

/**
 * #625 第二段：Works 路由的 NavEntry — 提取以降低 [rememberSujianEntryProvider] 认知复杂度。
 * ProjectWorkspaceScreen 消费 layoutSpec 与 chrome — 大屏 Editor 位置画 WideWritingWorkspace。
 *
 * #641 评论 问题5：把 workbenchPlan/safeBounds/pane 收起/chrome 打包成 WorkbenchPresentationState
 * 传给 ProjectWorkspaceScreen，避免 WideLayoutContent Editor 分支再用 null/空函数让 plan 永远走 single-pane。
 */
private fun rememberWorksEntry(
    key: SujianRoute.Works,
    context: SujianNavContext,
    topLevelBackStack: SujianTopLevelBackStack,
    deps: SujianAppDependencies,
): NavEntry<NavKey> =
    NavEntry(key, metadata = noPageTransitionMetadata) { route ->
        val workbenchPresentation =
            com.xiwei.sujian.feature.project.ui.WorkbenchPresentationState(
                plan = context.workbenchPlan,
                safeBounds = context.workbenchSafeBounds,
                chapterTreeCollapsed = context.chapterTreeCollapsed,
                toolPaneCollapsed = context.toolPaneCollapsed,
                onToggleChapterTree = context.onToggleChapterTree,
                onToggleToolPane = context.onToggleToolPane,
                chrome = context.chrome,
            )
        ProjectWorkspaceScreen(
            appState = context.appState,
            workspaceNavState = context.workspaceNavState,
            projectListActions = context.projectListActions,
            projectWorkspaceActions = context.projectWorkspaceActions,
            layoutSpec = context.layoutSpec,
            workbenchPresentation = workbenchPresentation,
        )
    }

/**
 * #617 评论二：一级（底栏/侧栏）切换动效 — 只动画“新页面绘制层”。
 *
 * 不恢复 #614 删除的双页面整页 crossfade：切换期间旧页立即退出，新页只在
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

/** 工作区 navigator — 在导航套件层创建的唯一实例（#597：返回历史始终同一份）。
 *
 * #625 第二段：改用纯业务 [WorkspaceNavigator]，不再依赖 Material3 Adaptive
 * 的 rememberListDetailPaneScaffoldNavigator。"当前在哪个业务位置"与"屏幕上同时画
 * 哪些区域"彻底分开 — 后者由 [com.xiwei.sujian.app.presentation.layout.AndroidLayoutSpec]
 * 的 `workspaceLayoutMode` 决定，[ProjectWorkspaceScreen] 消费。 */
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

/** 工作区返回处理 — 系统返回/预测返回（正文→章节树→作品列表）。
 * NavDisplay 只在全局栈可弹出（如设置页）时处理返回；Works 根时由这里接管。
 *
 * #624 评论12 第1项：预测返回不再用 ThreePaneScaffoldPredictiveBackHandler
 * （它会自己提交 navigator.navigateBack，保存来不及做）— 改用
 * androidx.activity.compose.PredictiveBackHandler。
 *
 * #624 评论13 第1项：同一 back dispatcher 里最后组合的 enabled handler 优先 —
 * 旧的第二个 BackHandler 会抢先消费普通返回，predictive callback 拿不到手势
 * progress。现在只保留这一个 PredictiveBackHandler：
 * - 无条件参与组合（官方要求），只用 `enabled` 控制：Works 路由且可返回时生效；
 * - 手势过程把 BackEventCompat.progress 喂给 navigator.seekBack；
 * - 手势真正完成后先 ActiveDocumentGate.flushActiveDocument()（保存活动正文）；
 *   保存成功再真正导航离开（back()）；保存失败把 seek 复位到 0f；
 * - 手势取消（CancellationException）同样把 seek 复位到 0f 后重新抛出 —
 *   navigator 不得停在半截 seek 状态；
 * - AndroidX 自己的 handleOnBackPressed() 会为普通返回创建 non-predictive
 *   back instance（progressFlow 单事件后正常完成），不再叠第二个 BackHandler。 */
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
            onSeekBack = workspaceNavState::seekBack,
            onFlushActiveDocument = ActiveDocumentGate::flushActiveDocument,
            onBack = workspaceNavState::back,
        )
    }
}

/**
 * #624 评论13 第1项：工作区预测/系统返回的单一执行体（纯逻辑，便于单测）。
 *
 * - 手势过程：把每个 [androidx.activity.BackEventCompat.progress] 喂给 [onSeekBack]；
 * - 手势正常完成：先 [onFlushActiveDocument] 保存活动正文 — 成功才 [onBack] 导航
 *   离开；失败把 seek 复位到 0f（保持 Editor 目的地，正文不丢）；
 * - 手势取消（[kotlinx.coroutines.CancellationException]）：把 seek 复位到 0f 后
 *   重新抛出 — navigator 不得停在半截 seek 状态。
 */
internal suspend fun runPredictiveWorkspaceBack(
    progressFlow: kotlinx.coroutines.flow.Flow<androidx.activity.BackEventCompat>,
    onSeekBack: (Float) -> Unit,
    onFlushActiveDocument: suspend () -> Boolean,
    onBack: () -> Unit,
) {
    try {
        progressFlow.collect { event ->
            onSeekBack(event.progress)
        }
        // 手势真正完成：先保存活动正文，保存成功才导航离开。
        if (onFlushActiveDocument()) {
            onBack()
        } else {
            // 保存失败 — 复位 seek，保持 Editor 目的地（正文不丢）。
            onSeekBack(0f)
        }
    } catch (e: kotlinx.coroutines.CancellationException) {
        // #624 评论14 第1项：catch 块运行在已取消的 coroutine 里（Activity Compose
        // 的 PredictiveBackHandler 在手势取消时直接 job.cancel()）。直接调用 suspend
        // onSeekBack(0f) 会再次响应取消，复位可能没完成。用 NonCancellable 包裹保证
        // 复位完整执行后再向上重抛（navigator 不得停在半截过渡态）。
        withContext(NonCancellable) { onSeekBack(0f) }
        throw e
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
    // #614: nav.top_level_switch 改由目的地变化的 LaunchedEffect 异步记录，
    // 不在 onTopLevelSelected 交互回调里同步做诊断格式化/落队列。
    // 首次组合（previous 为 null）与原地重复选择（前后相同）都不记录，
    // 与 resolveTopLevelSwitchInteraction 的保真度语义一致。
    var previousTopDestination by remember { mutableStateOf<SujianDestination?>(null) }
    LaunchedEffect(currentTopDestination) {
        val previous = previousTopDestination
        previousTopDestination = currentTopDestination
        if (previous == null || previous == currentTopDestination) return@LaunchedEffect
        withContext(Dispatchers.IO) {
            com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.navTopLevelSwitch(
                previous.name,
                currentTopDestination.name,
            )
        }
    }
}

/**
 * #618 一：作品工作区的两份静态动作 spec 取自容器创建时解析的 PresentationPolicyCatalog。
 * 章节树固定按 PROJECT_WORKSPACE 取动作契约（卷章操作不依赖父层组合帧观察到的
 * navigator 位置）；作品列表固定按 PROJECT_LIST 取契约（新建作品主操作同理）。
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
    val resolver = PresentationContractBridge.layoutContractResolver(deps.appServiceBridge)
    val workbenchResolver = PresentationContractBridge.workbenchLayoutResolver(deps.appServiceBridge)
    val layoutSpec = rememberAndroidLayoutSpec(foldingFeatures, resolver)
    // #640 评论 5443789509：把稳定系统 UI（systemBars + displayCutout）的 safe viewport 放到
    // planner 输入前，Rust 看到的 (0,0) 是稳定安全工作区左上角；plan 返回后再 offsetBy 平移回物理坐标。
    val workbenchViewportFrame = rememberAndroidWorkbenchViewportFrame(layoutSpec.viewport)
    // #640 评论 5444584755：物理安全矩形（plan 有无都共用）。workbenchPlan == null（resolver 失败）时
    // wide SinglePane / EditorOnly 用它作 fallback，不再回落整个 constraints（物理窗口 (0,0)），
    // 避免 TopAppBar 从 (0,0) 开始、正文只躲 IME 不躲 status bar / display cutout / navigation bar。
    val workbenchSafeBounds = workbenchViewportFrame.physicalSafeBounds
    val workbenchPlanner = rememberWorkbenchLayoutPlanner(workbenchViewportFrame, workbenchResolver)

    // #628 评论 5301021120 02:59:39Z 版：pane 收起状态 + Rust workbench plan 统一在
    // 导航套件层持有/解析（外层顶栏归属必须消费同一份 Rust 最终 mode）。
    val workbenchPlanState = rememberAndroidWorkbenchPlanState(workbenchPlanner)
    val (initialTopLevel, initialStackRoutes) = rememberInitialNavStack(initialDestination)
    val topLevelBackStack = rememberSujianTopLevelBackStack(initialTopLevel, initialStackRoutes)
    val backStack = topLevelBackStack.backStack
    val currentRoute = backStack.lastOrNull() as? SujianRoute ?: SujianRoute.Works
    val currentTopDestination = currentRoute.toTopDestination()
    val snackbarHostState = remember { SnackbarHostState() }

    // #614：收集 ViewModel 抛出的 UI 事件，错误走 Snackbar 展示。
    SujianUiEventEffect(appState, snackbarHostState)

    val syncState by deps.syncStatusRepository.state.collectAsState()
    val coroutineScope = rememberCoroutineScope()

    val workspaceNavState = rememberSujianWorkspaceNavState(appState)

    // #618 一：页面契约在应用容器创建时已一次性解析进 PresentationPolicyCatalog，
    // 热路径只查内存 Map，不再同步跨 UniFFI 取契约。章节树固定按 PROJECT_WORKSPACE
    // 取动作契约（卷章操作不依赖父层组合帧观察到的 navigator 位置）；作品列表
    // 固定按 PROJECT_LIST 取契约（新建作品主操作同理）。
    val screenRole = AndroidChromePolicy.screenRoleFor(currentRoute, workspaceNavState.currentLocation)
    val screenPolicy = deps.presentationPolicyCatalog[screenRole]
    val (projectListActions, projectWorkspaceActions) = rememberProjectActions(deps.presentationPolicyCatalog)
    val chrome =
        AndroidChromePolicy.resolve(
            screenRole = screenRole,
            screenPolicy = screenPolicy,
            workspaceLocation = workspaceNavState.currentLocation,
            canWorkspaceNavigateBack = workspaceNavState.canNavigateBack,
        )

    SujianWorkspaceBackEffects(currentRoute, workspaceNavState)
    SujianRouteEffects(currentRoute, currentTopDestination)
    val env = SujianTopBarEnv(syncState, coroutineScope, deps, topLevelBackStack)
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
            projectListActions = projectListActions,
            projectWorkspaceActions = projectWorkspaceActions,
            layoutSpec = layoutSpec,
            workbenchSafeBounds = workbenchSafeBounds,
            workbenchPlanState = workbenchPlanState,
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
            layoutSpec = layoutSpec,
            chrome = chrome,
            scaffoldChrome = scaffoldState.scaffoldChrome,
            selection = SujianTopLevelSelection(currentTopDestination, onTopLevelSelected),
            navDisplayContent = scaffoldState.navDisplayContent,
            containerColor = scaffoldState.containerColor,
        )
    }
}

/**
 * #641 评论 问题5：构造 [SujianNavContext] — 提取以降低 [SujianNavigationSuite] 行数。
 */
@Composable
@Suppress("LongParameterList") // #641 评论 问题5：8 参数达 threshold，函数级 suppress（既有先例）
private fun rememberSujianNavContext(
    appState: SujianAppState,
    workspaceNavState: ProjectNavigationState,
    projectListActions: AndroidWorkspaceActionSpec,
    projectWorkspaceActions: AndroidWorkspaceActionSpec,
    layoutSpec: AndroidLayoutSpec,
    workbenchSafeBounds: com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect,
    workbenchPlanState: com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchPlanState,
    chrome: SujianChromeSpec,
): SujianNavContext =
    SujianNavContext(
        appState = appState,
        workspaceNavState = workspaceNavState,
        projectListActions = projectListActions,
        projectWorkspaceActions = projectWorkspaceActions,
        layoutSpec = layoutSpec,
        workbenchPlan = workbenchPlanState.workbenchPlan,
        workbenchSafeBounds = workbenchSafeBounds,
        chapterTreeCollapsed = workbenchPlanState.chapterTreeCollapsed,
        toolPaneCollapsed = workbenchPlanState.toolPaneCollapsed,
        onToggleChapterTree = workbenchPlanState.onToggleChapterTree,
        onToggleToolPane = workbenchPlanState.onToggleToolPane,
        chrome = chrome,
    )

/**
 * #641 评论 问题5：构造外层 Scaffold 状态（chrome/containerColor/navDisplayContent）—
 * 提取以降低 [SujianNavigationSuite] 行数。
 */
@Composable
@Suppress("LongParameterList") // #641 评论 问题5：8 参数达 threshold，函数级 suppress（既有先例）
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
    // #628 评论 5301021120 02:59:39Z 版：外层顶栏归属必须消费同一份 Rust 最终模式。
    // #641：Editor 位置的顶栏全部归 Editor composable，
    // Scaffold 不再给任何 Editor 画顶栏（showOuterTopBar=!isEditorLocation）。
    val showOuterTopBar = !isEditorLocation
    // #641：Editor 位置 Scaffold container 透明，让 Editor composable 透出；
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

/** #614：收集 ViewModel 抛出的 UI 事件，错误走 Snackbar 展示。 */
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

/** Issue #612 四 / #614 / #631 comment 5364514035 item 5：用 PerformanceMetricsState 写 screen/interaction 上下文。
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
 * Issue #631 comment 5364514035 item 5：screen 值的唯一计算点。
 * EditorWindowHost 不再持有 screen，SettingsRoute 不再写 screen，全部由此函数决定。
 */
internal fun resolveScreenState(
    currentRoute: SujianRoute,
    currentTopDestination: SujianDestination,
    workspaceNavState: ProjectNavigationState,
): String =
    when {
        currentRoute is SujianRoute.Settings -> "Settings"
        currentRoute is SujianRoute.Works &&
            workspaceNavState.currentLocation is WorkspaceLocation.Editor -> "Editor"
        else -> currentTopDestination.name
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
