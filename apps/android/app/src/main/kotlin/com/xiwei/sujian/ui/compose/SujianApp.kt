package com.xiwei.sujian.ui.compose

import android.app.Activity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.window.layout.FoldingFeature
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WorkspaceUseCase
import com.xiwei.sujian.model.FoldFeatureInfo
import com.xiwei.sujian.model.FoldOcclusion
import com.xiwei.sujian.model.FoldOrientation
import com.xiwei.sujian.model.FoldState
import com.xiwei.sujian.ui.compose.adaptive.AndroidAdaptiveWindowAdapter
import com.xiwei.sujian.ui.compose.adaptive.AndroidFoldFeatureInfo
import com.xiwei.sujian.ui.compose.adaptive.FoldOcclusionType
import com.xiwei.sujian.ui.compose.adaptive.FoldOrientation as AdaptiveFoldOrientation
import com.xiwei.sujian.ui.compose.adaptive.FoldState as AdaptiveFoldState
import com.xiwei.sujian.ui.compose.navigation.SujianNavigationSuite
import com.xiwei.sujian.ui.compose.theme.SujianTheme

@Composable
fun SujianApp() {
    val context = LocalContext.current
    val appState = rememberSujianAppState()

    LaunchedEffect(Unit) {
        val workspaceRepo = WorkspaceRepository(context)
        val settingsRepo = SettingsRepository(context)
        val workspaceUC = WorkspaceUseCase(workspaceRepo)
        appState.initialize(workspaceRepo, workspaceUC, settingsRepo)
    }

    val activity = LocalContext.current as? Activity
    var foldingFeatures by remember { mutableStateOf<List<FoldingFeature>>(emptyList()) }

    if (activity != null) {
        val adapter = remember { AndroidAdaptiveWindowAdapter(activity) }
        DisposableEffect(adapter) {
            adapter.startCollecting { features ->
                foldingFeatures = features
            }
            onDispose { }
        }

        LaunchedEffect(foldingFeatures) {
            val coreFoldInfo = if (foldingFeatures.isNotEmpty()) {
                val feature = foldingFeatures.first()
                val info = AndroidAdaptiveWindowAdapter.toFoldFeatureInfo(feature)
                FoldFeatureInfo(
                    state = when (info.state) {
                        AdaptiveFoldState.Flat -> FoldState.Flat
                        AdaptiveFoldState.HalfOpened -> FoldState.HalfOpened
                        else -> FoldState.None
                    },
                    orientation = if (info.orientation == AdaptiveFoldOrientation.Horizontal) FoldOrientation.Horizontal else FoldOrientation.Vertical,
                    isSeparating = info.isSeparating,
                    occlusion = if (info.occlusionType == FoldOcclusionType.Full) FoldOcclusion.Full else FoldOcclusion.None,
                    boundsLeftVp = info.boundsLeft.toFloat(),
                    boundsTopVp = info.boundsTop.toFloat(),
                    boundsRightVp = info.boundsRight.toFloat(),
                    boundsBottomVp = info.boundsBottom.toFloat()
                )
            } else {
                FoldFeatureInfo()
            }
            appState.updateFoldFeature(coreFoldInfo)
        }
    }

    SujianTheme {
        Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
            SujianNavigationSuite(
                appState = appState,
                modifier = Modifier.padding(innerPadding)
            )
        }
    }
}
