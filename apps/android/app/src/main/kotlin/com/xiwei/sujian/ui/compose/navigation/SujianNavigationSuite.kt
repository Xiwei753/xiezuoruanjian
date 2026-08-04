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
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.layout.safeDrawing
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.scene.Scene
import androidx.navigation3.ui.NavDisplay
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianTopAppBar
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.platform.api.WindowSizeClass
import com.xiwei.sujian.ui.compose.LocalAndroidCapabilities
import com.xiwei.sujian.ui.compose.SujianAppState
import com.xiwei.sujian.ui.compose.settings.SettingsRoute
import com.xiwei.sujian.ui.compose.settings.settingsCategories
import com.xiwei.sujian.ui.compose.starmap.StarMapScreen
import com.xiwei.sujian.ui.compose.stats.StatsScreen
import com.xiwei.sujian.ui.compose.workspace.ProjectWorkspaceScreen

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

/**
 * 写作工作区内部窗格的顶栏返回入口。
 *
 * 工作区手机窗格（作品列表/章节树/正文）由工作区内部的 Material3 Adaptive
 * navigator 管理，系统返回与顶栏返回必须调用同一套窗格转换；工作区把转换
 * 入口上抛给唯一根壳的 TopAppBar，避免根壳直接改选择状态造成两套返回路径。
 */
@Stable
class WorkspaceBackState {
    var onBack by mutableStateOf<(() -> Unit)?>(null)
        private set

    fun update(onBack: (() -> Unit)?) {
        this.onBack = onBack
    }
}

/**
 * 星图编辑态的一级 TopAppBar 内容。
 *
 * 星图编辑是星图目的地内部的窗格状态（不进入全局 back stack），
 * 其标题、返回和操作按钮通过该状态上抛给唯一根壳的 TopAppBar。
 */
@Stable
class StarMapTopBarState {
    var title by mutableStateOf("")
        private set
    var onBack by mutableStateOf<(() -> Unit)?>(null)
        private set
    var actions by mutableStateOf<(@Composable () -> Unit)?>(null)
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
 * 唯一根 App 壳 + 全局 Navigation3。
 *
 * - 根 [Scaffold] 统一处理 edge-to-edge 的状态栏/导航栏 Insets（[WindowInsets.safeDrawing]）、
 *   IME Insets（[Modifier.imePadding]）、一级 TopAppBar、NavigationBar/NavigationRail 和全局 Snackbar。
 * - 全局 back stack 只保留 Works/StarMap/Stats/Settings 四个一级 destination；Works 常驻栈底，
 *   其余一级 destination 在 Works 之上 push/pop，因此普通返回与可预见返回由 NavDisplay 的
 *   popTransitionSpec / predictivePopTransitionSpec 统一驱动（手势进度 seek 真实跟手）；
 *   作品/卷/章节（[SujianAppState]）和设置分类（[SettingsSection]）都是目的地内部状态。
 * - 前进、普通返回和可预见返回全部由 [NavDisplay] 的 transitionSpec / popTransitionSpec /
 *   predictivePopTransitionSpec 统一驱动，destination 内容外不再包裹 AnimatedContent。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SujianNavigationSuite(
    appState: SujianAppState,
    initialDestination: String? = null,
    modifier: Modifier = Modifier,
) {
    // deeplink 直接进入非 Works 一级入口时也保持 Works 栈底常驻：
    // 初始栈即 [Works, X]，保证返回/预测返回始终由 NavDisplay 统一驱动。
    val initialStack: List<SujianRoute> = when (initialDestination) {
        "settings" -> listOf(SujianRoute.Works, SujianRoute.Settings)
        "starmap" -> listOf(SujianRoute.Works, SujianRoute.StarMap)
        "stats" -> listOf(SujianRoute.Works, SujianRoute.Stats)
        else -> listOf(SujianRoute.Works)
    }
    val backStack = rememberNavBackStack(*initialStack.toTypedArray())
    val currentRoute = backStack.lastOrNull() as? SujianRoute ?: SujianRoute.Works
    val currentTopDestination = currentRoute.toTopDestination()
    val capabilities = LocalAndroidCapabilities.current
    val isWideScreen = capabilities.windowSizeClass != WindowSizeClass.Compact
    val snackbarHostState = remember { SnackbarHostState() }

    // 设置壳的列表-详情状态（提升到壳层，供一级 TopAppBar 显示分类标题与返回）。
    var settingsDetailSection by remember { mutableStateOf<SettingsSection?>(null) }
    // 星图编辑态的一级 TopAppBar 内容。
    val starMapTopBarState = remember { StarMapTopBarState() }
    // 写作工作区内部窗格的顶栏返回入口（手机窗格转换由工作区内部 navigator 驱动）。
    val workspaceBackState = remember { WorkspaceBackState() }

    // 离开目的地时清空其内部状态，避免下次进入显示陈旧标题/操作；
    // 平板/大屏进入设置时默认选中 Appearance，左右窗格立即完整显示。
    LaunchedEffect(currentTopDestination, isWideScreen) {
        com.xiwei.sujian.diagnostics.DiagnosticsEvents.navigation(currentTopDestination.name)
        if (currentTopDestination != SujianDestination.StarMap) {
            starMapTopBarState.clear()
        }
        if (currentTopDestination != SujianDestination.Settings) {
            settingsDetailSection = null
        }
        if (currentTopDestination == SujianDestination.Settings && isWideScreen && settingsDetailSection == null) {
            settingsDetailSection = SettingsSection.Appearance
        }
    }

    // 顶栏返回与系统返回共用同一套内部窗格转换：工作区把内部 navigator 的
    // 返回入口上抛到壳层，根壳只负责在 Works 目的地展示。
    val workspaceBack: (() -> Unit)? = if (currentTopDestination == SujianDestination.Works) {
        workspaceBackState.onBack
    } else null

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
        SujianDestination.Settings -> {
            if (!isWideScreen) {
                settingsDetailSection
                    ?.let { section -> settingsCategories.firstOrNull { it.section == section }?.titleResId }
                    ?.let { stringResource(id = it) }
                    ?: stringResource(id = R.string.action_settings)
            } else {
                stringResource(id = R.string.action_settings)
            }
        }
    }

    val showWorkspaceBack = currentTopDestination == SujianDestination.Works &&
        !isWideScreen && appState.currentProjectId != null
    val showSettingsBack = currentTopDestination == SujianDestination.Settings &&
        !isWideScreen && settingsDetailSection != null
    val topBarNavigationIcon = when {
        showWorkspaceBack -> SujianIcons.ArrowBack
        showSettingsBack -> SujianIcons.ArrowBack
        currentTopDestination == SujianDestination.StarMap && starMapTopBarState.onBack != null -> SujianIcons.ArrowBack
        else -> null
    }
    val topBarOnNavigationClick = when {
        showWorkspaceBack -> workspaceBack
        showSettingsBack -> { { settingsDetailSection = null } }
        currentTopDestination == SujianDestination.StarMap -> starMapTopBarState.onBack
        else -> null
    }
    // 返回按钮无障碍说明按当前场景动态选择：写作区返回章节列表/返回作品列表、
    // 设置详情返回设置、星图编辑返回星图列表；不得统一写死为星图返回。
    val navigationIconContentDescription: String? = when {
        showWorkspaceBack -> {
            if (appState.currentChapterId != null) {
                stringResource(id = R.string.back_to_chapter_list)
            } else {
                stringResource(id = R.string.back_to_project_list)
            }
        }
        showSettingsBack -> stringResource(id = R.string.back_to_settings)
        currentTopDestination == SujianDestination.StarMap && starMapTopBarState.onBack != null ->
            stringResource(id = R.string.back_to_starmap_list)
        else -> null
    }
    val topBarActions: @Composable () -> Unit = {
        if (currentTopDestination == SujianDestination.StarMap) {
            starMapTopBarState.actions?.invoke()
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
                                workspaceBackState = workspaceBackState,
                            )
                            is SujianRoute.StarMap -> StarMapScreen(
                                topBarState = starMapTopBarState,
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

    Box(modifier = modifier.fillMaxSize()) {
    Scaffold(
        modifier = Modifier.fillMaxSize(),
        topBar = {
            SujianTopAppBar(
                title = topBarTitle,
                navigationIcon = topBarNavigationIcon,
                navigationIconContentDescription = navigationIconContentDescription,
                onNavigationClick = topBarOnNavigationClick,
                actions = topBarActions,
            )
        },
        bottomBar = {
            if (!isWideScreen) {
                NavigationBar {
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
            }
        },
        snackbarHost = {
            SnackbarHost(hostState = snackbarHostState) { data ->
                com.xiwei.sujian.designsystem.component.SujianSnackbar(data = data)
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
    ) { innerPadding ->
        if (isWideScreen) {
            // 平板/大屏：NavigationRail 位于内容区左侧（不占用 bottomBar 槽位），
            // 正文内容占满剩余区域。
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
        } else {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(innerPadding)
                    .imePadding(),
            ) {
                navDisplayContent()
            }
        }
    }

        // 全局编辑器宿主层：唯一 Editor Host 覆盖层，按当前一级目的地控制可见性，
        // 避免工作区编辑器在设置/统计等页面上方残留渲染。
        val coordinator = com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator.current
        val editorOverlayVisible = currentTopDestination == SujianDestination.Works ||
            currentTopDestination == SujianDestination.StarMap
        if (coordinator != null) {
            com.xiwei.sujian.editor.v2.compose.AnimatedTextEditorSlot(
                coordinator = coordinator,
                modifier = Modifier.fillMaxSize(),
                visible = editorOverlayVisible,
            )
        }
    }
}

private fun navItemModifier(destination: SujianDestination): Modifier {
    val semanticTag = when (destination) {
        SujianDestination.Works -> SujianSemanticIds.NavigationWorks
        SujianDestination.Settings -> SujianSemanticIds.NavigationSettings
        else -> null
    }
    return if (semanticTag != null) Modifier.testTag(semanticTag) else Modifier
}

/**
 * 切换到一级 destination：Works 常驻栈底，其余 destination 在 Works 之上 push/pop。
 *
 * - 栈底恒为 Works（deeplink 直入的非 Works 栈在首次切换时重建为标准形态），
 *   因此 NavDisplay 的 previousEntries 恒非空，普通返回与可预见返回（手势进度）
 *   始终由 NavDisplay 统一驱动，不会出现返回被吞或预测返回不可达。
 * - 切换只改栈形态，不重建写作工作区选择状态（[SujianAppState] 在 ViewModel 层）。
 */
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
    // predictivePopTransitionSpec 的 Int 参数是手势起始边（NavigationEvent.SwipeEdge），
    // 不是距离比例：当前场景沿手势方向退场，前一场景从对侧入场，
    // 时长固定（NavDisplay 按手势进度 seek，提交/取消后再按剩余时长收尾）。
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

// ========== 工作区/星图内部窗格的预测返回进度映射 ==========

/**
 * 把系统返回手势进度（BackEvent.progress，0..1）映射为单窗格状态下内部窗格
 * 过渡的进度，供工作区与星图的列表—详情 navigator seek。
 *
 * 与 Material3 Adaptive 库 `ThreePaneScaffoldPredictiveBackHandler` 内部
 * `backProgressToStateProgress` 对单窗格（expandedCount == 1）的计算完全一致：
 * 同一缓动曲线，峰值比例 SinglePaneProgressRatio = 0.1（手势全程为“窥视”，
 * 提交后由 navigateBack 播放剩余过渡）。不依赖库私有实现，常量与公式照抄，
 * 保证手势跟手语义与库一致。
 */
internal fun predictiveBackStateFraction(progress: Float): Float =
    PredictiveBackEasing.transform(progress) * SinglePaneProgressRatio

private val PredictiveBackEasing: androidx.compose.animation.core.CubicBezierEasing =
    androidx.compose.animation.core.CubicBezierEasing(0.1f, 0.1f, 0f, 1f)

internal const val SinglePaneProgressRatio: Float = 0.1f
