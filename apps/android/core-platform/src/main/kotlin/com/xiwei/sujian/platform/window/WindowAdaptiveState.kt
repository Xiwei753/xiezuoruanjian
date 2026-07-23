package com.xiwei.sujian.platform.window

import androidx.window.layout.FoldingFeature
import com.xiwei.sujian.platform.api.FoldOrientation
import com.xiwei.sujian.platform.api.FoldPosture
import com.xiwei.sujian.platform.api.OcclusionType

data class WindowAdaptiveState(
    val foldingFeatures: List<FoldingFeature> = emptyList(),
)

data class FoldPostureInfo(
    val state: FoldPosture = FoldPosture.None,
    val orientation: FoldOrientation = FoldOrientation.Vertical,
    val isSeparating: Boolean = false,
    val occlusionType: OcclusionType = OcclusionType.None,
    val boundsLeft: Int = 0,
    val boundsTop: Int = 0,
    val boundsRight: Int = 0,
    val boundsBottom: Int = 0,
)

fun FoldingFeature.toFoldPostureInfo(): FoldPostureInfo {
    return FoldPostureInfo(
        state = when (state) {
            FoldingFeature.State.FLAT -> FoldPosture.Flat
            FoldingFeature.State.HALF_OPENED -> FoldPosture.HalfOpened
            else -> FoldPosture.None
        },
        orientation = if (orientation == FoldingFeature.Orientation.HORIZONTAL) {
            FoldOrientation.Horizontal
        } else {
            FoldOrientation.Vertical
        },
        isSeparating = isSeparating,
        occlusionType = when (occlusionType) {
            FoldingFeature.OcclusionType.FULL -> OcclusionType.Full
            FoldingFeature.OcclusionType.NONE -> OcclusionType.None
            else -> OcclusionType.Partial
        },
        boundsLeft = bounds.left,
        boundsTop = bounds.top,
        boundsRight = bounds.right,
        boundsBottom = bounds.bottom,
    )
}
