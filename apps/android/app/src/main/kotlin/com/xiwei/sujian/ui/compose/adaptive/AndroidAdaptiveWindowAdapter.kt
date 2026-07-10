package com.xiwei.sujian.ui.compose.adaptive

import android.app.Activity
import androidx.lifecycle.lifecycleScope
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import androidx.window.layout.WindowLayoutInfo
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

class AndroidAdaptiveWindowAdapter(private val activity: Activity) {

    private val windowInfoTracker = WindowInfoTracker.getOrCreate(activity)

    val windowLayoutInfo: StateFlow<WindowLayoutInfo>? = null

    fun startCollecting(onFoldFeatureChanged: (List<FoldingFeature>) -> Unit) {
        activity.lifecycleScope.launch {
            windowInfoTracker.windowLayoutInfo(activity)
                .collect { info ->
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
