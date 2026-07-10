package com.xiwei.sujian.ui.compose

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WorkspaceUseCase
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

    LaunchedEffect(windowState.foldingFeatures) {
        appState.updateFoldFeaturesFromAdaptive(windowState.foldingFeatures)
    }

    SujianTheme(uiState = themeController.uiState.value) {
        Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
            SujianNavigationSuite(
                appState = appState,
                modifier = Modifier.padding(innerPadding)
            )
        }
    }
}
