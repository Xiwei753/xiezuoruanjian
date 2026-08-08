package com.xiwei.sujian.core.designsystem.theme

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.Easing
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

@Immutable
data class SujianMotion(
    val instantDurationMs: Int = 50,
    val quickDurationMs: Int = 150,
    val standardDurationMs: Int = 300,
    val emphasizedDurationMs: Int = 500,
    val standardEasing: Easing = CubicBezierEasing(0.2f, 0f, 0f, 1f),
    val emphasizedEasing: Easing = CubicBezierEasing(0.2f, 0f, 0f, 1f),
    val emphasizedDecelerateEasing: Easing = CubicBezierEasing(0f, 0f, 0f, 1f),
    val emphasizedAccelerateEasing: Easing = CubicBezierEasing(0.3f, 0f, 0.8f, 0.15f),
    val linearEasing: Easing = CubicBezierEasing(0f, 0f, 1f, 1f),
) {
    companion object {
        fun reducedMotion(): SujianMotion = SujianMotion(
            instantDurationMs = 0,
            quickDurationMs = 0,
            standardDurationMs = 0,
            emphasizedDurationMs = 0,
            standardEasing = Easing { it },
            emphasizedEasing = Easing { it },
            emphasizedDecelerateEasing = Easing { it },
            emphasizedAccelerateEasing = Easing { it },
            linearEasing = Easing { it },
        )
    }
}

@Immutable
data class SujianElevation(
    val level0: Dp = 0.dp,
    val level1: Dp = 1.dp,
    val level2: Dp = 3.dp,
    val level3: Dp = 6.dp,
    val level4: Dp = 8.dp,
    val level5: Dp = 12.dp,
)

val LocalSujianMotion = staticCompositionLocalOf { SujianMotion() }
val LocalSujianElevation = staticCompositionLocalOf { SujianElevation() }
