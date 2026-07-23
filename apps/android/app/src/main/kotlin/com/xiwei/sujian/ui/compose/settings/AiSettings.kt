package com.xiwei.sujian.ui.compose.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianSection
import com.xiwei.sujian.designsystem.component.SujianSwitchRow

@Composable
fun AiSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (!state.aiAvailable) return

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SujianSection(title = stringResource(id = R.string.pref_category_ai)) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_ai_enabled),
                checked = state.settings.aiEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(aiEnabled = checked) })
                },
            )
        }
    }
}
