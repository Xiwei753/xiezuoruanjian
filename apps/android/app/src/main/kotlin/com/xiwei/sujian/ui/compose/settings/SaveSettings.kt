package com.xiwei.sujian.ui.compose.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianSection
import com.xiwei.sujian.designsystem.component.SujianSlider
import com.xiwei.sujian.designsystem.component.SujianSwitchRow

@Composable
fun SaveSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val settings = state.settings
    var autoSaveDelay by remember { mutableFloatStateOf((settings.autoSaveDelayMs / 1000f)) }

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SujianSection(title = stringResource(id = R.string.pref_category_save)) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_save),
                checked = settings.autoSaveEnabled,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateLocal { it.copy(autoSaveEnabled = checked) })
                },
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianSlider(
                title = stringResource(id = R.string.pref_auto_save_delay),
                value = autoSaveDelay,
                onValueChange = { autoSaveDelay = it },
                onValueChangeFinished = {
                    onIntent(SettingsIntent.UpdateLocal { it.copy(autoSaveDelayMs = (autoSaveDelay * 1000).toLong()) })
                },
                valueRange = 1f..10f,
                steps = 8,
                valueLabel = stringResource(id = R.string.auto_save_delay_seconds, autoSaveDelay.toInt()),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
