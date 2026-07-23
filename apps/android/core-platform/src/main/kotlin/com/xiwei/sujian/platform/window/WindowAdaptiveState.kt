package com.xiwei.sujian.platform.window

import androidx.window.layout.FoldingFeature

data class WindowAdaptiveState(
    val foldingFeatures: List<FoldingFeature> = emptyList(),
)

data class FoldPostureInfo(
    val state: FoldState = FoldState.None,
    val orientation: FoldOrientation = FoldOrientation.Vertical,
    val isSeparating: Boolean = false,
    val boundsLeft: Int = 0,
    val boundsTop: Int = 0,
    val boundsRight: Int = 0,
    val boundsBottom: Int = 0,
)

enum class FoldState {
    None,
    Flat,
    HalfOpened,
}

enum class FoldOrientation {
    Horizontal,
    Vertical,
}

fun FoldingFeature.toFoldPostureInfo(): FoldPostureInfo {
    return FoldPostureInfo(
        state = when (state) {
            FoldingFeature.State.FLAT -> FoldState.Flat
            FoldingFeature.State.HALF_OPENED -> FoldState.HalfOpened
            else -> FoldState.None
        },
        orientation = if (orientation == FoldingFeature.Orientation.HORIZONTAL) {
            FoldOrientation.Horizontal
        } else {
            FoldOrientation.Vertical
        },
        isSeparating = isSeparating,
        boundsLeft = bounds.left,
        boundsTop = bounds.top,
        boundsRight = bounds.right,
        boundsBottom = bounds.bottom,
    )
}
