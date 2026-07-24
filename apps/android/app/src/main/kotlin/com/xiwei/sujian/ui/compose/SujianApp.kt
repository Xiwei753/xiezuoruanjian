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
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.editor.v2.compose.AnimatedTextEditorSlot
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
    val themeController = rememberThemeController(context)
    val vendorRegistry = remember { VendorAdapterRegistry().also { VendorAdapterSetup.ensureInitialized(it) } }

    val capabilityProvider = remember { AospCapabilityProvider(context.applicationContext) }
    val capabilities by capabilityProvider.capabilities.collectAsState()

    DisposableEffect(capabilityProvider) {
        capabilityProvider.registerInputDeviceListener()
        onDispose {
            capabilityProvider.unregisterInputDeviceListener()
        }
    }

    val coordinator = remember {
        val bridge = BridgeProvider.getAppServiceBridge(context)
        AnimatedTextEditorCoordinator(context, bridge)
    }

    LaunchedEffect(Unit) {
        val workspaceRepo = WorkspaceRepository(context)
        val settingsRepo = SettingsRepository(context)
        val workspaceUC = WorkspaceUseCase(workspaceRepo)
        vm.initialize(workspaceRepo, workspaceUC, settingsRepo, context)
    }

    val activity = LocalContext.current as? androidx.activity.ComponentActivity
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

    val configuration = LocalConfiguration.current
    val density = LocalDensity.current.density

    LaunchedEffect(foldingFeatures, configuration.screenWidthDp, configuration.screenHeightDp) {
        capabilityProvider.updateFromFoldFeatures(foldingFeatures)
        capabilityProvider.updateFromConfiguration(configuration)

        vm.updateFoldFeaturesFromAdaptive(foldingFeatures, density)

        val settingsRepo = SettingsRepository(context)
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
            LocalAnimatedTextEditorCoordinator provides coordinator,
        ) {
            Box(modifier = Modifier.fillMaxSize()) {
                SujianNavigationSuite(
                    appState = appState,
                    initialDestination = initialDestination,
                    modifier = Modifier.fillMaxSize()
                )
                AnimatedTextEditorSlot(
                    coordinator = coordinator,
                    modifier = Modifier.fillMaxSize()
                )
            }
        }
    }
}
