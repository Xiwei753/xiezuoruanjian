package com.xiwei.sujian.app

import android.annotation.SuppressLint
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.di.SujianAppDependencies
import com.xiwei.sujian.app.navigation.SujianNavigationSuite
import com.xiwei.sujian.app.theme.SujianTheme
import com.xiwei.sujian.app.theme.ThemeStore
import com.xiwei.sujian.app.theme.rememberThemeController
import com.xiwei.sujian.core.platform.aosp.AospCapabilityProvider
import com.xiwei.sujian.core.platform.aosp.VendorAdapterSetup
import com.xiwei.sujian.core.platform.api.AndroidCapabilities
import com.xiwei.sujian.core.platform.api.PointerKind
import com.xiwei.sujian.core.platform.vendor.VendorAdapterRegistry
import com.xiwei.sujian.core.platform.window.AospFoldFeatureInfo
import com.xiwei.sujian.core.platform.window.ImmersiveSystemBarsEffect
import com.xiwei.sujian.core.platform.window.WindowFoldFeatureCollector
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.editor.window.EditorWindowHost
import com.xiwei.sujian.feature.project.domain.ProjectUseCase

val LocalAndroidCapabilities =
    androidx.compose.runtime.compositionLocalOf<AndroidCapabilities> {
        AndroidCapabilities()
    }

// #592 一：Compose UI 必须从同一个 Application 进程级容器取得依赖实例，
// 不能再次 DefaultAppServiceContainer(context) 创建第二份容器。
// 后台 Worker 也从同一容器取依赖，保证 SyncStatusRepository StateFlow
// 和 SyncCoordinator 全进程唯一。
@Composable
private fun rememberSujianAppDependencies(context: android.content.Context): SujianAppDependencies {
    val app = context.applicationContext as? com.xiwei.sujian.app.SujianApplication
    return remember {
        val testProvider = SujianAppDependencies.getTestProvider()
        testProvider?.invoke(context) ?: requireNotNull(app).dependencies
    }
}

// #592 一：EditorWindowHost 是窗口级宿主，每个窗口创建一份。
// #592 二：配置变化时只释放窗口宿主（View、FrameClock），Rust 会话由
// EditorSessionViewModel 持有并跨配置变化存活；Activity 永久结束时
// ViewModel.onCleared() 调用 releaseHost() 关闭全部会话。
@Composable
private fun rememberSujianWindowHost(
    context: android.content.Context,
    deps: SujianAppDependencies,
    sessionCoordinator: com.xiwei.sujian.feature.editor.session.EditorSessionCoordinator,
): EditorWindowHost {
    val windowCoordinator =
        remember(sessionCoordinator) {
            EditorWindowHost(
                context.applicationContext,
                sessionCoordinator,
                deps.appServiceBridge,
                com.xiwei.sujian.feature.editor.visual.ChoreographerAnimationTimeSource(),
                com.xiwei.sujian.feature.editor.visual.TransactionIdSource(),
            )
        }
    DisposableEffect(windowCoordinator) {
        onDispose {
            windowCoordinator.releaseWindow()
        }
    }
    return windowCoordinator
}

@Composable
private fun SujianAppCapabilityEffects(capabilityProvider: AospCapabilityProvider) {
    DisposableEffect(capabilityProvider) {
        capabilityProvider.registerInputDeviceListener()
        onDispose {
            capabilityProvider.unregisterInputDeviceListener()
        }
    }
}

@Composable
private fun SujianAppActivityLifecycleEvents(activityRef: androidx.activity.ComponentActivity?) {
    DisposableEffect(activityRef) {
        val act = activityRef ?: return@DisposableEffect onDispose { }
        val observer =
            androidx.lifecycle.LifecycleEventObserver { _, event ->
                when (event) {
                    androidx.lifecycle.Lifecycle.Event.ON_PAUSE ->
                        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.activityLifecycle("pause")
                    androidx.lifecycle.Lifecycle.Event.ON_DESTROY -> {
                        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.activityLifecycle("destroy")
                    }
                    else -> {}
                }
            }
        act.lifecycle.addObserver(observer)
        onDispose {
            act.lifecycle.removeObserver(observer)
        }
    }
}

@Composable
private fun SujianAppInitialization(
    deps: SujianAppDependencies,
    vm: SujianAppViewModel,
    context: android.content.Context,
    windowCoordinator: EditorWindowHost,
) {
    // #614 评论二：单条 LaunchedEffect 链保证 initialize 先于 repeatOnLifecycle refresh，
    // 避免两个独立 LaunchedEffect 无顺序保证、refresh 可能在 initialize 前执行。
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    LaunchedEffect(lifecycleOwner, deps, vm) {
        val projectUC = ProjectUseCase(deps.projectRepository, deps.recentEditsRepository)
        vm.initialize(deps.projectRepository, projectUC, deps.settingsRepository, context)
        lifecycleOwner.lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            deps.syncStatusRepository.refreshState()
            // #625 项6：列表 UI 唯一数据源是 projectSummaries（含字数），
            // projects 第二数据源已删 — 只刷新 projectSummaries。
            vm.refreshProjectSummaries()
            vm.refreshRecentEdits()
        }
    }
    // #625 项6：章节保存成功 → 及时刷新作品摘要（含字数），不再仅靠 RESUMED 生命周期。
    // 信号由 EditorViewModel 落盘成功后经 sessionCoordinator 向上暴露，app 层向下收集；
    // editor feature 层不依赖 app 层，不新增第二数据源。
    LaunchedEffect(windowCoordinator, vm) {
        windowCoordinator.chapterSavedSignal.collect {
            vm.refreshProjectSummaries()
        }
    }
    // #630 评论 #1：全量同步完成 → 及时刷新作品摘要（含字数/修改时间），
    // 不再仅靠 RESUMED 生命周期。信号由 SyncCoordinator.runFullSync 映射成 Completed 后发出；
    // 手动同步 / 设置触发 / AutoSyncWorker 走同一 deps.syncCoordinator，同一条失效链。
    LaunchedEffect(deps, vm) {
        deps.syncCoordinator.fullSyncCompleted.collect {
            vm.refreshProjectSummaries()
        }
    }
}

@Composable
private fun rememberFoldFeatureCollection(activity: androidx.activity.ComponentActivity?): List<AospFoldFeatureInfo> {
    var foldingFeatures by remember { mutableStateOf<List<AospFoldFeatureInfo>>(emptyList()) }
    if (activity != null) {
        val foldCollector = remember { WindowFoldFeatureCollector(activity) }
        DisposableEffect(foldCollector) {
            foldCollector.startCollecting { features ->
                foldingFeatures = features
            }
            onDispose {
                foldCollector.stopCollecting()
            }
        }
    }
    return foldingFeatures
}

// LocalConfiguration.smallestScreenWidthDp 无 Compose API 替代，需用 Configuration 检测设备类型。
@SuppressLint("ConfigurationScreenWidthHeight")
@Composable
private fun SujianAppAdaptiveWindowSync(
    capabilityProvider: AospCapabilityProvider,
    foldingFeatures: List<AospFoldFeatureInfo>,
    deps: SujianAppDependencies,
) {
    val configuration = LocalConfiguration.current
    LaunchedEffect(foldingFeatures, configuration.screenWidthDp, configuration.screenHeightDp) {
        capabilityProvider.updateFromFoldFeatures(foldingFeatures)
        capabilityProvider.updateFromConfiguration(configuration)
        val hasFoldFeature = foldingFeatures.isNotEmpty()
        val deviceClass =
            deps.themeRepository.detectDeviceClassFromFoldFeature(
                hasFoldFeature,
                configuration.smallestScreenWidthDp,
            )
        ThemeStore.setFoldDeviceClass(deviceClass)
    }
}

// LocalConfiguration.smallestScreenWidthDp 无 Compose API 替代，需用 Configuration 检测设备类型。
@SuppressLint("ConfigurationScreenWidthHeight")
@Composable
fun SujianApp(initialDestination: String? = null) {
    val context = LocalContext.current
    val vm: SujianAppViewModel = viewModel()
    val appState = remember { SujianAppState(vm) }
    val deps = rememberSujianAppDependencies(context)
    val activityRef = androidx.activity.compose.LocalActivity.current as? androidx.activity.ComponentActivity
    val sessionVm: com.xiwei.sujian.feature.editor.session.EditorSessionViewModel = viewModel()
    val sessionCoordinator =
        sessionVm.getOrCreateSessionCoordinator(
            deps.appServiceBridge,
        )
    val windowCoordinator = rememberSujianWindowHost(context, deps, sessionCoordinator)
    // #609 一：主题控制器在 CompositionLocalProvider 建立之前初始化，
    // 必须显式注入依赖，不得反向读取 CompositionLocal。
    // #618 三：同步状态不再参与主题刷新（旧代码的 Synced 分支与无条件 reload
    // 动作完全相同，是重复解析），控制器只依赖 settings/theme 两个仓库。
    val themeController =
        rememberThemeController(
            context = context,
            settingsRepository = deps.settingsRepository,
            themeRepository = deps.themeRepository,
        )
    remember { VendorAdapterRegistry().also { VendorAdapterSetup.ensureInitialized(it) } }

    val capabilityProvider = remember { AospCapabilityProvider(context.applicationContext) }
    val capabilities by capabilityProvider.capabilities.collectAsState()
    SujianAppCapabilityEffects(capabilityProvider)

    SujianAppActivityLifecycleEvents(activityRef)
    SujianAppInitialization(deps, vm, context, windowCoordinator)

    val foldingFeatures = rememberFoldFeatureCollection(activityRef)
    SujianAppAdaptiveWindowSync(capabilityProvider, foldingFeatures, deps)

    // #617 评论六：只收集沉浸式全屏这一位 — 由 SettingsRepository 构造时从
    // SharedPreferences 初始化、保存成功后同步；其它本地设置变化不再触碰应用根。
    val immersiveFullscreenEnabled by
        deps.settingsRepository.immersiveFullscreenEnabled.collectAsState()
    ImmersiveSystemBarsEffect(
        activity = activityRef,
        enabled = immersiveFullscreenEnabled,
    )

    val uiState by themeController.uiState.collectAsState()

    SujianTheme(uiState = uiState) {
        CompositionLocalProvider(
            LocalAndroidCapabilities provides capabilities,
            LocalEditorWindowHost provides windowCoordinator,
            LocalSujianAppDependencies provides deps,
        ) {
            SujianAppContent(
                capabilityProvider = capabilityProvider,
                appState = appState,
                initialDestination = initialDestination,
                foldingFeatures = foldingFeatures,
            )
        }
    }
}

/**
 * 应用内容根 — 负责追踪活动指针类型（鼠标/触控笔/触摸）并驱动导航套件。
 */
@Composable
private fun SujianAppContent(
    capabilityProvider: AospCapabilityProvider,
    appState: SujianAppState,
    initialDestination: String?,
    foldingFeatures: List<AospFoldFeatureInfo>,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier =
            modifier
                .fillMaxSize()
                .pointerInput(capabilityProvider) {
                    awaitPointerEventScope {
                        while (true) {
                            val event = awaitPointerEvent(PointerEventPass.Initial)
                            val changes = event.changes
                            if (changes.isNotEmpty()) {
                                val type = changes.first().type
                                val kind =
                                    when (type) {
                                        PointerType.Mouse -> PointerKind.Mouse
                                        PointerType.Stylus -> PointerKind.Stylus
                                        PointerType.Eraser -> PointerKind.Stylus
                                        else -> PointerKind.Touch
                                    }
                                capabilityProvider.updateActivePointerKind(kind)
                            }
                        }
                    }
                },
    ) {
        SujianNavigationSuite(
            appState = appState,
            initialDestination = initialDestination,
            foldingFeatures = foldingFeatures,
            modifier = Modifier.fillMaxSize(),
        )
    }
}
