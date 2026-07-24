package com.xiwei.sujian.ui.compose.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.designsystem.component.SujianSection
import com.xiwei.sujian.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions

@Composable
fun SyncSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val syncConfig = state.syncConfig
    val syncCapability = state.syncCapability
    val dims = LocalSujianDimensions.current

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space16),
    ) {
        if (!syncCapability.canRun && syncCapability.blockReasonCode != null) {
            val blockMessage = resolveBlockMessage(syncCapability.blockReasonCode, syncCapability.blockMessageKey)
            SujianSection(title = stringResource(id = R.string.pref_category_sync)) {
                Text(
                    text = blockMessage,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }

        if (state.secureStorageWarning != null) {
            SujianSection(title = stringResource(id = R.string.pref_category_sync)) {
                Text(
                    text = stringResource(id = R.string.sync_migration_warning),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }

        SujianSection(title = stringResource(id = R.string.pref_category_sync)) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_enable_sync),
                checked = syncConfig.enabled ?: false,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(enabled = checked)))
                },
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_sync),
                checked = syncConfig.autoSync ?: false,
                onCheckedChange = { checked ->
                    onIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(autoSync = checked)))
                },
            )
            Spacer(modifier = Modifier.height(dims.space16))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_dry_run),
                onClick = { },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_test_connection),
                onClick = { },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_perform_sync),
                onClick = { },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@Composable
private fun resolveBlockMessage(blockReasonCode: String, blockMessageKey: String?): String {
    return when (blockReasonCode) {
        "DISABLED" -> stringResource(id = R.string.sync_block_disabled)
        "SECURE_STORAGE_UNAVAILABLE" -> stringResource(id = R.string.sync_block_secure_storage_unavailable)
        "REMOTE_URL_MISSING" -> stringResource(id = R.string.sync_block_remote_url_missing)
        "TOKEN_MISSING" -> stringResource(id = R.string.sync_block_token_missing)
        else -> blockMessageKey ?: blockReasonCode
    }
}
