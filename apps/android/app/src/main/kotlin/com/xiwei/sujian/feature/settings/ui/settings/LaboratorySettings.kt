package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSection
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions

@Composable
fun LaboratorySettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val settings = state.settings
    val dims = LocalSujianDimensions.current

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space16),
    ) {
        SujianSection(title = stringResource(id = R.string.pref_category_laboratory)) {
            SujianSwitchRow(
                title = stringResource(id = R.string.lab_fullscreen_immersive),
                checked = settings.experimentalFullscreenMode,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(experimentalFullscreenMode = checked) })
                },
                supportingText = stringResource(id = R.string.lab_fullscreen_immersive_summary),
            )
        }
    }
}
