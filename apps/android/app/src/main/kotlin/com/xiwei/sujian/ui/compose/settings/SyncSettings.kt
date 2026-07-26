package com.xiwei.sujian.ui.compose.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.designsystem.component.SujianSection
import com.xiwei.sujian.designsystem.component.SujianSlider
import com.xiwei.sujian.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.editor.v2.compose.AnimatedTextField
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile

@Composable
fun SyncSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val syncConfig = state.syncConfig
    val syncSecrets = state.syncSecrets
    val syncCapability = state.syncCapability
    val dims = LocalSujianDimensions.current

    var remoteUrl by rememberSaveable { mutableStateOf(syncConfig.remoteUrl ?: "") }
    var branch by rememberSaveable { mutableStateOf(syncConfig.branch ?: "main") }
    var token by rememberSaveable { mutableStateOf(syncSecrets.token ?: "") }
    var syncInterval by rememberSaveable { mutableFloatStateOf((syncConfig.syncIntervalSeconds ?: 300).toFloat()) }

    LaunchedEffect(syncConfig.remoteUrl) { remoteUrl = syncConfig.remoteUrl ?: "" }
    LaunchedEffect(syncConfig.branch) { branch = syncConfig.branch ?: "main" }
    LaunchedEffect(syncSecrets.token) { token = syncSecrets.token ?: "" }
    LaunchedEffect(syncConfig.syncIntervalSeconds) { syncInterval = (syncConfig.syncIntervalSeconds ?: 300).toFloat() }

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
                enabled = syncConfig.enabled ?: false,
            )
        }

        SujianSection(title = stringResource(id = R.string.pref_category_sync)) {
            AnimatedTextField(
                targetId = "sync_remote_url",
                value = remoteUrl,
                onValueChange = { remoteUrl = it },
                onCommit = { text ->
                    remoteUrl = text
                    onIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(remoteUrl = text)))
                },
                profile = TextEditorProfile.RepositoryUrl,
                label = { Text(stringResource(id = R.string.pref_github_repo)) },
                enabled = syncConfig.enabled ?: false,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            AnimatedTextField(
                targetId = "sync_branch",
                value = branch,
                onValueChange = { branch = it },
                onCommit = { text ->
                    branch = text
                    onIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(branch = text)))
                },
                profile = TextEditorProfile.BranchName,
                label = { Text(stringResource(id = R.string.pref_branch)) },
                enabled = syncConfig.enabled ?: false,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            AnimatedTextField(
                targetId = "sync_token",
                value = token,
                onValueChange = { token = it },
                onCommit = { text ->
                    token = text
                    onIntent(SettingsIntent.UpdateSyncSecrets(syncSecrets.copy(token = text.ifBlank { null })))
                },
                profile = TextEditorProfile.SecretToken,
                label = { Text(stringResource(id = R.string.pref_https_token)) },
                enabled = syncConfig.enabled ?: false,
            )
            Spacer(modifier = Modifier.height(dims.space16))
            SujianSlider(
                title = stringResource(id = R.string.pref_sync_interval),
                value = syncInterval,
                onValueChange = { syncInterval = it },
                onValueChangeFinished = {
                    val seconds = syncInterval.toInt().coerceAtLeast(60)
                    onIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(syncIntervalSeconds = seconds)))
                },
                valueRange = 60f..3600f,
                steps = 5,
                valueFormatter = { v ->
                    val minutes = (v / 60).toInt()
                    if (minutes >= 1) "${minutes}min" else "${v.toInt()}s"
                },
                enabled = syncConfig.enabled ?: false,
            )
        }

        if (syncConfig.enabled == true) {
            val anySyncRunning = state.dryRunState == SyncCommandState.RUNNING ||
                state.testConnectionState == SyncCommandState.RUNNING ||
                state.performSyncState == SyncCommandState.RUNNING
            SujianSection(title = stringResource(id = R.string.pref_category_sync)) {
                SujianOutlinedButton(
                    text = stringResource(id = R.string.btn_dry_run),
                    onClick = { onIntent(SettingsIntent.DryRun) },
                    modifier = Modifier.fillMaxWidth(),
                    loading = state.dryRunState == SyncCommandState.RUNNING,
                    enabled = syncCapability.canRun && !anySyncRunning,
                )
                Spacer(modifier = Modifier.height(dims.space8))
                SujianOutlinedButton(
                    text = stringResource(id = R.string.btn_test_connection),
                    onClick = { onIntent(SettingsIntent.TestConnection) },
                    modifier = Modifier.fillMaxWidth(),
                    loading = state.testConnectionState == SyncCommandState.RUNNING,
                    enabled = syncCapability.canRun && !anySyncRunning,
                )
                Spacer(modifier = Modifier.height(dims.space8))
                SujianOutlinedButton(
                    text = stringResource(id = R.string.btn_perform_sync),
                    onClick = { onIntent(SettingsIntent.PerformSync) },
                    modifier = Modifier.fillMaxWidth(),
                    loading = state.performSyncState == SyncCommandState.RUNNING,
                    enabled = syncCapability.canRun && !anySyncRunning,
                )

                val structured = state.structuredSyncResult
                if (structured != null) {
                    Spacer(modifier = Modifier.height(dims.space8))
                    val isSuccess = structured.statusCode == "ok"
                    val displayResult = resolveStructuredResult(structured)
                    Text(
                        text = displayResult,
                        color = if (isSuccess) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
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
        "sync_already_running" -> stringResource(id = R.string.sync_already_running)
        else -> blockMessageKey ?: blockReasonCode
    }
}

@Composable
private fun resolveStructuredResult(result: StructuredSyncResult): String {
    return when (result.messageKey) {
        "sync_dry_run_result" -> stringResource(
            id = R.string.sync_dry_run_result,
            result.counts.uploaded,
            result.counts.downloaded,
            result.counts.deletedRemote,
            result.counts.deletedLocal,
            result.counts.conflicts
        )
        "sync_test_connection_result" -> {
            val net = result.messageArgs["network"] ?: ""
            val auth = result.messageArgs["auth"] ?: ""
            val repo = result.messageArgs["repo"] ?: ""
            val branch = result.messageArgs["branch"] ?: ""
            stringResource(
                id = R.string.sync_test_connection_result,
                translateStatusComponent(net),
                translateStatusComponent(auth),
                translateStatusComponent(repo),
                translateStatusComponent(branch)
            )
        }
        "sync_perform_result" -> stringResource(
            id = R.string.sync_perform_result,
            result.counts.uploaded,
            result.counts.downloaded,
            result.counts.deletedRemote,
            result.counts.deletedLocal,
            result.counts.conflicts
        )
        "dry_run_error" -> stringResource(id = R.string.dry_run_error)
        "diagnostics_error" -> stringResource(id = R.string.diagnostics_error)
        "sync_error" -> stringResource(id = R.string.sync_error)
        "core_not_loaded" -> stringResource(id = R.string.core_not_loaded)
        "unexpected_error" -> stringResource(id = R.string.unexpected_error)
        "sync_already_running" -> stringResource(id = R.string.sync_already_running)
        "save_config_failed" -> stringResource(id = R.string.save_sync_config_failed)
        "save_secrets_failed" -> stringResource(id = R.string.save_sync_secrets_failed)
        "sync_not_ready" -> stringResource(id = R.string.sync_block_not_ready)
        else -> result.statusCode
    }
}

@Composable
private fun translateStatusComponent(value: String): String {
    return when (value) {
        "ok" -> stringResource(id = R.string.sync_diag_ok)
        "fail" -> stringResource(id = R.string.sync_diag_fail)
        else -> value
    }
}
