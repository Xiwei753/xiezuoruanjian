package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions

@Composable
fun SaveSettings(
    state: SaveSectionState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    var autoSaveDelay by rememberSaveable(state.autoSaveDelayMs / 1000f) {
        mutableFloatStateOf(state.autoSaveDelayMs / 1000f)
    }

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space16),
    ) {
        SettingsFieldGroup(title = stringResource(id = R.string.pref_category_save)) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_save),
                checked = state.autoSaveEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(autoSaveEnabled = checked) })
                },
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianSlider(
                title = stringResource(id = R.string.pref_auto_save_delay),
                value = autoSaveDelay,
                onValueChange = { autoSaveDelay = it },
                onValueChangeFinished = {
                    onIntent(SettingsIntent.UpdateLocal { it.copy(autoSaveDelayMs = (autoSaveDelay * 1000).toLong()) })
                },
                valueRange = 1f..10f,
                steps = 8,
                valueLabel =
                    pluralStringResource(
                        id = R.plurals.auto_save_delay_seconds,
                        autoSaveDelay.toInt(),
                        autoSaveDelay.toInt(),
                    ),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
