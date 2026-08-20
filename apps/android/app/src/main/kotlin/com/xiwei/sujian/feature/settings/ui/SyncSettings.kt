package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.LazyListScope
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
 * #630 R14：同步设置字段组模式 — 一个真实字段组一个 High Surface item，组内多个字段普通布局。
 * 使用 [SettingsExpandedGroupContainer] 统一 16dp content padding、12dp 圆角。
 * 每个 item 只 collect 自己需要的 row-level StateFlow，避免整分类重组。
 *
 * 字段组划分：
 * - sync_general: 说明 + 启用同步 + 自动同步
 * - sync_credentials: 凭据标题 + 远程仓库 + 分支 + Token
 * - sync_interval: 间隔标题 + 间隔 Slider
 * - sync_actions: 操作标题 + Dry run + Test + Perform + 结果
 */
fun LazyListScope.syncSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    // ── 同步通用组（说明 + 启用同步 + 自动同步）─
    item(key = "sync.general_group", contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP) {
        val enabledRow by vm.syncEnabledRow.collectAsStateWithLifecycle()
        val autoSyncRow by vm.syncAutoSyncRow.collectAsStateWithLifecycle()
        SettingsExpandedGroupContainer(
            closeOuterGroup = false,
            firstInGroup = true,
            lastInGroup = false,
        ) {
            Text(
                text = stringResource(id = R.string.sync_github_api_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.height(48.dp).padding(vertical = 8.dp),
            )
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_enable_sync),
                checked = enabledRow.enabled,
                onCheckedChange = { checked ->
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig { copy(enabled = checked) })
                },
            )
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_sync),
                checked = autoSyncRow.autoSync,
                onCheckedChange = { checked ->
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig { copy(autoSync = checked) })
                },
                enabled = enabledRow.enabled,
            )
        }
    }

    // ── 凭据组（凭据标题 + 远程仓库 + 分支 + Token）─
    item(key = "sync.credentials_group", contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP) {
        val remoteUrlRow by vm.syncRemoteUrlRow.collectAsStateWithLifecycle()
        val branchRow by vm.syncBranchRow.collectAsStateWithLifecycle()
        val tokenRow by vm.syncTokenRow.collectAsStateWithLifecycle()
        var remoteUrl by rememberSaveable { mutableStateOf(remoteUrlRow.remoteUrl) }
        LaunchedEffect(remoteUrlRow.remoteUrl) { remoteUrl = remoteUrlRow.remoteUrl }
        var branch by rememberSaveable { mutableStateOf(branchRow.branch) }
        LaunchedEffect(branchRow.branch) { branch = branchRow.branch }
        var token by rememberSaveable { mutableStateOf(tokenRow.token) }
        LaunchedEffect(tokenRow.token) { token = tokenRow.token }
        SettingsExpandedGroupContainer(
            closeOuterGroup = false,
            firstInGroup = false,
            lastInGroup = false,
        ) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_sync_credentials))
            SujianTextField(
                value = remoteUrl,
                onValueChange = {
                    remoteUrl = it
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig { copy(remoteUrl = it) })
                },
                label = { Text(stringResource(id = R.string.pref_github_repo)) },
                modifier = rememberFieldFocusModifier("sync_remote_url") { remoteUrl },
                enabled = remoteUrlRow.enabled,
            )
            SujianTextField(
                value = branch,
                onValueChange = {
                    branch = it
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig { copy(branch = it) })
                },
                label = { Text(stringResource(id = R.string.pref_branch)) },
                modifier = rememberFieldFocusModifier("sync_branch") { branch },
                enabled = branchRow.enabled,
            )
            SujianSecretTextField(
                value = token,
                onValueChange = {
                    token = it
                    vm.handleIntent(SettingsIntent.UpdateSyncSecrets { copy(token = it.ifBlank { null }) })
                },
                label = { Text(stringResource(id = R.string.pref_https_token)) },
                modifier = rememberFieldFocusModifier("sync_token") { token },
                enabled = tokenRow.enabled,
            )
        }
    }

    // ── 同步间隔组（间隔标题 + 间隔 Slider）─
    item(key = "sync.interval_group", contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP) {
        val intervalRow by vm.syncIntervalRow.collectAsStateWithLifecycle()
        var syncInterval by rememberSaveable { mutableFloatStateOf(intervalRow.intervalSeconds.toFloat()) }
        LaunchedEffect(intervalRow.intervalSeconds) { syncInterval = intervalRow.intervalSeconds.toFloat() }
        SettingsExpandedGroupContainer(
            closeOuterGroup = false,
            firstInGroup = false,
            lastInGroup = false,
        ) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_sync_interval))
            SujianSlider(
                title = stringResource(id = R.string.pref_sync_interval),
                value = syncInterval,
                onValueChange = { syncInterval = it },
                onValueChangeFinished = {
                    val seconds = syncInterval.toInt().coerceAtLeast(60)
                    vm.handleIntent(SettingsIntent.UpdateSyncConfig { copy(syncIntervalSeconds = seconds) })
                },
                valueRange = 60f..3600f,
                steps = 5,
                valueFormatter = { v ->
                    val minutes = (v / 60).toInt()
                    if (minutes >= 1) "${minutes}min" else "${v.toInt()}s"
                },
                enabled = intervalRow.enabled,
            )
        }
    }

    // ── 同步操作组（操作标题 + Dry run + Test + Perform + 结果）─
    item(key = "sync.actions_group", contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP) {
        val actionsRow by vm.syncActionsRow.collectAsStateWithLifecycle()
        val resultPair by vm.syncResultRow.collectAsStateWithLifecycle()
        val anySyncRunning = actionsRow.test == SyncCommandState.RUNNING || actionsRow.perform == SyncCommandState.RUNNING
        SettingsExpandedGroupContainer(
            closeOuterGroup = closeOuterGroup,
            firstInGroup = false,
            lastInGroup = true,
        ) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_sync_actions))
            if (actionsRow.enabled) {
                SujianOutlinedButton(
                    text = stringResource(id = R.string.btn_dry_run),
                    onClick = { vm.handleIntent(SettingsIntent.DryRun) },
                    modifier = Modifier.fillMaxWidth(),
                    loading = actionsRow.dryRun == SyncCommandState.RUNNING,
                    enabled = actionsRow.capability.canRun && !anySyncRunning,
                )
                SujianOutlinedButton(
                    text = stringResource(id = R.string.btn_test_connection),
                    onClick = { vm.handleIntent(SettingsIntent.TestConnection) },
                    modifier = Modifier.fillMaxWidth(),
                    loading = actionsRow.test == SyncCommandState.RUNNING,
                    enabled = actionsRow.capability.canRun && !anySyncRunning,
                )
                SujianOutlinedButton(
                    text = stringResource(id = R.string.btn_perform_sync),
                    onClick = { vm.handleIntent(SettingsIntent.PerformSync) },
                    modifier = Modifier.fillMaxWidth(),
                    loading = actionsRow.perform == SyncCommandState.RUNNING,
                    enabled = actionsRow.capability.canRun && !anySyncRunning,
                )
            }
            val structured = resultPair.first
            if (structured != null) {
                val isSuccess = structured.statusCode == "ok"
                val displayResult = resolveStructuredResult(structured)
                Text(
                    text = displayResult,
                    color =
                        if (isSuccess) {
                            MaterialTheme.colorScheme.primary
                        } else {
                            MaterialTheme.colorScheme.error
                        },
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }
}

// 以下辅助函数保持不变

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
        "sync_already_running" -> stringResource(id = R.string.sync_block_already_running)
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
