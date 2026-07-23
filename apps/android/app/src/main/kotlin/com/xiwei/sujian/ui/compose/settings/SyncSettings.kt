package com.xiwei.sujian.ui.compose.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.designsystem.component.SujianSection
import com.xiwei.sujian.designsystem.component.SujianSwitchRow

@Composable
fun SyncSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val syncConfig = state.syncConfig

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SujianSection(title = stringResource(id = R.string.pref_category_sync)) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_enable_sync),
                checked = syncConfig.enabled ?: false,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(enabled = checked)))
                },
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_sync),
                checked = syncConfig.autoSync ?: false,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(autoSync = checked)))
                },
            )
            Spacer(modifier = Modifier.height(16.dp))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_dry_run),
                onClick = { },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_test_connection),
                onClick = { },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_perform_sync),
                onClick = { },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
