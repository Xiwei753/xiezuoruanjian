package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
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
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.core.designsystem.component.SujianSecretTextField
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.component.SujianTextField
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents

/**
 * #630 评论13/评论15: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * Sync 分类字段最多，每个字段独立 item 避免任一状态变化让整个同步面板重组。
 */
fun LazyListScope.syncSettingsItems(vm: SettingsViewModel) {
    // ── 同步说明 ──
    item(key = "sync.description") {
        SettingsGroupItemContainer(isLast = false, isFirst = true) {
            Text(
                text = stringResource(id = R.string.sync_github_api_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.height(48.dp).padding(vertical = 8.dp),
            )
        }
    }

    // ── 启用分组标题 ──
    item(key = "sync.enable_title") {
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_sync))
        }
    }

    // 启用同步开关
    item(key = "sync.enable_sync") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val syncConfig = state.syncConfig
        SettingsGroupItemContainer(isLast = false) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_enable_sync),
                checked = syncConfig.enabled ?: false,
                onCheckedChange = { checked ->
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(enabled = checked)))
                },
            )
        }
    }

    // 自动同步开关
    item(key = "sync.auto_sync") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val syncConfig = state.syncConfig
        SettingsGroupItemContainer(isLast = true) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_sync),
                checked = syncConfig.autoSync ?: false,
                onCheckedChange = { checked ->
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(autoSync = checked)))
                },
                enabled = syncConfig.enabled ?: false,
            )
        }
    }

    // ── 凭据分组标题 ──
    item(key = "sync.credentials_title") {
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_sync_credentials))
        }
    }

    // 远程仓库地址
    item(key = "sync.remote_url") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val syncConfig = state.syncConfig
        var remoteUrl by rememberSaveable { mutableStateOf(syncConfig.remoteUrl ?: "") }
        LaunchedEffect(syncConfig.remoteUrl) { remoteUrl = syncConfig.remoteUrl ?: "" }
        SettingsGroupItemContainer(isLast = false) {
            SujianTextField(
                value = remoteUrl,
                onValueChange = {
                    remoteUrl = it
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(remoteUrl = it)))
                },
                label = { Text(stringResource(id = R.string.pref_github_repo)) },
                modifier = rememberFieldFocusModifier("sync_remote_url") { remoteUrl },
                enabled = syncConfig.enabled ?: false,
            )
        }
    }

    // 分支
    item(key = "sync.branch") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val syncConfig = state.syncConfig
        var branch by rememberSaveable { mutableStateOf(syncConfig.branch ?: "main") }
        LaunchedEffect(syncConfig.branch) { branch = syncConfig.branch ?: "main" }
        SettingsGroupItemContainer(isLast = false) {
            SujianTextField(
                value = branch,
                onValueChange = {
                    branch = it
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(branch = it)))
                },
                label = { Text(stringResource(id = R.string.pref_branch)) },
                modifier = rememberFieldFocusModifier("sync_branch") { branch },
                enabled = syncConfig.enabled ?: false,
            )
        }
    }

    // Token
    item(key = "sync.token") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val syncConfig = state.syncConfig
        val syncSecrets = state.syncSecrets
        var token by rememberSaveable { mutableStateOf(syncSecrets.token ?: "") }
        LaunchedEffect(syncSecrets.token) { token = syncSecrets.token ?: "" }
        SettingsGroupItemContainer(isLast = true) {
            SujianSecretTextField(
                value = token,
                onValueChange = {
                    token = it
                    vm.handleIntent(SettingsIntent.UpdateSyncSecrets(syncSecrets.copy(token = it.ifBlank { null })))
                },
                label = { Text(stringResource(id = R.string.pref_https_token)) },
                modifier = rememberFieldFocusModifier("sync_token") { token },
                enabled = syncConfig.enabled ?: false,
            )
        }
    }

    // ── 同步间隔 ──
    item(key = "sync.interval_title") {
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_sync_interval))
        }
    }

    item(key = "sync.interval") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val syncConfig = state.syncConfig
        var syncInterval by rememberSaveable { mutableFloatStateOf((syncConfig.syncIntervalSeconds ?: 300).toFloat()) }
        LaunchedEffect(
            syncConfig.syncIntervalSeconds,
        ) { syncInterval = (syncConfig.syncIntervalSeconds ?: 300).toFloat() }
        SettingsGroupItemContainer(isLast = true) {
            SujianSlider(
                title = stringResource(id = R.string.pref_sync_interval),
                value = syncInterval,
                onValueChange = { syncInterval = it },
                onValueChangeFinished = {
                    val seconds = syncInterval.toInt().coerceAtLeast(60)
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig(syncConfig.copy(syncIntervalSeconds = seconds)))
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
    }

    // ── 操作分组标题 ──
    item(key = "sync.actions_title") {
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_sync_actions))
        }
    }

    // Dry run 按钮
    item(key = "sync.dry_run") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val syncConfig = state.syncConfig
        val syncCapability = state.syncCapability
        val anySyncRunning =
            state.dryRunState == SyncCommandState.RUNNING ||
                state.testConnectionState == SyncCommandState.RUNNING ||
                state.performSyncState == SyncCommandState.RUNNING
        if (syncConfig.enabled == true) {
            SettingsGroupItemContainer(isLast = false) {
                SujianOutlinedButton(
                    text = stringResource(id = R.string.btn_dry_run),
                    onClick = { vm.handleIntent(SettingsIntent.DryRun) },
                    modifier = Modifier.fillMaxWidth(),
                    loading = state.dryRunState == SyncCommandState.RUNNING,
                    enabled = syncCapability.canRun && !anySyncRunning,
                )
            }
        }
    }

    // Test connection 按钮
    item(key = "sync.test_connection") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val syncConfig = state.syncConfig
        val syncCapability = state.syncCapability
        val anySyncRunning =
            state.dryRunState == SyncCommandState.RUNNING ||
                state.testConnectionState == SyncCommandState.RUNNING ||
                state.performSyncState == SyncCommandState.RUNNING
        if (syncConfig.enabled == true) {
            SettingsGroupItemContainer(isLast = false) {
                SujianOutlinedButton(
                    text = stringResource(id = R.string.btn_test_connection),
                    onClick = { vm.handleIntent(SettingsIntent.TestConnection) },
                    modifier = Modifier.fillMaxWidth(),
                    loading = state.testConnectionState == SyncCommandState.RUNNING,
                    enabled = syncCapability.canRun && !anySyncRunning,
                )
            }
        }
    }

    // Perform sync 按钮
    item(key = "sync.perform_sync") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val syncConfig = state.syncConfig
        val syncCapability = state.syncCapability
        val anySyncRunning =
            state.dryRunState == SyncCommandState.RUNNING ||
                state.testConnectionState == SyncCommandState.RUNNING ||
                state.performSyncState == SyncCommandState.RUNNING
        if (syncConfig.enabled == true) {
            SettingsGroupItemContainer(isLast = true) {
                SujianOutlinedButton(
                    text = stringResource(id = R.string.btn_perform_sync),
                    onClick = { vm.handleIntent(SettingsIntent.PerformSync) },
                    modifier = Modifier.fillMaxWidth(),
                    loading = state.performSyncState == SyncCommandState.RUNNING,
                    enabled = syncCapability.canRun && !anySyncRunning,
                )
            }
        }
    }

    // 同步结果
    item(key = "sync.result") {
        val state by vm.syncState.collectAsStateWithLifecycle()
        val structured = state.syncResult
        if (structured != null) {
            SettingsGroupItemContainer(isLast = true) {
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

// 以下辅助函数保持不变

@androidx.compose.runtime.Composable
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

@androidx.compose.runtime.Composable
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

@androidx.compose.runtime.Composable
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

@androidx.compose.runtime.Composable
private fun translateStatusComponent(value: String): String {
    return when (value) {
        "ok" -> stringResource(id = R.string.sync_diag_ok)
        "fail" -> stringResource(id = R.string.sync_diag_fail)
        else -> value
    }
}

/**
 * Issue #612 五：设置字段焦点/提交诊断事件。
 */
@androidx.compose.runtime.Composable
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
