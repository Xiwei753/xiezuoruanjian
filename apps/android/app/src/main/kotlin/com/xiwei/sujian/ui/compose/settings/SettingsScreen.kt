package com.xiwei.sujian.ui.compose.settings

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

@Composable
fun SettingsScreen(
    onReturnFromSettings: (() -> Unit)? = null,
    modifier: Modifier = Modifier
) {
    SettingsRoute(
        onNavigateBack = onReturnFromSettings,
        modifier = modifier,
    )
}
