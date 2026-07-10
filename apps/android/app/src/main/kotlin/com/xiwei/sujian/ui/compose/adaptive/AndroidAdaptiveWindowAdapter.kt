package com.xiwei.sujian.ui.compose.adaptive

import android.app.Activity
import androidx.lifecycle.lifecycleScope
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import androidx.window.layout.WindowLayoutInfo
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

class AndroidAdaptiveWindowAdapter(private val activity: Activity) {

    private val windowInfoTracker = WindowInfoTracker.getOrCreate(activity)
    private val _windowLayoutInfo = MutableStateFlow(WindowLayoutInfo(emptyList()))
    val windowLayoutInfo: StateFlow<WindowLayoutInfo> = _windowLayoutInfo.asStateFlow()

    fun startCollecting(onFoldFeatureChanged: (List<FoldingFeature>) -> Unit) {
        activity.lifecycleScope.launch {
            windowInfoTracker.windowLayoutInfo(activity)
                .collect { info ->
                    _windowLayoutInfo.value = info
                    val features = info.displayFeatures
                        .filterIsInstance<FoldingFeature>()
                    onFoldFeatureChanged(features)
                }
        }
    }

    companion object {
        fun toFoldFeatureInfo(feature: FoldingFeature): AndroidFoldFeatureInfo {
            val state = when (feature.state) {
                FoldingFeature.State.FLAT -> FoldState.Flat
                FoldingFeature.State.HALF_OPENED -> FoldState.HalfOpened
                else -> FoldState.None
            }
            val orientation = if (feature.orientation == FoldingFeature.Orientation.HORIZONTAL) {
                FoldOrientation.Horizontal
            } else {
                FoldOrientation.Vertical
            }
            val occlusion = when (feature.occlusionType) {
                FoldingFeature.OcclusionType.FULL -> FoldOcclusionType.Full
                else -> FoldOcclusionType.None
            }
            val bounds = feature.bounds
            return AndroidFoldFeatureInfo(
                state = state,
                orientation = orientation,
                isSeparating = feature.isSeparating,
                occlusionType = occlusion,
                boundsLeft = bounds.left,
                boundsTop = bounds.top,
                boundsRight = bounds.right,
                boundsBottom = bounds.bottom
            )
        }
    }
}
