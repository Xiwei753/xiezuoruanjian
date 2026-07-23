package com.xiwei.sujian.platform.window

import androidx.window.layout.FoldingFeature
import com.xiwei.sujian.platform.api.FoldOrientation
import com.xiwei.sujian.platform.api.FoldPosture
import com.xiwei.sujian.platform.api.OcclusionType

data class FoldPostureState(
    val posture: FoldPosture = FoldPosture.None,
    val orientation: FoldOrientation = FoldOrientation.Vertical,
    val isSeparating: Boolean = false,
    val occlusionType: OcclusionType = OcclusionType.None,
    val hingeBoundsLeft: Int = 0,
    val hingeBoundsTop: Int = 0,
    val hingeBoundsRight: Int = 0,
    val hingeBoundsBottom: Int = 0,
)

class FoldPostureAdapter {

    fun resolveFromFeatures(features: List<FoldingFeature>): FoldPostureState {
        val foldingFeature = features.firstOrNull() ?: return FoldPostureState()
        return FoldPostureState(
            posture = when (foldingFeature.state) {
                FoldingFeature.State.FLAT -> FoldPosture.Flat
                FoldingFeature.State.HALF_OPENED -> FoldPosture.HalfOpened
                else -> FoldPosture.None
            },
            orientation = if (foldingFeature.orientation == FoldingFeature.Orientation.HORIZONTAL) {
                FoldOrientation.Horizontal
            } else {
                FoldOrientation.Vertical
            },
            isSeparating = foldingFeature.isSeparating,
            occlusionType = if (foldingFeature.occlusionType == FoldingFeature.OcclusionType.FULL) {
                OcclusionType.Full
            } else if (foldingFeature.occlusionType == FoldingFeature.OcclusionType.NONE) {
                OcclusionType.None
            } else {
                OcclusionType.Partial
            },
            hingeBoundsLeft = foldingFeature.bounds.left,
            hingeBoundsTop = foldingFeature.bounds.top,
            hingeBoundsRight = foldingFeature.bounds.right,
            hingeBoundsBottom = foldingFeature.bounds.bottom,
        )
    }

    fun shouldAvoidHingeRegion(state: FoldPostureState): Boolean {
        return state.posture == FoldPosture.HalfOpened && state.isSeparating
    }

    fun hingeHeightDp(state: FoldPostureState, density: Float): Float {
        if (state.posture == FoldPosture.None) return 0f
        val heightPx = state.hingeBoundsBottom - state.hingeBoundsTop
        return if (density > 0f) heightPx / density else 0f
    }

    fun hingeWidthDp(state: FoldPostureState, density: Float): Float {
        if (state.posture == FoldPosture.None) return 0f
        val widthPx = state.hingeBoundsRight - state.hingeBoundsLeft
        return if (density > 0f) widthPx / density else 0f
    }
}
