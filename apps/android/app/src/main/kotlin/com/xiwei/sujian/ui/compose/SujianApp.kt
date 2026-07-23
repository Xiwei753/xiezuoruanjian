package com.xiwei.sujian.ui.compose

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.model.Orientation
import com.xiwei.sujian.model.PointerKind
import com.xiwei.sujian.model.WindowMetrics
import com.xiwei.sujian.ui.compose.adaptive.rememberAdaptiveWindowState
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.editor.v2.compose.AnimatedTextEditorSlot
import com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.ui.compose.navigation.SujianNavigationSuite
import com.xiwei.sujian.ui.compose.theme.SujianTheme
import com.xiwei.sujian.ui.compose.theme.ThemeStore
import com.xiwei.sujian.ui.compose.theme.rememberThemeController

@Composable
fun SujianApp(
    initialDestination: String? = null,
) {
    val context = LocalContext.current
    val vm: SujianAppViewModel = viewModel()
    val appState = remember { SujianAppState(vm) }
    val themeController = rememberThemeController(context)

    val coordinator = remember {
        val bridge = BridgeProvider.getAppServiceBridge(context)
        AnimatedTextEditorCoordinator(context, bridge)
    }

    LaunchedEffect(Unit) {
        val workspaceRepo = WorkspaceRepository(context)
        val settingsRepo = SettingsRepository(context)
        val workspaceUC = WorkspaceUseCase(workspaceRepo)
        vm.initialize(workspaceRepo, workspaceUC, settingsRepo, context)
        if (initialDestination == "settings") {
            vm.navigateTo(com.xiwei.sujian.ui.compose.navigation.SujianDestination.Settings)
        }
    }

    val windowState = rememberAdaptiveWindowState()
    val configuration = LocalConfiguration.current
    val density = LocalDensity.current.density

    LaunchedEffect(windowState.foldingFeatures, configuration.screenWidthDp, configuration.screenHeightDp) {
        vm.updateFoldFeaturesFromAdaptive(windowState.foldingFeatures, density)
        val hasFoldFeature = windowState.foldingFeatures.isNotEmpty()
        val settingsRepo = SettingsRepository(context)
        val deviceClass = settingsRepo.detectDeviceClassFromFoldFeature(
            hasFoldFeature, configuration.smallestScreenWidthDp
        )
        ThemeStore.setFoldDeviceClass(deviceClass)
        val metrics = WindowMetrics(
            widthDp = configuration.screenWidthDp.toFloat(),
            heightDp = configuration.screenHeightDp.toFloat(),
            foldFeature = vm.foldFeatureInfo,
            orientation = if (configuration.screenWidthDp > configuration.screenHeightDp) Orientation.Landscape else Orientation.Portrait,
            pointer = PointerKind.Touch
        )
        vm.resolveLayout(metrics)
    }

    val uiState by themeController.uiState.collectAsState()

    SujianTheme(uiState = uiState) {
        androidx.compose.runtime.CompositionLocalProvider(
            LocalAnimatedTextEditorCoordinator provides coordinator
        ) {
            Box(modifier = Modifier.fillMaxSize()) {
                SujianNavigationSuite(
                    appState = appState,
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
