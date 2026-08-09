package com.xiwei.sujian.feature.settings.ui

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
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.core.designsystem.component.SujianSection
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.feature.editor.ui.AnimatedTextField
import com.xiwei.sujian.feature.editor.session.TextEditorProfile

/**
 * #600 评论 #5：设置页同步区域拆成两组完全独立的 UI：
 * - 作品同步：编辑 [SettingsUiState.projectSyncConfig]/[SettingsUiState.projectSyncSecrets]，
 *   发送 [SettingsIntent.DryRun]/[SettingsIntent.TestConnection]/[SettingsIntent.PerformSync]。
 * - 应用数据同步：编辑 [SettingsUiState.appSyncConfig]/[SettingsUiState.appSyncSecrets]，
 *   发送 [SettingsIntent.AppDryRun]/[SettingsIntent.AppTestConnection]/[SettingsIntent.AppPerformSync]。
 *
 * 两组控件不共用任何状态字段，rememberSaveable 局部变量也各自独立。
 */
@Composable
fun SyncSettings(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val dims = LocalSujianDimensions.current

    androidx.compose.foundation.layout.Column(
        modifier = modifier.padding(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space16),
    ) {
        ProjectSyncSection(state, onIntent)
        AppSyncSection(state, onIntent)
    }
}

// ── 作品同步 ──

@Composable
private fun ProjectSyncSection(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
) {
    val syncConfig = state.projectSyncConfig
    val syncSecrets = state.projectSyncSecrets
    val syncCapability = state.projectSyncCapability
    val dims = LocalSujianDimensions.current

    var remoteUrl by rememberSaveable { mutableStateOf(syncConfig.remoteUrl ?: "") }
    var branch by rememberSaveable { mutableStateOf(syncConfig.branch ?: "main") }
    var token by rememberSaveable { mutableStateOf(syncSecrets.token ?: "") }
    var syncInterval by rememberSaveable { mutableFloatStateOf((syncConfig.syncIntervalSeconds ?: 300).toFloat()) }

    LaunchedEffect(syncConfig.remoteUrl) { remoteUrl = syncConfig.remoteUrl ?: "" }
    LaunchedEffect(syncConfig.branch) { branch = syncConfig.branch ?: "main" }
    LaunchedEffect(syncSecrets.token) { token = syncSecrets.token ?: "" }
    LaunchedEffect(syncConfig.syncIntervalSeconds) { syncInterval = (syncConfig.syncIntervalSeconds ?: 300).toFloat() }

    // #595 四：同步 profile 读取失败（安全存储/原生库/配置损坏）→ 显示真实错误，
    // 字段保留上一次已确认值，不再静默退化为默认空 token。
    if (state.projectSyncProfileLoadState is SyncProfileLoadState.Failed) {
        val failed = state.projectSyncProfileLoadState
        SujianSection(title = stringResource(id = R.string.pref_category_sync)) {
            Text(
                text = resolveSyncFailureMessage(failed.kind.messageKey()),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
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
                onIntent(SettingsIntent.UpdateProjectSyncConfig(syncConfig.copy(enabled = checked)))
            },
        )
        Spacer(modifier = Modifier.height(dims.space8))
        SujianSwitchRow(
            title = stringResource(id = R.string.pref_auto_sync),
            checked = syncConfig.autoSync ?: false,
            onCheckedChange = { checked ->
                onIntent(SettingsIntent.UpdateProjectSyncConfig(syncConfig.copy(autoSync = checked)))
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
                onIntent(SettingsIntent.UpdateProjectSyncConfig(syncConfig.copy(remoteUrl = text)))
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
                onIntent(SettingsIntent.UpdateProjectSyncConfig(syncConfig.copy(branch = text)))
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
                onIntent(SettingsIntent.UpdateProjectSyncSecrets(syncSecrets.copy(token = text.ifBlank { null })))
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
                onIntent(SettingsIntent.UpdateProjectSyncConfig(syncConfig.copy(syncIntervalSeconds = seconds)))
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
        val anySyncRunning =
            state.projectDryRunState == SyncCommandState.RUNNING ||
                state.projectTestConnectionState == SyncCommandState.RUNNING ||
                state.projectPerformSyncState == SyncCommandState.RUNNING
        SujianSection(title = stringResource(id = R.string.pref_category_sync)) {
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_dry_run),
                onClick = { onIntent(SettingsIntent.DryRun) },
                modifier = Modifier.fillMaxWidth(),
                loading = state.projectDryRunState == SyncCommandState.RUNNING,
                enabled = syncCapability.canRun && !anySyncRunning,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_test_connection),
                onClick = { onIntent(SettingsIntent.TestConnection) },
                modifier = Modifier.fillMaxWidth(),
                loading = state.projectTestConnectionState == SyncCommandState.RUNNING,
                enabled = syncCapability.canRun && !anySyncRunning,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_perform_sync),
                onClick = { onIntent(SettingsIntent.PerformSync) },
                modifier = Modifier.fillMaxWidth(),
                loading = state.projectPerformSyncState == SyncCommandState.RUNNING,
                enabled = syncCapability.canRun && !anySyncRunning,
            )

            val structured = state.projectSyncResult
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

// ── 应用数据同步（#600 评论 #5） ──

@Composable
private fun AppSyncSection(
    state: SettingsUiState,
    onIntent: (SettingsIntent) -> Unit,
) {
    val syncConfig = state.appSyncConfig
    val syncSecrets = state.appSyncSecrets
    val dims = LocalSujianDimensions.current

    var remoteUrl by rememberSaveable { mutableStateOf(syncConfig.remoteUrl ?: "") }
    var branch by rememberSaveable { mutableStateOf(syncConfig.branch ?: "main") }
    var token by rememberSaveable { mutableStateOf(syncSecrets.token ?: "") }
    var syncInterval by rememberSaveable { mutableFloatStateOf((syncConfig.syncIntervalSeconds ?: 300).toFloat()) }

    LaunchedEffect(syncConfig.remoteUrl) { remoteUrl = syncConfig.remoteUrl ?: "" }
    LaunchedEffect(syncConfig.branch) { branch = syncConfig.branch ?: "main" }
    LaunchedEffect(syncSecrets.token) { token = syncSecrets.token ?: "" }
    LaunchedEffect(syncConfig.syncIntervalSeconds) { syncInterval = (syncConfig.syncIntervalSeconds ?: 300).toFloat() }

    // 应用级 profile 读取失败 → 显示真实错误。
    if (state.appSyncProfileLoadState is SyncProfileLoadState.Failed) {
        val failed = state.appSyncProfileLoadState
        SujianSection(title = stringResource(id = R.string.pref_category_app_sync)) {
            Text(
                text = resolveSyncFailureMessage(failed.kind.messageKey()),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }

    SujianSection(title = stringResource(id = R.string.pref_category_app_sync)) {
        SujianSwitchRow(
            title = stringResource(id = R.string.pref_enable_sync),
            checked = syncConfig.enabled ?: false,
            onCheckedChange = { checked ->
                onIntent(SettingsIntent.UpdateAppSyncConfig(syncConfig.copy(enabled = checked)))
            },
        )
        Spacer(modifier = Modifier.height(dims.space8))
        SujianSwitchRow(
            title = stringResource(id = R.string.pref_auto_sync),
            checked = syncConfig.autoSync ?: false,
            onCheckedChange = { checked ->
                onIntent(SettingsIntent.UpdateAppSyncConfig(syncConfig.copy(autoSync = checked)))
            },
            enabled = syncConfig.enabled ?: false,
        )
    }

    SujianSection(title = stringResource(id = R.string.pref_category_app_sync)) {
        AnimatedTextField(
            targetId = "app_sync_remote_url",
            value = remoteUrl,
            onValueChange = { remoteUrl = it },
            onCommit = { text ->
                remoteUrl = text
                onIntent(SettingsIntent.UpdateAppSyncConfig(syncConfig.copy(remoteUrl = text)))
            },
            profile = TextEditorProfile.RepositoryUrl,
            label = { Text(stringResource(id = R.string.pref_github_repo)) },
            enabled = syncConfig.enabled ?: false,
        )
        Spacer(modifier = Modifier.height(dims.space8))
        AnimatedTextField(
            targetId = "app_sync_branch",
            value = branch,
            onValueChange = { branch = it },
            onCommit = { text ->
                branch = text
                onIntent(SettingsIntent.UpdateAppSyncConfig(syncConfig.copy(branch = text)))
            },
            profile = TextEditorProfile.BranchName,
            label = { Text(stringResource(id = R.string.pref_branch)) },
            enabled = syncConfig.enabled ?: false,
        )
        Spacer(modifier = Modifier.height(dims.space8))
        AnimatedTextField(
            targetId = "app_sync_token",
            value = token,
            onValueChange = { token = it },
            onCommit = { text ->
                token = text
                onIntent(SettingsIntent.UpdateAppSyncSecrets(syncSecrets.copy(token = text.ifBlank { null })))
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
                onIntent(SettingsIntent.UpdateAppSyncConfig(syncConfig.copy(syncIntervalSeconds = seconds)))
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
        val anySyncRunning =
            state.appDryRunState == SyncCommandState.RUNNING ||
                state.appTestConnectionState == SyncCommandState.RUNNING ||
                state.appPerformSyncState == SyncCommandState.RUNNING
        // 应用级不需要 capability 检查：enabled + remoteUrl 非空即可操作。
        val canAct = (syncConfig.remoteUrl ?: "").isNotEmpty()
        SujianSection(title = stringResource(id = R.string.pref_category_app_sync)) {
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_dry_run),
                onClick = { onIntent(SettingsIntent.AppDryRun) },
                modifier = Modifier.fillMaxWidth(),
                loading = state.appDryRunState == SyncCommandState.RUNNING,
                enabled = canAct && !anySyncRunning,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_test_connection),
                onClick = { onIntent(SettingsIntent.AppTestConnection) },
                modifier = Modifier.fillMaxWidth(),
                loading = state.appTestConnectionState == SyncCommandState.RUNNING,
                enabled = canAct && !anySyncRunning,
            )
            Spacer(modifier = Modifier.height(dims.space8))
            SujianOutlinedButton(
                text = stringResource(id = R.string.btn_perform_sync),
                onClick = { onIntent(SettingsIntent.AppPerformSync) },
                modifier = Modifier.fillMaxWidth(),
                loading = state.appPerformSyncState == SyncCommandState.RUNNING,
                enabled = canAct && !anySyncRunning,
            )

            val structured = state.appSyncResult
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
