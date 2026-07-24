package com.xiwei.sujian.designsystem.theme

import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

@Immutable
data class SujianDimensions(
    val space2: Dp = 2.dp,
    val space4: Dp = 4.dp,
    val space8: Dp = 8.dp,
    val space12: Dp = 12.dp,
    val space16: Dp = 16.dp,
    val space20: Dp = 20.dp,
    val space24: Dp = 24.dp,
    val space32: Dp = 32.dp,
    val space40: Dp = 40.dp,
    val space48: Dp = 48.dp,
    val space56: Dp = 56.dp,
    val space64: Dp = 64.dp,
    val contentMaxWidth: Dp = 840.dp,
    val sidebarWidth: Dp = 320.dp,
    val listPanePreferredWidth: Dp = 360.dp,
    val detailPaneMinWidth: Dp = 400.dp,
    val minTouchTarget: Dp = 48.dp,
    val iconSizeSmall: Dp = 18.dp,
    val iconSizeMedium: Dp = 24.dp,
    val iconSizeLarge: Dp = 36.dp,
    val bodyLineHeight: Dp = 24.dp,
    val dialogListHeight: Dp = 120.dp,
)

val LocalSujianDimensions = staticCompositionLocalOf { SujianDimensions() }
