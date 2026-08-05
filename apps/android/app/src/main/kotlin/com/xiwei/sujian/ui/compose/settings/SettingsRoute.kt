package com.xiwei.sujian.ui.compose.settings

import android.os.Parcelable
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.BackNavigationBehavior
import androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldPredictiveBackHandler
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import com.xiwei.sujian.data.SyncStatusRepository
import com.xiwei.sujian.designsystem.icon.SujianIcons
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.SaveField
import com.xiwei.sujian.data.SaveFailure
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SettingsSaveResult
import com.xiwei.sujian.designsystem.component.SujianListItem
import com.xiwei.sujian.designsystem.layout.SujianListDetailScaffoldWithNavigator
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.runtime.LocalSujianAppDependencies
import com.xiwei.sujian.ui.compose.navigation.SettingsSection
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.parcelize.Parcelize
import com.xiwei.sujian.data.ExclusiveResult
import com.xiwei.sujian.data.SyncCoordinator
import com.xiwei.sujian.data.SyncOutcome
import com.xiwei.sujian.data.SyncSession
import com.xiwei.sujian.model.SyncTrigger

enum class SyncCommandState { IDLE, RUNNING, SUCCESS, FAILURE }

data class SettingsUiState(
    val settings: LocalSettings = LocalSettings(),
    val fontSize: Float = 16f,
    val syncConfig: com.xiwei.sujian.model.SyncConfig = com.xiwei.sujian.model.SyncConfig(),
    val syncSecrets: com.xiwei.sujian.model.SyncSecrets = com.xiwei.sujian.model.SyncSecrets(),
    val syncCapability: com.xiwei.sujian.model.SyncCapabilityData = com.xiwei.sujian.model.SyncCapabilityData(),
    val secureStorageWarning: String? = null,
    val builtinThemes: List<uniffi.writer_core.BuiltinThemeDto> = emptyList(),
    val paletteRecords: List<uniffi.writer_core.ThemePaletteRecordDto> = emptyList(),
    val aiAvailable: Boolean = false,
    val workspacePath: String = "",
    val versionInfo: String = "",
    val saveErrorResId: Int? = null,
    val dryRunState: SyncCommandState = SyncCommandState.IDLE,
    val testConnectionState: SyncCommandState = SyncCommandState.IDLE,
    val performSyncState: SyncCommandState = SyncCommandState.IDLE,
    val structuredSyncResult: StructuredSyncResult? = null,
    val lastCommandType: SyncCommandType? = null,
)

sealed interface SettingsIntent {
    data class UpdateLocal(val transform: (LocalSettings) -> LocalSettings) : SettingsIntent
    data class UpdateFontSize(val fontSize: Float) : SettingsIntent
    data class UpdateSyncConfig(val config: com.xiwei.sujian.model.SyncConfig) : SettingsIntent
    data class UpdateSyncSecrets(val secrets: com.xiwei.sujian.model.SyncSecrets) : SettingsIntent
    data object Refresh : SettingsIntent
    data object CaptureDynamicColor : SettingsIntent
    data class DeletePalette(val deviceId: String, val fingerprint: String) : SettingsIntent
    data object DryRun : SettingsIntent
    data object TestConnection : SettingsIntent
    data object PerformSync : SettingsIntent
}

sealed interface SettingsSaveCommand {
    data class Local(
        val settings: LocalSettings,
        val revision: Long,
    ) : SettingsSaveCommand

    data class FontSize(
        val fontSize: Float,
        val revision: Long,
    ) : SettingsSaveCommand

    data class SyncConfig(
        val config: com.xiwei.sujian.model.SyncConfig,
        val revision: Long,
    ) : SettingsSaveCommand

    data class SyncSecrets(
        val secrets: com.xiwei.sujian.model.SyncSecrets,
        val revision: Long,
    ) : SettingsSaveCommand
}

sealed interface SettingsTransactionCommand {
    data class SaveAndRunSync(
        val config: com.xiwei.sujian.model.SyncConfig,
        val configRevision: Long,
        val secrets: com.xiwei.sujian.model.SyncSecrets,
        val secretsRevision: Long,
        val trigger: SyncTrigger,
    ) : SettingsTransactionCommand

    data class SaveAndRunDryRun(
        val config: com.xiwei.sujian.model.SyncConfig,
        val configRevision: Long,
        val secrets: com.xiwei.sujian.model.SyncSecrets,
        val secretsRevision: Long,
    ) : SettingsTransactionCommand

    data class SaveAndRunDiagnostics(
        val config: com.xiwei.sujian.model.SyncConfig,
        val configRevision: Long,
        val secrets: com.xiwei.sujian.model.SyncSecrets,
        val secretsRevision: Long,
    ) : SettingsTransactionCommand
}

private data class PendingCommands(
    val local: SettingsSaveCommand.Local? = null,
    val fontSize: SettingsSaveCommand.FontSize? = null,
    val syncConfig: SettingsSaveCommand.SyncConfig? = null,
    val syncSecrets: SettingsSaveCommand.SyncSecrets? = null,
)

enum class SyncCommandType { DRY_RUN, TEST_CONNECTION, PERFORM_SYNC }

private data class SyncCommandIoResult(
    val configSaved: Boolean,
    val secretsSaved: Boolean,
    val structuredResult: StructuredSyncResult,
) {
    val isSuccess: Boolean get() = structuredResult.statusCode == "ok"
}

class SettingsViewModel(
    private val settingsRepo: SettingsRepository,
    private val syncCoordinator: com.xiwei.sujian.data.SyncCoordinator,
) : ViewModel() {
    private val _uiState = MutableStateFlow(SettingsUiState())
    val uiState: StateFlow<SettingsUiState> = _uiState.asStateFlow()

    private sealed interface QueueItem {
        data class Save(val command: SettingsSaveCommand) : QueueItem
        data class Transaction(val command: SettingsTransactionCommand) : QueueItem
    }
    private val saveChannel = Channel<QueueItem>(Channel.UNLIMITED)
    private val _saveFailureEvents = Channel<Int>(Channel.BUFFERED)
    val saveFailureEvents = _saveFailureEvents.receiveAsFlow()

    private var localRevision = 0L
    private var fontSizeRevision = 0L
    private var syncConfigRevision = 0L
    private var syncSecretsRevision = 0L

    private var localPersistedRevision = 0L
    private var fontSizePersistedRevision = 0L
    private var syncConfigPersistedRevision = 0L
    private var syncSecretsPersistedRevision = 0L

    private var pendingCommands = PendingCommands()

    private fun hasUnsavedLocal() = localRevision != localPersistedRevision
    private fun hasUnsavedFontSize() = fontSizeRevision != fontSizePersistedRevision
    private fun hasUnsavedSyncConfig() = syncConfigRevision != syncConfigPersistedRevision
    private fun hasUnsavedSyncSecrets() = syncSecretsRevision != syncSecretsPersistedRevision

    init {
        loadInitial()
        viewModelScope.launch {
            for (item in saveChannel) {
                when (item) {
                    is QueueItem.Save -> {
                        mergeCommand(item.command)
                        while (true) {
                            val next = saveChannel.tryReceive().getOrNull()
                            if (next is QueueItem.Save) {
                                mergeCommand(next.command)
                            } else {
                                if (next != null) {
                                    // Put the non-save item back by re-sending; Channel is UNLIMITED so trySend always succeeds
                                    saveChannel.trySend(next)
                                }
                                break
                            }
                        }
                        flushPending()
                    }
                    is QueueItem.Transaction -> {
                        // Transaction is a barrier: flush any pending saves first, then execute the transaction
                        flushPending()
                        executeTransaction(item.command)
                    }
                }
            }
        }
    }

    private fun mergeCommand(command: SettingsSaveCommand) {
        pendingCommands = when (command) {
            is SettingsSaveCommand.Local -> pendingCommands.copy(local = command)
            is SettingsSaveCommand.FontSize -> pendingCommands.copy(fontSize = command)
            is SettingsSaveCommand.SyncConfig -> pendingCommands.copy(syncConfig = command)
            is SettingsSaveCommand.SyncSecrets -> pendingCommands.copy(syncSecrets = command)
        }
    }

    class Factory(
        private val repo: SettingsRepository,
        private val coordinator: com.xiwei.sujian.data.SyncCoordinator,
    ) : androidx.lifecycle.ViewModelProvider.Factory {
        override fun <T : ViewModel> create(modelClass: Class<T>): T {
            return modelClass.cast(SettingsViewModel(repo, coordinator)) as T
        }
    }

    private fun loadInitial() {
        val repo = settingsRepo
        viewModelScope.launch {
            val snapshotLocalRev = localRevision
            val snapshotFontSizeRev = fontSizeRevision
            val snapshotSyncConfigRev = syncConfigRevision
            val snapshotSyncSecretsRev = syncSecretsRevision

            val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
            val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
            val syncConfig = withContext(Dispatchers.IO) { repo.loadSyncConfig() }
            val syncSecrets = withContext(Dispatchers.IO) { repo.loadSyncSecrets() }
            val syncCapability = withContext(Dispatchers.IO) { repo.getSyncCapability() }
            val secureStorageWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
            val builtinThemes = withContext(Dispatchers.IO) { repo.listBuiltinThemes() }
            val paletteRecords = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
            val aiAvailable = withContext(Dispatchers.IO) { repo.aiAvailable() }
            val workspacePath = withContext(Dispatchers.IO) { repo.workspaceDir() }

            _uiState.update { current ->
                SettingsUiState(
                    settings = if (localRevision == snapshotLocalRev) settings else current.settings,
                    fontSize = if (fontSizeRevision == snapshotFontSizeRev) fontSize else current.fontSize,
                    syncConfig = if (syncConfigRevision == snapshotSyncConfigRev) syncConfig else current.syncConfig,
                    syncSecrets = if (syncSecretsRevision == snapshotSyncSecretsRev) syncSecrets else current.syncSecrets,
                    syncCapability = syncCapability,
                    secureStorageWarning = secureStorageWarning,
                    builtinThemes = builtinThemes,
                    paletteRecords = paletteRecords,
                    aiAvailable = aiAvailable,
                    workspacePath = workspacePath,
                )
            }
        }
    }

    private suspend fun flushPending() {
        val repo = settingsRepo
        val cmds = pendingCommands
        if (cmds.local == null && cmds.fontSize == null && cmds.syncConfig == null && cmds.syncSecrets == null) return
        pendingCommands = PendingCommands()
        executeSave(repo, cmds.local, cmds.fontSize, cmds.syncConfig, cmds.syncSecrets)
    }

    private suspend fun executeSave(
        repo: SettingsRepository,
        local: SettingsSaveCommand.Local?,
        fontSize: SettingsSaveCommand.FontSize?,
        syncConfig: SettingsSaveCommand.SyncConfig?,
        syncSecrets: SettingsSaveCommand.SyncSecrets?,
    ) {
        val failures = mutableListOf<SaveFailure>()

        if (local != null) {
            val result = withContext(Dispatchers.IO) { repo.saveLocalSettings(local.settings) }
            when (result) {
                is SettingsSaveResult.Success -> {
                    localPersistedRevision = local.revision
                    com.xiwei.sujian.ui.compose.theme.ThemeStore.reload()
                }
                is SettingsSaveResult.Failed -> {
                    if (localRevision == local.revision) {
                        failures.add(SaveFailure(SaveField.LOCAL_SETTINGS, local.revision))
                    }
                }
            }
        }

        if (fontSize != null) {
            val result = withContext(Dispatchers.IO) { repo.setFontSize(fontSize.fontSize) }
            when (result) {
                is SettingsSaveResult.Success -> {
                    fontSizePersistedRevision = fontSize.revision
                }
                is SettingsSaveResult.Failed -> {
                    if (fontSizeRevision == fontSize.revision) {
                        failures.add(SaveFailure(SaveField.FONT_SIZE, fontSize.revision))
                    }
                }
            }
        }

        var syncConfigSaved = false
        var syncSecretsSaved = false

        if (syncConfig != null) {
            val result = withContext(Dispatchers.IO) { repo.saveSyncConfig(syncConfig.config) }
            when (result) {
                is SettingsSaveResult.Success -> {
                    syncConfigPersistedRevision = syncConfig.revision
                    syncConfigSaved = true
                }
                is SettingsSaveResult.Failed -> {
                    if (syncConfigRevision == syncConfig.revision) {
                        failures.add(SaveFailure(SaveField.SYNC_CONFIG, syncConfig.revision))
                    }
                }
            }
        }

        if (syncSecrets != null) {
            val result = withContext(Dispatchers.IO) { repo.saveSyncSecrets(syncSecrets.secrets) }
            when (result) {
                is SettingsSaveResult.Success -> {
                    syncSecretsPersistedRevision = syncSecrets.revision
                    syncSecretsSaved = true
                }
                is SettingsSaveResult.Failed -> {
                    if (syncSecretsRevision == syncSecrets.revision) {
                        failures.add(SaveFailure(SaveField.SYNC_SECRETS, syncSecrets.revision))
                    }
                }
            }
        }

        if (syncConfigSaved || syncSecretsSaved) {
            val refreshedCapability = withContext(Dispatchers.IO) { repo.getSyncCapability() }
            val refreshedWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
            _uiState.update { it.copy(syncCapability = refreshedCapability, secureStorageWarning = refreshedWarning) }
        }

        if (failures.isNotEmpty()) {
            rollbackFailures(repo, failures)
            val errorResId = when (failures.first().field) {
                SaveField.LOCAL_SETTINGS -> R.string.save_local_settings_failed
                SaveField.FONT_SIZE -> R.string.save_font_size_failed
                SaveField.SYNC_CONFIG -> R.string.save_sync_config_failed
                SaveField.SYNC_SECRETS -> R.string.save_sync_secrets_failed
            }
            _uiState.update { it.copy(saveErrorResId = errorResId) }
            _saveFailureEvents.send(errorResId)
        } else if (local != null || fontSize != null || syncConfig != null || syncSecrets != null) {
            _uiState.update { it.copy(saveErrorResId = null) }
        }
    }

    private suspend fun executeTransaction(command: SettingsTransactionCommand) {
        when (command) {
            is SettingsTransactionCommand.SaveAndRunSync -> executeSyncTransaction(command)
            is SettingsTransactionCommand.SaveAndRunDryRun -> executeDryRunTransaction(command)
            is SettingsTransactionCommand.SaveAndRunDiagnostics -> executeDiagnosticsTransaction(command)
        }
    }

    private suspend fun saveTransactionConfigAndSecrets(
        config: com.xiwei.sujian.model.SyncConfig,
        configRevision: Long,
        secrets: com.xiwei.sujian.model.SyncSecrets,
        secretsRevision: Long,
    ): Boolean {
        if (syncConfigRevision == configRevision) {
            val configSaveResult = withContext(Dispatchers.IO) { settingsRepo.saveSyncConfig(config) }
            if (configSaveResult is SettingsSaveResult.Failed) return false
            syncConfigPersistedRevision = configRevision
        }
        if (syncSecretsRevision == secretsRevision) {
            val secretsSaveResult = withContext(Dispatchers.IO) { settingsRepo.saveSyncSecrets(secrets) }
            if (secretsSaveResult is SettingsSaveResult.Failed) {
                if (syncConfigRevision == configRevision) {
                    syncConfigPersistedRevision = configRevision
                }
                return false
            }
            syncSecretsPersistedRevision = secretsRevision
        }
        return true
    }

    private suspend fun executeSyncTransaction(
        command: SettingsTransactionCommand.SaveAndRunSync,
    ) {
        _uiState.update { it.copy(performSyncState = SyncCommandState.RUNNING, structuredSyncResult = null, lastCommandType = SyncCommandType.PERFORM_SYNC) }
        try {
            if (!saveTransactionConfigAndSecrets(command.config, command.configRevision, command.secrets, command.secretsRevision)) {
                _uiState.update { it.copy(performSyncState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "error", messageKey = "save_config_or_secrets_failed")) }
                return
            }
            val syncOutcome = syncCoordinator.runSync(command.trigger)
            val ioResult = when (syncOutcome) {
                is SyncOutcome.Completed -> {
                    val sr = syncOutcome.result
                    val counts = SyncCounts(
                        uploaded = sr.uploadedFiles.size,
                        downloaded = sr.downloadedFiles.size,
                        deletedRemote = sr.remoteDeletes.size,
                        deletedLocal = sr.localDeletes.size,
                        conflicts = sr.conflicts.size,
                        overwritten = sr.overwrittenFiles.size,
                        ignored = sr.ignoredFiles.size
                    )
                    SyncCommandIoResult(true, true, StructuredSyncResult(
                        statusCode = if (sr.error == null) "ok" else "error",
                        messageKey = "sync_perform_result",
                        counts = counts,
                        sanitizedDiagnostic = if (sr.error != null) "sync_failed" else null
                    ))
                }
                is SyncOutcome.Unconfigured ->
                    SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "sync_unconfigured"))
                is SyncOutcome.Disabled ->
                    SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "sync_disabled"))
                is SyncOutcome.Busy ->
                    SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "sync_busy"))
                is SyncOutcome.RetryableFailure ->
                    SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "sync_retryable_failure"))
                is SyncOutcome.TerminalFailure ->
                    SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "sync_terminal_failure"))
                else ->
                    SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "sync_unknown"))
            }
            _uiState.update { current ->
                if (ioResult.isSuccess) {
                    current.copy(performSyncState = SyncCommandState.SUCCESS, structuredSyncResult = ioResult.structuredResult, lastCommandType = SyncCommandType.PERFORM_SYNC)
                } else {
                    current.copy(performSyncState = SyncCommandState.FAILURE, structuredSyncResult = ioResult.structuredResult, lastCommandType = SyncCommandType.PERFORM_SYNC)
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _uiState.update { it.copy(performSyncState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "error", messageKey = "unexpected_error", sanitizedDiagnostic = e.message), lastCommandType = SyncCommandType.PERFORM_SYNC) }
        }
        try {
            val refreshedCapability = withContext(Dispatchers.IO) { settingsRepo.getSyncCapability() }
            val refreshedWarning = withContext(Dispatchers.IO) { settingsRepo.getSecureStorageWarning() }
            _uiState.update { it.copy(syncCapability = refreshedCapability, secureStorageWarning = refreshedWarning) }
        } catch (_: Exception) { }
    }

    private suspend fun executeDryRunTransaction(
        command: SettingsTransactionCommand.SaveAndRunDryRun,
    ) {
        _uiState.update { it.copy(dryRunState = SyncCommandState.RUNNING, structuredSyncResult = null, lastCommandType = SyncCommandType.DRY_RUN) }
        try {
            if (!saveTransactionConfigAndSecrets(command.config, command.configRevision, command.secrets, command.secretsRevision)) {
                _uiState.update { it.copy(dryRunState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "error", messageKey = "save_config_or_secrets_failed")) }
                return
            }
            val exclusiveResult = SyncSession.runExclusive { _ ->
                withContext(Dispatchers.IO) {
                    val capability = settingsRepo.getSyncCapability()
                    if (!capability.canRun) {
                        return@withContext SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "blocked", messageKey = capability.blockMessageKey ?: "sync_not_ready"))
                    }
                    when (val r = settingsRepo.performSyncDryRun(command.config)) {
                        is BridgeResult.Success -> {
                            val plan = r.data
                            val counts = SyncCounts(
                                uploaded = plan.filesToUpload.size,
                                downloaded = plan.filesToDownload.size,
                                deletedRemote = plan.filesToDeleteRemote.size,
                                deletedLocal = plan.filesToDeleteLocal.size,
                                conflicts = plan.conflicts.size,
                                ignored = plan.ignoredFiles.size
                            )
                            SyncCommandIoResult(true, true, StructuredSyncResult(
                                statusCode = "ok",
                                messageKey = "sync_dry_run_result",
                                counts = counts
                            ))
                        }
                        is BridgeResult.Error -> SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "dry_run_error", sanitizedDiagnostic = r.message))
                        BridgeResult.NotLoaded -> SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "core_not_loaded"))
                    }
                }
            }
            when (exclusiveResult) {
                is ExclusiveResult.Busy -> {
                    _uiState.update { it.copy(dryRunState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "busy", messageKey = "sync_already_running"), lastCommandType = SyncCommandType.DRY_RUN) }
                }
                is ExclusiveResult.Success -> {
                    val ioResult = exclusiveResult.value
                    _uiState.update { current ->
                        if (ioResult.isSuccess) {
                            current.copy(dryRunState = SyncCommandState.SUCCESS, structuredSyncResult = ioResult.structuredResult)
                        } else {
                            current.copy(dryRunState = SyncCommandState.FAILURE, structuredSyncResult = ioResult.structuredResult)
                        }
                    }
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _uiState.update { it.copy(dryRunState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "error", messageKey = "unexpected_error", sanitizedDiagnostic = e.message)) }
        }
        try {
            val refreshedCapability = withContext(Dispatchers.IO) { settingsRepo.getSyncCapability() }
            val refreshedWarning = withContext(Dispatchers.IO) { settingsRepo.getSecureStorageWarning() }
            _uiState.update { it.copy(syncCapability = refreshedCapability, secureStorageWarning = refreshedWarning) }
        } catch (_: Exception) { }
    }

    private suspend fun executeDiagnosticsTransaction(
        command: SettingsTransactionCommand.SaveAndRunDiagnostics,
    ) {
        _uiState.update { it.copy(testConnectionState = SyncCommandState.RUNNING, structuredSyncResult = null, lastCommandType = SyncCommandType.TEST_CONNECTION) }
        try {
            if (!saveTransactionConfigAndSecrets(command.config, command.configRevision, command.secrets, command.secretsRevision)) {
                _uiState.update { it.copy(testConnectionState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "error", messageKey = "save_config_or_secrets_failed")) }
                return
            }
            val exclusiveResult = SyncSession.runExclusive { _ ->
                withContext(Dispatchers.IO) {
                    val capability = settingsRepo.getSyncCapability()
                    if (!capability.canRun) {
                        return@withContext SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "blocked", messageKey = capability.blockMessageKey ?: "sync_not_ready"))
                    }
                    when (val r = settingsRepo.performSyncDiagnostics(command.config)) {
                        is BridgeResult.Success -> {
                            val diag = r.data
                            SyncCommandIoResult(true, true, StructuredSyncResult(
                                statusCode = if (diag.success) "ok" else "fail",
                                messageKey = "sync_test_connection_result",
                                messageArgs = mapOf(
                                    "network" to if (diag.networkOk) "ok" else "fail",
                                    "auth" to if (diag.authOk) "ok" else "fail",
                                    "repo" to if (diag.repoOk) "ok" else "fail",
                                    "branch" to if (diag.branchOk) "ok" else "fail"
                                ),
                                sanitizedDiagnostic = if (!diag.success) "connection_failed" else null
                            ))
                        }
                        is BridgeResult.Error -> SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "diagnostics_error", sanitizedDiagnostic = r.message))
                        BridgeResult.NotLoaded -> SyncCommandIoResult(true, true, StructuredSyncResult(statusCode = "error", messageKey = "core_not_loaded"))
                    }
                }
            }
            when (exclusiveResult) {
                is ExclusiveResult.Busy -> {
                    _uiState.update { it.copy(testConnectionState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "busy", messageKey = "sync_already_running"), lastCommandType = SyncCommandType.TEST_CONNECTION) }
                }
                is ExclusiveResult.Success -> {
                    val ioResult = exclusiveResult.value
                    _uiState.update { current ->
                        if (ioResult.isSuccess) {
                            current.copy(testConnectionState = SyncCommandState.SUCCESS, structuredSyncResult = ioResult.structuredResult)
                        } else {
                            current.copy(testConnectionState = SyncCommandState.FAILURE, structuredSyncResult = ioResult.structuredResult)
                        }
                    }
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _uiState.update { it.copy(testConnectionState = SyncCommandState.FAILURE, structuredSyncResult = StructuredSyncResult(statusCode = "error", messageKey = "unexpected_error", sanitizedDiagnostic = e.message)) }
        }
        try {
            val refreshedCapability = withContext(Dispatchers.IO) { settingsRepo.getSyncCapability() }
            val refreshedWarning = withContext(Dispatchers.IO) { settingsRepo.getSecureStorageWarning() }
            _uiState.update { it.copy(syncCapability = refreshedCapability, secureStorageWarning = refreshedWarning) }
        } catch (_: Exception) { }
    }

    private suspend fun rollbackFailures(repo: SettingsRepository, failures: List<SaveFailure>) {
        for (failure in failures) {
            rollbackIfRevisionMatches(repo, failure)
        }
    }

    private suspend fun rollbackIfRevisionMatches(repo: SettingsRepository, failure: SaveFailure) {
        when (failure.field) {
            SaveField.LOCAL_SETTINGS -> {
                if (localRevision != failure.revision) return
                val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
                if (localRevision != failure.revision) return
                localRevision = localPersistedRevision
                _uiState.update { it.copy(settings = settings) }
            }
            SaveField.FONT_SIZE -> {
                if (fontSizeRevision != failure.revision) return
                val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
                if (fontSizeRevision != failure.revision) return
                fontSizeRevision = fontSizePersistedRevision
                _uiState.update { it.copy(fontSize = fontSize) }
            }
            SaveField.SYNC_CONFIG -> {
                if (syncConfigRevision != failure.revision) return
                val config = withContext(Dispatchers.IO) { repo.loadSyncConfig() }
                if (syncConfigRevision != failure.revision) return
                syncConfigRevision = syncConfigPersistedRevision
                _uiState.update { it.copy(syncConfig = config) }
            }
            SaveField.SYNC_SECRETS -> {
                if (syncSecretsRevision != failure.revision) return
                val secrets = withContext(Dispatchers.IO) { repo.loadSyncSecrets() }
                if (syncSecretsRevision != failure.revision) return
                syncSecretsRevision = syncSecretsPersistedRevision
                _uiState.update { it.copy(syncSecrets = secrets) }
            }
        }
    }

    fun consumeSaveError() {
        _uiState.update { it.copy(saveErrorResId = null) }
    }

    fun handleIntent(intent: SettingsIntent) {
        when (intent) {
            is SettingsIntent.UpdateLocal -> {
                val newSettings = intent.transform(_uiState.value.settings)
                _uiState.update { it.copy(settings = newSettings) }
                val rev = ++localRevision
                saveChannel.trySend(QueueItem.Save(SettingsSaveCommand.Local(newSettings, rev)))
            }
            is SettingsIntent.UpdateFontSize -> {
                _uiState.update { it.copy(fontSize = intent.fontSize) }
                val rev = ++fontSizeRevision
                saveChannel.trySend(QueueItem.Save(SettingsSaveCommand.FontSize(intent.fontSize, rev)))
            }
            is SettingsIntent.UpdateSyncConfig -> {
                _uiState.update { it.copy(syncConfig = intent.config) }
                val rev = ++syncConfigRevision
                saveChannel.trySend(QueueItem.Save(SettingsSaveCommand.SyncConfig(intent.config, rev)))
            }
            is SettingsIntent.UpdateSyncSecrets -> {
                _uiState.update { it.copy(syncSecrets = intent.secrets) }
                val rev = ++syncSecretsRevision
                saveChannel.trySend(QueueItem.Save(SettingsSaveCommand.SyncSecrets(intent.secrets, rev)))
            }
            is SettingsIntent.Refresh -> mergeRefresh()
            is SettingsIntent.CaptureDynamicColor -> {
                val repo = settingsRepo
                viewModelScope.launch {
                    val records = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
                    _uiState.update { it.copy(paletteRecords = records) }
                }
            }
            is SettingsIntent.DeletePalette -> {
                val repo = settingsRepo
                viewModelScope.launch {
                    withContext(Dispatchers.IO) { repo.deletePaletteRecord(intent.deviceId, intent.fingerprint) }
                    val records = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
                    _uiState.update { it.copy(paletteRecords = records) }
                }
            }
            is SettingsIntent.DryRun -> {
                val currentConfig = _uiState.value.syncConfig
                val currentSecrets = _uiState.value.syncSecrets
                saveChannel.trySend(QueueItem.Transaction(SettingsTransactionCommand.SaveAndRunDryRun(
                    config = currentConfig,
                    configRevision = syncConfigRevision,
                    secrets = currentSecrets,
                    secretsRevision = syncSecretsRevision,
                )))
            }
            is SettingsIntent.TestConnection -> {
                val currentConfig = _uiState.value.syncConfig
                val currentSecrets = _uiState.value.syncSecrets
                saveChannel.trySend(QueueItem.Transaction(SettingsTransactionCommand.SaveAndRunDiagnostics(
                    config = currentConfig,
                    configRevision = syncConfigRevision,
                    secrets = currentSecrets,
                    secretsRevision = syncSecretsRevision,
                )))
            }
            is SettingsIntent.PerformSync -> {
                val currentConfig = _uiState.value.syncConfig
                val currentSecrets = _uiState.value.syncSecrets
                saveChannel.trySend(QueueItem.Transaction(SettingsTransactionCommand.SaveAndRunSync(
                    config = currentConfig,
                    configRevision = syncConfigRevision,
                    secrets = currentSecrets,
                    secretsRevision = syncSecretsRevision,
                    trigger = SyncTrigger.SettingsPage,
                )))
            }
        }
    }

    private fun mergeRefresh() {
        val repo = settingsRepo
        viewModelScope.launch {
            val current = _uiState.value
            val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
            val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
            val syncConfig = withContext(Dispatchers.IO) { repo.loadSyncConfig() }
            val syncSecrets = withContext(Dispatchers.IO) { repo.loadSyncSecrets() }
            val syncCapability = withContext(Dispatchers.IO) { repo.getSyncCapability() }
            val secureStorageWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
            val builtinThemes = withContext(Dispatchers.IO) { repo.listBuiltinThemes() }
            val paletteRecords = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
            val aiAvailable = withContext(Dispatchers.IO) { repo.aiAvailable() }
            val workspacePath = withContext(Dispatchers.IO) { repo.workspaceDir() }
            _uiState.update {
                SettingsUiState(
                    settings = if (!hasUnsavedLocal()) settings else current.settings,
                    fontSize = if (!hasUnsavedFontSize()) fontSize else current.fontSize,
                    syncConfig = if (!hasUnsavedSyncConfig()) syncConfig else current.syncConfig,
                    syncSecrets = if (!hasUnsavedSyncSecrets()) syncSecrets else current.syncSecrets,
                    syncCapability = syncCapability,
                    secureStorageWarning = secureStorageWarning,
                    builtinThemes = builtinThemes,
                    paletteRecords = paletteRecords,
                    aiAvailable = aiAvailable,
                    workspacePath = workspacePath,
                    dryRunState = current.dryRunState,
                    testConnectionState = current.testConnectionState,
                    performSyncState = current.performSyncState,
                    structuredSyncResult = current.structuredSyncResult,
                    lastCommandType = current.lastCommandType,
                )
            }
        }
    }
}

/**
 * 设置列表分组（手机列表按组呈现，组内每一项显示标题、说明或当前值与尾箭头）。
 */
enum class SettingsGroup(val titleResId: Int) {
    Appearance(R.string.pref_group_appearance),
    Writing(R.string.pref_group_writing),
    DataSync(R.string.pref_group_data_sync),
    Advanced(R.string.pref_group_advanced),
    About(R.string.pref_group_about),
}

data class SettingsCategory(
    val section: SettingsSection,
    val titleResId: Int,
    val icon: ImageVector,
    val group: SettingsGroup,
)

val settingsCategories = listOf(
    SettingsCategory(SettingsSection.Appearance, R.string.pref_category_appearance, SujianIcons.Palette, SettingsGroup.Appearance),
    SettingsCategory(SettingsSection.Editor, R.string.pref_category_editor, SujianIcons.Edit, SettingsGroup.Writing),
    SettingsCategory(SettingsSection.Save, R.string.pref_category_save, SujianIcons.Save, SettingsGroup.Writing),
    SettingsCategory(SettingsSection.Sync, R.string.pref_category_sync, SujianIcons.CloudSync, SettingsGroup.DataSync),
    SettingsCategory(SettingsSection.Ai, R.string.pref_category_ai, SujianIcons.AutoStories, SettingsGroup.Advanced),
    SettingsCategory(SettingsSection.Diagnostics, R.string.pref_category_diagnostics, SujianIcons.BugReport, SettingsGroup.Advanced),
    SettingsCategory(SettingsSection.Laboratory, R.string.pref_category_laboratory, SujianIcons.Science, SettingsGroup.Advanced),
    SettingsCategory(SettingsSection.About, R.string.pref_category_about, SujianIcons.Info, SettingsGroup.About),
)

@Parcelize
private data class SettingsSelection(val section: SettingsSection) : Parcelable

/**
 * 设置一级 destination 的唯一内容。
 *
 * 设置列表与设置详情共享同一个壳（Material3 Adaptive 列表—详情）：
 * - 手机：列表与详情在同一壳内切换，详情返回先回列表，再离开设置一级入口；
 * - 平板/大屏：左侧分类、右侧详情并排；
 * - 窗口尺寸变化只改变窗格呈现方式，不建立第二套设置状态。
 *
 * [detailSection] / [onDetailSectionChange] 由根壳提升持有，供一级 TopAppBar
 * 显示当前分类标题与返回按钮；详情→列表的可预见返回由
 * [ThreePaneScaffoldPredictiveBackHandler] 处理；列表→离开设置一级入口的返回
 * 由全局 NavDisplay 统一驱动（Works 常驻栈底，pop 带真实手势进度）。
 */
@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun SettingsRoute(
    detailSection: SettingsSection? = null,
    onDetailSectionChange: ((SettingsSection?) -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val deps = LocalSujianAppDependencies.current
    val vm: SettingsViewModel = viewModel(factory = SettingsViewModel.Factory(deps.settingsRepository, deps.syncCoordinator))
    val uiState by vm.uiState.collectAsState()
    val snackbarHostState = remember { androidx.compose.material3.SnackbarHostState() }

    @Suppress("LocalContextGetResourceValueCall")
    LaunchedEffect(Unit) {
        vm.saveFailureEvents.collect { errorResId ->
            snackbarHostState.showSnackbar(context.getString(errorResId))
        }
    }

    // 列表—详情窗格与返回/可预见返回必须共享同一个 navigator 实例：
    // 窗格由 navigator.currentDestination 驱动，返回层级（详情→列表→离开设置）
    // 与根壳状态同步都依赖同一份导航历史。
    val navigator = rememberListDetailPaneScaffoldNavigator<SettingsSelection>()
    ThreePaneScaffoldPredictiveBackHandler(
        navigator = navigator,
        backBehavior = BackNavigationBehavior.PopUntilScaffoldValueChange,
    )

    // 详情状态同步：navigator 内部变化（如预测性返回完成）回写根壳状态。
    // 首次组合跳过（初始 contentKey 恒为 null，回写 null 会覆盖平板进入时根壳
    // 注入的默认 Appearance，导致右侧详情空白）；后续变化才回写。
    var syncedOnce by remember { mutableStateOf(false) }
    LaunchedEffect(navigator.currentDestination?.contentKey) {
        if (!syncedOnce) {
            syncedOnce = true
            return@LaunchedEffect
        }
        onDetailSectionChange?.invoke(navigator.currentDestination?.contentKey?.section)
    }

    // 根壳状态变化（分类点击 / 顶栏返回）驱动 navigator 窗格。
    LaunchedEffect(detailSection) {
        if (detailSection == null) {
            if (navigator.canNavigateBack()) {
                navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
            }
        } else {
            val key = SettingsSelection(detailSection)
            if (navigator.currentDestination?.contentKey != key) {
                navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, key)
            }
        }
    }

    val coroutineScope = rememberCoroutineScope()

    // 返回层级：详情 → 列表由列表-详情 navigator 处理；
    // 列表 → 离开设置一级入口交给全局 NavDisplay（Works 常驻栈底，
    // predictivePopTransitionSpec 带真实手势进度），不再在本壳内拦截。
    BackHandler(enabled = navigator.canNavigateBack()) {
        coroutineScope.launch {
            navigator.navigateBack(BackNavigationBehavior.PopUntilScaffoldValueChange)
        }
    }

    Box(modifier = modifier.fillMaxSize()) {
        SujianListDetailScaffoldWithNavigator(
            navigator = navigator,
            modifier = Modifier.fillMaxSize(),
            listPane = {
                SettingsListPane(
                    onNavigateToDetail = { section ->
                        onDetailSectionChange?.invoke(section)
                    },
                    selectedSection = navigator.currentDestination?.contentKey?.section ?: detailSection,
                    state = uiState,
                )
            },
            detailPane = {
                val selection = navigator.currentDestination?.contentKey
                if (selection != null) {
                    SettingsDetailPane(
                        section = selection.section,
                        state = uiState,
                        onIntent = vm::handleIntent,
                    )
                }
            },
        )
        androidx.compose.material3.SnackbarHost(
            hostState = snackbarHostState,
            modifier = Modifier.align(Alignment.BottomCenter),
        )
    }
}

/**
 * 设置列表：按“外观 / 写作 / 数据与同步 / 高级 / 关于”分组，每一项显示标题、
 * 说明或当前值，尾部带进入详情的箭头；与详情共享根设置壳与 TopAppBar。
 */
@Composable
fun SettingsListPane(
    onNavigateToDetail: (SettingsSection) -> Unit,
    modifier: Modifier = Modifier,
    selectedSection: SettingsSection? = null,
    state: SettingsUiState = SettingsUiState(),
) {
    val dims = LocalSujianDimensions.current
    LazyColumn(
        modifier = modifier.fillMaxSize().testTag(SujianSemanticIds.SettingsScreen),
        contentPadding = PaddingValues(vertical = dims.space8),
    ) {
        SettingsGroup.entries.forEach { group ->
            val categories = settingsCategories.filter { it.group == group }
            if (categories.isEmpty()) return@forEach
            item(key = "settings_group_${group.name}") {
                Text(
                    text = stringResource(id = group.titleResId),
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.padding(horizontal = dims.space16, vertical = dims.space8),
                )
            }
            items(categories, key = { it.section.name }) { category ->
                SujianListItem(
                    headline = stringResource(id = category.titleResId),
                    supportingText = settingsCategorySummary(category),
                    valueText = settingsCategoryValue(category, state),
                    leadingIcon = category.icon,
                    selected = selectedSection == category.section,
                    onClick = { onNavigateToDetail(category.section) },
                    semanticId = when (category.section) {
                        SettingsSection.Appearance -> SujianSemanticIds.SettingsNavAppearance
                        SettingsSection.Editor -> SujianSemanticIds.SettingsNavEditor
                        SettingsSection.Save -> SujianSemanticIds.SettingsNavSave
                        SettingsSection.Sync -> SujianSemanticIds.SettingsNavSync
                        SettingsSection.Ai -> SujianSemanticIds.SettingsNavAi
                        SettingsSection.Diagnostics -> SujianSemanticIds.SettingsNavDiagnostics
                        SettingsSection.Laboratory -> SujianSemanticIds.SettingsNavLaboratory
                        SettingsSection.About -> SujianSemanticIds.SettingsNavAbout
                    },
                    trailingContent = {
                        Icon(
                            imageVector = SujianIcons.KeyboardArrowRight,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}

/**
 * 设置列表项的功能说明（静态文案，不冒充状态）：
 * 与 [settingsCategoryValue] 的“真实当前值”独立展示，不再二选一。
 */
@Composable
private fun settingsCategorySummary(category: SettingsCategory): String? = when (category.section) {
    SettingsSection.Appearance -> stringResource(id = R.string.pref_summary_appearance)
    SettingsSection.Editor -> stringResource(id = R.string.pref_summary_editor)
    SettingsSection.Save -> stringResource(id = R.string.pref_summary_save)
    SettingsSection.Sync -> stringResource(id = R.string.pref_summary_sync)
    SettingsSection.Ai -> stringResource(id = R.string.pref_summary_ai)
    SettingsSection.Diagnostics -> stringResource(id = R.string.pref_summary_diagnostics)
    SettingsSection.Laboratory -> stringResource(id = R.string.pref_summary_laboratory)
    SettingsSection.About -> stringResource(id = R.string.pref_summary_about)
}

/**
 * 设置列表项的真实当前值：全部来自真实 [SettingsUiState]，保存后随状态即时更新；
 * 无当前值可展示的分类（实验室/关于）返回 null，只保留功能说明。
 */
@Composable
private fun settingsCategoryValue(category: SettingsCategory, state: SettingsUiState): String? =
    when (category.section) {
        SettingsSection.Appearance -> when (state.settings.appearanceMode) {
            "light" -> stringResource(id = R.string.theme_light)
            "dark" -> stringResource(id = R.string.theme_dark)
            else -> stringResource(id = R.string.theme_system)
        }
        SettingsSection.Editor -> stringResource(
            id = R.string.pref_value_font_size,
            if (state.fontSize % 1f == 0f) state.fontSize.toInt().toString() else state.fontSize.toString(),
        )
        SettingsSection.Save -> if (state.settings.autoSaveEnabled) {
            stringResource(id = R.string.pref_state_on)
        } else {
            stringResource(id = R.string.pref_state_off)
        }
        SettingsSection.Sync -> if (state.syncConfig.enabled == true) {
            stringResource(id = R.string.pref_state_on)
        } else {
            stringResource(id = R.string.pref_state_off)
        }
        SettingsSection.Ai -> if (state.settings.aiEnabled) {
            stringResource(id = R.string.pref_state_on)
        } else {
            stringResource(id = R.string.pref_state_off)
        }
        SettingsSection.Diagnostics -> if (state.settings.diagnosticsEnabled) {
            stringResource(id = R.string.pref_state_on)
        } else {
            stringResource(id = R.string.pref_state_off)
        }
        SettingsSection.Laboratory,
        SettingsSection.About -> null
    }
