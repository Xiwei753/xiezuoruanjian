package com.xiwei.sujian.ui.compose.settings

import androidx.compose.runtime.Composable
import com.xiwei.sujian.ui.compose.navigation.SettingsSection

@Composable
fun SettingsDetailPane(
    section: SettingsSection,
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: androidx.compose.ui.Modifier = androidx.compose.ui.Modifier,
) {
    when (section) {
        SettingsSection.Appearance -> AppearanceSettings(state = state, onIntent = onIntent, modifier = modifier)
        SettingsSection.Editor -> EditorSettings(state = state, onIntent = onIntent, modifier = modifier)
        SettingsSection.Save -> SaveSettings(state = state, onIntent = onIntent, modifier = modifier)
        SettingsSection.Sync -> SyncSettings(state = state, onIntent = onIntent, modifier = modifier)
        SettingsSection.Ai -> AiSettings(state = state, onIntent = onIntent, modifier = modifier)
        SettingsSection.Diagnostics -> DiagnosticsSettings(state = state, onIntent = onIntent, modifier = modifier)
        SettingsSection.Laboratory -> LaboratorySettings(state = state, onIntent = onIntent, modifier = modifier)
        SettingsSection.About -> AboutSettings(state = state, onIntent = onIntent, modifier = modifier)
    }
}
