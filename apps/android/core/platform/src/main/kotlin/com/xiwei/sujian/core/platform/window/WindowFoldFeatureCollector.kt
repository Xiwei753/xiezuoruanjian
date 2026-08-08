package com.xiwei.sujian.core.platform.window

import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.lifecycleScope
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import com.xiwei.sujian.core.platform.api.FoldOrientation
import com.xiwei.sujian.core.platform.api.FoldPosture
import com.xiwei.sujian.core.platform.api.OcclusionType
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

data class AospFoldFeatureInfo(
    val state: FoldPosture,
    val orientation: FoldOrientation,
    val isSeparating: Boolean,
    val occlusionType: OcclusionType,
    val boundsLeft: Int,
    val boundsTop: Int,
    val boundsRight: Int,
    val boundsBottom: Int
)

class WindowFoldFeatureCollector(
    private val activity: androidx.activity.ComponentActivity
) {
    private val windowInfoTracker = WindowInfoTracker.getOrCreate(activity)

    private var collectJob: Job? = null

    fun startCollecting(onFoldFeaturesChanged: (List<FoldingFeature>) -> Unit) {
        stopCollecting()
        collectJob = activity.lifecycleScope.launch {
            windowInfoTracker.windowLayoutInfo(activity)
                .collect { info ->
                    val features = info.displayFeatures
                        .filterIsInstance<FoldingFeature>()
                    onFoldFeaturesChanged(features)
                }
        }
    }

    fun stopCollecting() {
        collectJob?.cancel()
        collectJob = null
    }

    companion object {
        fun toFoldFeatureInfo(feature: FoldingFeature): AospFoldFeatureInfo {
            val state = when (feature.state) {
                FoldingFeature.State.FLAT -> FoldPosture.Flat
                FoldingFeature.State.HALF_OPENED -> FoldPosture.HalfOpened
                else -> FoldPosture.None
            }
            val orientation = if (feature.orientation == FoldingFeature.Orientation.HORIZONTAL) {
                FoldOrientation.Horizontal
            } else {
                FoldOrientation.Vertical
            }
            val occlusion = when (feature.occlusionType) {
                FoldingFeature.OcclusionType.FULL -> OcclusionType.Full
                else -> OcclusionType.None
            }
            val bounds = feature.bounds
            return AospFoldFeatureInfo(
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
