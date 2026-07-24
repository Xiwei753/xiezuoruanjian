package com.xiwei.sujian.ui.compose.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianSection
import com.xiwei.sujian.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions

@Composable
fun AiSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (!state.aiAvailable) return
    val dims = LocalSujianDimensions.current

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space16),
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
