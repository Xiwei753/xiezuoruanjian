package com.xiwei.sujian.ui.compose

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
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.model.Orientation
import com.xiwei.sujian.model.WindowMetrics
import com.xiwei.sujian.platform.api.AndroidCapabilities
import com.xiwei.sujian.platform.api.PointerKind
import com.xiwei.sujian.platform.aosp.AospCapabilityProvider
import com.xiwei.sujian.platform.window.WindowFoldFeatureCollector
import com.xiwei.sujian.ui.compose.navigation.SujianNavigationSuite
import com.xiwei.sujian.ui.compose.theme.SujianTheme
import com.xiwei.sujian.ui.compose.theme.ThemeStore
import com.xiwei.sujian.ui.compose.theme.rememberThemeController
import com.xiwei.sujian.platform.aosp.VendorAdapterSetup
import com.xiwei.sujian.platform.vendor.VendorAdapterRegistry
import com.xiwei.sujian.runtime.LocalSujianAppDependencies
import com.xiwei.sujian.runtime.SujianAppDependencies
import kotlinx.coroutines.launch

val LocalAndroidCapabilities = androidx.compose.runtime.compositionLocalOf<AndroidCapabilities> {
    AndroidCapabilities()
}

@Composable
fun SujianApp(
    initialDestination: String? = null,
) {
    val context = LocalContext.current
    val vm: SujianAppViewModel = viewModel()
    val appState = remember { SujianAppState(vm) }
    val app = context.applicationContext as? com.xiwei.sujian.SujianApp
    // #592 一：Compose UI 必须从同一个 Application 进程级容器取得依赖实例，
    // 不能再次 DefaultAppServiceContainer(context) 创建第二份容器。
    // 后台 Worker 也从同一容器取依赖，保证 SyncStatusRepository StateFlow
    // 和 SyncCoordinator 全进程唯一。
    val deps = remember {
        val testProvider = SujianAppDependencies.getTestProvider()
        testProvider?.invoke(context) ?: requireNotNull(app).dependencies
    }
    val activityRef = androidx.activity.compose.LocalActivity.current as? androidx.activity.ComponentActivity
    val windowCoordinator = remember {
        com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator(
            context.applicationContext, deps.appServiceBridge
        )
    }
    // #592 三：每次组合离开都释放窗口宿主，包括配置变化。
    // 旧实例的 releaseHost 清理 View、FrameClock 和 Rust 会话；
    // 新 Activity 的 remember 创建新实例，从 ViewModel 恢复 session 状态。
    DisposableEffect(windowCoordinator) {
        onDispose {
            windowCoordinator.releaseHost()
        }
    }
    val themeController = rememberThemeController(context, deps.settingsRepository)
    val vendorRegistry = remember { VendorAdapterRegistry().also { VendorAdapterSetup.ensureInitialized(it) } }

    val capabilityProvider = remember { AospCapabilityProvider(context.applicationContext) }
    val capabilities by capabilityProvider.capabilities.collectAsState()

    DisposableEffect(capabilityProvider) {
        capabilityProvider.registerInputDeviceListener()
        onDispose {
            capabilityProvider.unregisterInputDeviceListener()
        }
    }
    DisposableEffect(deps, activityRef) {
        val act = activityRef ?: return@DisposableEffect onDispose { }
        val observer = androidx.lifecycle.LifecycleEventObserver { _, event ->
            when (event) {
                androidx.lifecycle.Lifecycle.Event.ON_PAUSE -> com.xiwei.sujian.diagnostics.DiagnosticsEvents.activityLifecycle("pause")
                androidx.lifecycle.Lifecycle.Event.ON_DESTROY -> {
                    com.xiwei.sujian.diagnostics.DiagnosticsEvents.activityLifecycle("destroy")
                }
                else -> {}
            }
        }
        act.lifecycle.addObserver(observer)
        onDispose {
            act.lifecycle.removeObserver(observer)
        }
    }


    LaunchedEffect(Unit) {
        val workspaceUC = WorkspaceUseCase(deps.workspaceRepository)
        vm.initialize(deps.workspaceRepository, workspaceUC, deps.settingsRepository, context)
    }

    val activity = activityRef
    var foldingFeatures by remember { mutableStateOf<List<androidx.window.layout.FoldingFeature>>(emptyList()) }

    if (activity != null) {
        val lifecycleOwner = androidx.lifecycle.compose.LocalLifecycleOwner.current
        LaunchedEffect(lifecycleOwner) {
            lifecycleOwner.lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
                deps.syncStatusRepository.refreshState()
                vm.refreshProjects()
                vm.refreshRecentEdits()
            }
        }
    }

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

    val configuration = LocalConfiguration.current
    val density = LocalDensity.current.density

    LaunchedEffect(foldingFeatures, configuration.screenWidthDp, configuration.screenHeightDp) {
        capabilityProvider.updateFromFoldFeatures(foldingFeatures)
        capabilityProvider.updateFromConfiguration(configuration)

        vm.updateFoldFeaturesFromAdaptive(foldingFeatures, density)

        val settingsRepo = deps.settingsRepository
        val hasFoldFeature = foldingFeatures.isNotEmpty()
        val deviceClass = settingsRepo.detectDeviceClassFromFoldFeature(
            hasFoldFeature, configuration.smallestScreenWidthDp
        )
        ThemeStore.setFoldDeviceClass(deviceClass)
    }

    LaunchedEffect(capabilities) {
        val pointerKind = capabilities.activePointerKind
        val metrics = WindowMetrics(
            widthDp = configuration.screenWidthDp.toFloat(),
            heightDp = configuration.screenHeightDp.toFloat(),
            foldFeature = vm.foldFeatureInfo,
            orientation = if (configuration.screenWidthDp > configuration.screenHeightDp) Orientation.Landscape else Orientation.Portrait,
            pointer = when (pointerKind) {
                PointerKind.Mouse -> com.xiwei.sujian.model.PointerKind.Mouse
                PointerKind.Trackpad -> com.xiwei.sujian.model.PointerKind.Trackpad
                PointerKind.Stylus -> com.xiwei.sujian.model.PointerKind.Stylus
                else -> com.xiwei.sujian.model.PointerKind.Touch
            },
        )
        vm.resolveLayout(metrics)
    }

    val uiState by themeController.uiState.collectAsState()

    SujianTheme(uiState = uiState) {
        CompositionLocalProvider(
            LocalAndroidCapabilities provides capabilities,
            LocalAnimatedTextEditorCoordinator provides windowCoordinator,
            LocalSujianAppDependencies provides deps,
        ) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .pointerInput(capabilityProvider) {
                        awaitPointerEventScope {
                            while (true) {
                                val event = awaitPointerEvent(PointerEventPass.Initial)
                                val changes = event.changes
                                if (changes.isNotEmpty()) {
                                    val type = changes.first().type
                                    val kind = when (type) {
                                        PointerType.Mouse -> PointerKind.Mouse
                                        PointerType.Stylus -> PointerKind.Stylus
                                        PointerType.Eraser -> PointerKind.Stylus
                                        else -> PointerKind.Touch
                                    }
                                    capabilityProvider.updateActivePointerKind(kind)
                                }
                            }
                        }
                    }
            ) {
                SujianNavigationSuite(
                    appState = appState,
                    initialDestination = initialDestination,
                    modifier = Modifier.fillMaxSize()
                )
            }
        }
    }
}
