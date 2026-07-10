package com.xiwei.sujian.ui.compose

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.model.Orientation
import com.xiwei.sujian.model.PointerKind
import com.xiwei.sujian.model.WindowMetrics
import com.xiwei.sujian.ui.compose.adaptive.rememberAdaptiveWindowState
import com.xiwei.sujian.ui.compose.navigation.SujianNavigationSuite
import com.xiwei.sujian.ui.compose.theme.SujianTheme
import com.xiwei.sujian.ui.compose.theme.rememberThemeController

@Composable
fun SujianApp() {
    val context = LocalContext.current
    val appState = rememberSujianAppState()
    val themeController = rememberThemeController(context)

    LaunchedEffect(Unit) {
        val workspaceRepo = WorkspaceRepository(context)
        val settingsRepo = SettingsRepository(context)
        val workspaceUC = WorkspaceUseCase(workspaceRepo)
        appState.initialize(workspaceRepo, workspaceUC, settingsRepo)
    }

    val windowState = rememberAdaptiveWindowState()
    val configuration = LocalConfiguration.current

    LaunchedEffect(windowState.foldingFeatures, configuration.screenWidthDp, configuration.screenHeightDp) {
        appState.updateFoldFeaturesFromAdaptive(windowState.foldingFeatures)
        val metrics = WindowMetrics(
            widthDp = configuration.screenWidthDp.toFloat(),
            heightDp = configuration.screenHeightDp.toFloat(),
            foldFeature = appState.foldFeatureInfo,
            orientation = if (configuration.screenWidthDp > configuration.screenHeightDp) Orientation.Landscape else Orientation.Portrait,
            pointer = PointerKind.Touch
        )
        appState.resolveLayout(metrics)
    }

    val uiState by themeController.uiState.collectAsState()

    SujianTheme(uiState = uiState) {
        Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
            SujianNavigationSuite(
                appState = appState,
                modifier = Modifier.padding(innerPadding)
            )
        }
    }
}
