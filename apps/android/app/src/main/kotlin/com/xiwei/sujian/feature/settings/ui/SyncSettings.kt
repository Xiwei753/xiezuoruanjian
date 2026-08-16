package com.xiwei.sujian.feature.settings.ui

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
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.core.designsystem.component.SujianSecretTextField
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.component.SujianTextField
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents

/**
 * #630 评论 #1+#2：设置页同步区域只有一份 — 全量同步覆盖设置/星图/主题/全部作品。
 *
 * 编辑 [SettingsUiState.syncConfig]/[SettingsUiState.syncSecrets]，
 * 发送 [SettingsIntent.DryRun]/[SettingsIntent.TestConnection]/[SettingsIntent.PerformSync]。
 * 标题只出现一次（"同步"），supporting text 说明同步范围与 GitHub API 限制。
 */
@Composable
fun SyncSettings(
    state: SyncSectionState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current
    val syncConfig = state.syncConfig
    val syncSecrets = state.syncSecrets
    val syncCapability = state.syncCapability

    var remoteUrl by rememberSaveable { mutableStateOf(syncConfig.remoteUrl ?: "") }
    var branch by rememberSaveable { mutableStateOf(syncConfig.branch ?: "main") }
    var token by rememberSaveable { mutableStateOf(syncSecrets.token ?: "") }
    var syncInterval by rememberSaveable { mutableFloatStateOf((syncConfig.syncIntervalSeconds ?: 300).toFloat()) }

    LaunchedEffect(syncConfig.remoteUrl) { remoteUrl = syncConfig.remoteUrl ?: "" }
    LaunchedEffect(syncConfig.branch) { branch = syncConfig.branch ?: "main" }
    LaunchedEffect(syncSecrets.token) { token = syncSecrets.token ?: "" }
    LaunchedEffect(syncConfig.syncIntervalSeconds) { syncInterval = (syncConfig.syncIntervalSeconds ?: 300).toFloat() }

    SettingsSyncScope(title = stringResource(id = R.string.pref_category_sync)) {
        // supporting text：同步范围 + GitHub API 提示
        Text(
            text = stringResource(id = R.string.sync_github_api_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(bottom = dims.space12),
        )

        // #595 四：同步 profile 读取失败 → 显示真实错误
        if (state.syncProfileLoadState is SyncProfileLoadState.Failed) {
            val failed = state.syncProfileLoadState
            Text(
                text = resolveSyncFailureMessage(failed.kind.messageKey()),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(bottom = dims.space8),
            )
        }
        if (!syncCapability.canRun && syncCapability.blockReasonCode != null) {
            val blockMessage = resolveBlockMessage(syncCapability.blockReasonCode, syncCapability.blockMessageKey)
            Text(
                text = blockMessage,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(bottom = dims.space8),
            )
        }
        if (state.secureStorageWarning != null) {
            Text(
                text = stringResource(id = R.string.sync_migration_warning),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(bottom = dims.space8),
            )
        }

        // 启用/自动同步
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
        Spacer(modifier = Modifier.height(dims.space16))

        // GitHub 仓库/分支/Token/间隔
        SujianTextField(
            value = remoteUrl,
            onValueChange = {
                remoteUrl = it
                onIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(remoteUrl = it)))
            },
            label = { Text(stringResource(id = R.string.pref_github_repo)) },
            modifier = rememberFieldFocusModifier("sync_remote_url") { remoteUrl },
            enabled = syncConfig.enabled ?: false,
        )
        Spacer(modifier = Modifier.height(dims.space8))
        SujianTextField(
            value = branch,
            onValueChange = {
                branch = it
                onIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(branch = it)))
            },
            label = { Text(stringResource(id = R.string.pref_branch)) },
            modifier = rememberFieldFocusModifier("sync_branch") { branch },
            enabled = syncConfig.enabled ?: false,
        )
        Spacer(modifier = Modifier.height(dims.space8))
        SujianSecretTextField(
            value = token,
            onValueChange = {
                token = it
                onIntent(SettingsIntent.UpdateSyncSecrets(syncSecrets.copy(token = it.ifBlank { null })))
            },
            label = { Text(stringResource(id = R.string.pref_https_token)) },
            modifier = rememberFieldFocusModifier("sync_token") { token },
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

        // 检查/测试/立即同步
        if (syncConfig.enabled == true) {
            val anySyncRunning =
                state.dryRunState == SyncCommandState.RUNNING ||
                    state.testConnectionState == SyncCommandState.RUNNING ||
                    state.performSyncState == SyncCommandState.RUNNING
            Spacer(modifier = Modifier.height(dims.space16))
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

            val structured = state.syncResult
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

@Composable
private fun resolveSyncFailureMessage(messageKey: String): String =
    when (messageKey) {
        "sync_retryable_network" -> stringResource(id = R.string.sync_retryable_network)
        "sync_retryable_io" -> stringResource(id = R.string.sync_retryable_io)
        "sync_auth_failed" -> stringResource(id = R.string.sync_auth_failed)
        "sync_conflict" -> stringResource(id = R.string.sync_conflict)
        "sync_dirty_repository" -> stringResource(id = R.string.sync_dirty_repository)
        "sync_protocol_error" -> stringResource(id = R.string.sync_protocol_error)
        "sync_native_unavailable" -> stringResource(id = R.string.sync_native_unavailable)
        else -> stringResource(id = R.string.sync_fatal)
    }

@Composable
private fun resolveBlockMessage(
    blockReasonCode: String,
    blockMessageKey: String?,
): String {
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
        "sync_dry_run_result" ->
            pluralStringResource(
                id = R.plurals.sync_dry_run_result,
                count = result.counts.deletedLocal,
                result.counts.uploaded,
                result.counts.downloaded,
                result.counts.deletedRemote,
                result.counts.deletedLocal,
                result.counts.conflicts,
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
                translateStatusComponent(branch),
            )
        }
        "sync_perform_result" ->
            pluralStringResource(
                id = R.plurals.sync_perform_result,
                count = result.counts.conflicts,
                result.counts.uploaded,
                result.counts.downloaded,
                result.counts.deletedRemote,
                result.counts.deletedLocal,
                result.counts.conflicts,
            )
        "sync_terminal_failure" -> stringResource(id = R.string.sync_terminal_failure)
        "sync_retryable_failure" -> stringResource(id = R.string.sync_retryable_failure)
        "sync_retryable_network" -> stringResource(id = R.string.sync_retryable_network)
        "sync_retryable_io" -> stringResource(id = R.string.sync_retryable_io)
        "sync_auth_failed" -> stringResource(id = R.string.sync_auth_failed)
        "sync_conflict" -> stringResource(id = R.string.sync_conflict)
        "sync_dirty_repository" -> stringResource(id = R.string.sync_dirty_repository)
        "sync_protocol_error" -> stringResource(id = R.string.sync_protocol_error)
        "sync_native_unavailable" -> stringResource(id = R.string.sync_native_unavailable)
        "sync_fatal" -> stringResource(id = R.string.sync_fatal)
        "sync_unconfigured" -> stringResource(id = R.string.sync_unconfigured)
        "sync_disabled" -> stringResource(id = R.string.sync_disabled)
        "sync_busy" -> stringResource(id = R.string.sync_busy)
        "sync_unknown" -> stringResource(id = R.string.sync_unknown)
        "save_config_or_secrets_failed" -> stringResource(id = R.string.save_config_or_secrets_failed)
        "unexpected_error" -> stringResource(id = R.string.unexpected_error)
        "sync_already_running" -> stringResource(id = R.string.sync_already_running)
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

/**
 * Issue #612 五：设置字段焦点/提交诊断事件。
 * 获得焦点记录 fieldFocus(true)，失去焦点记录 fieldFocus(false) + fieldCommit(字符数, "blur")。
 * 用 wasFocused 防止初始组合（isFocused=false）产生噪声事件。
 */
@Composable
private fun rememberFieldFocusModifier(
    fieldType: String,
    value: () -> String,
): Modifier {
    var wasFocused by remember { mutableStateOf(false) }
    return Modifier.onFocusChanged { state ->
        val isFocused = state.isFocused
        if (isFocused == wasFocused) return@onFocusChanged
        wasFocused = isFocused
        DiagnosticsEvents.fieldFocus(fieldType, isFocused)
        if (!isFocused) {
            DiagnosticsEvents.fieldCommit(fieldType, value().length, "blur")
        }
    }
}
