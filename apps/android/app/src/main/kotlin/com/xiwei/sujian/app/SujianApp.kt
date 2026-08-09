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
import androidx.compose.ui.platform.LocalDensity
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.di.SujianAppDependencies
import com.xiwei.sujian.app.layout.model.Orientation
import com.xiwei.sujian.app.layout.model.WindowMetrics
import com.xiwei.sujian.app.navigation.SujianNavigationSuite
import com.xiwei.sujian.app.theme.SujianTheme
import com.xiwei.sujian.app.theme.ThemeStore
import com.xiwei.sujian.app.theme.rememberThemeController
import com.xiwei.sujian.feature.project.domain.ProjectUseCase
import com.xiwei.sujian.core.platform.aosp.AospCapabilityProvider
import com.xiwei.sujian.core.platform.aosp.VendorAdapterSetup
import com.xiwei.sujian.core.platform.api.AndroidCapabilities
import com.xiwei.sujian.core.platform.api.PointerKind
import com.xiwei.sujian.core.platform.vendor.VendorAdapterRegistry
import com.xiwei.sujian.core.platform.window.WindowFoldFeatureCollector
import com.xiwei.sujian.feature.editor.ui.LocalEditorWindowHost
import com.xiwei.sujian.feature.editor.window.EditorWindowHost

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
) {
    LaunchedEffect(Unit) {
        val projectUC = ProjectUseCase(deps.projectRepository, deps.recentEditsRepository)
        vm.initialize(deps.projectRepository, projectUC, deps.settingsRepository, context)
    }
    val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    LaunchedEffect(lifecycleOwner) {
        lifecycleOwner.lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            deps.syncStatusRepository.refreshState(vm.currentProjectId)
            vm.refreshProjects()
            vm.refreshRecentEdits()
        }
    }
}

@Composable
private fun rememberFoldFeatureCollection(
    activity: androidx.activity.ComponentActivity?,
): List<androidx.window.layout.FoldingFeature> {
    var foldingFeatures by remember { mutableStateOf<List<androidx.window.layout.FoldingFeature>>(emptyList()) }
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
    foldingFeatures: List<androidx.window.layout.FoldingFeature>,
    vm: SujianAppViewModel,
    deps: SujianAppDependencies,
) {
    val configuration = LocalConfiguration.current
    val density = LocalDensity.current.density
    LaunchedEffect(foldingFeatures, configuration.screenWidthDp, configuration.screenHeightDp) {
        capabilityProvider.updateFromFoldFeatures(foldingFeatures)
        capabilityProvider.updateFromConfiguration(configuration)
        vm.updateFoldFeaturesFromAdaptive(foldingFeatures, density)
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
private fun SujianAppLayoutResolution(
    capabilities: AndroidCapabilities,
    vm: SujianAppViewModel,
) {
    val configuration = LocalConfiguration.current
    LaunchedEffect(capabilities) {
        val metrics =
            WindowMetrics(
                widthDp = configuration.screenWidthDp.toFloat(),
                heightDp = configuration.screenHeightDp.toFloat(),
                foldFeature = vm.foldFeatureInfo,
                orientation =
                    if (configuration.screenWidthDp > configuration.screenHeightDp) {
                        Orientation.Landscape
                    } else {
                        Orientation.Portrait
                    },
                pointer =
                    when (capabilities.activePointerKind) {
                        PointerKind.Mouse -> com.xiwei.sujian.app.layout.model.PointerKind.Mouse
                        PointerKind.Trackpad -> com.xiwei.sujian.app.layout.model.PointerKind.Trackpad
                        PointerKind.Stylus -> com.xiwei.sujian.app.layout.model.PointerKind.Stylus
                        else -> com.xiwei.sujian.app.layout.model.PointerKind.Touch
                    },
            )
        vm.resolveLayout(metrics)
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
    val themeController = rememberThemeController(context, deps.settingsRepository, deps.themeRepository)
    remember { VendorAdapterRegistry().also { VendorAdapterSetup.ensureInitialized(it) } }

    val capabilityProvider = remember { AospCapabilityProvider(context.applicationContext) }
    val capabilities by capabilityProvider.capabilities.collectAsState()
    SujianAppCapabilityEffects(capabilityProvider)

    SujianAppActivityLifecycleEvents(activityRef)
    SujianAppInitialization(deps, vm, context)

    val foldingFeatures = rememberFoldFeatureCollection(activityRef)
    SujianAppAdaptiveWindowSync(capabilityProvider, foldingFeatures, vm, deps)
    SujianAppLayoutResolution(capabilities, vm)

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
            modifier = Modifier.fillMaxSize(),
        )
    }
}
