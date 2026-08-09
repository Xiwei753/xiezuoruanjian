package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import com.xiwei.sujian.app.navigation.SettingsSection
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

@Composable
fun SettingsDetailPane(
    section: SettingsSection,
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(modifier = modifier.testTag(SujianSemanticIds.SettingsScreen)) {
        when (section) {
            SettingsSection.Appearance -> AppearanceSettings(state = state, onIntent = onIntent)
            SettingsSection.Editor -> EditorSettings(state = state, onIntent = onIntent)
            SettingsSection.Save -> SaveSettings(state = state, onIntent = onIntent)
            SettingsSection.Sync -> SyncSettings(state = state, onIntent = onIntent)
            SettingsSection.Ai -> AiSettings(state = state, onIntent = onIntent)
            SettingsSection.Diagnostics -> DiagnosticsSettings(state = state, onIntent = onIntent)
            SettingsSection.Laboratory -> LaboratorySettings(state = state, onIntent = onIntent)
            SettingsSection.About -> AboutSettings(state = state, onIntent = onIntent)
        }
    }
}
