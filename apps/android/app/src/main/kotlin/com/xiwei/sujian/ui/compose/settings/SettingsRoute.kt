package com.xiwei.sujian.ui.compose.settings

import android.os.Parcelable
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import com.xiwei.sujian.designsystem.icon.SujianIcons
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
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
import com.xiwei.sujian.designsystem.layout.SujianListDetailScaffold
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.ui.compose.navigation.SettingsSection
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
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
import com.xiwei.sujian.data.SyncSession

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
    val syncCommandResult: String? = null,
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

private data class PendingCommands(
    val local: SettingsSaveCommand.Local? = null,
    val fontSize: SettingsSaveCommand.FontSize? = null,
    val syncConfig: SettingsSaveCommand.SyncConfig? = null,
    val syncSecrets: SettingsSaveCommand.SyncSecrets? = null,
)

enum class SyncCommandType { DRY_RUN, TEST_CONNECTION, PERFORM_SYNC }

class SettingsViewModel : ViewModel() {
    private var settingsRepo: SettingsRepository? = null
    private var initialized = false
    private val _uiState = MutableStateFlow(SettingsUiState())
    val uiState: StateFlow<SettingsUiState> = _uiState.asStateFlow()

    private val saveChannel = Channel<SettingsSaveCommand>(Channel.UNLIMITED)
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
        viewModelScope.launch {
            for (command in saveChannel) {
                mergeCommand(command)
                while (true) {
                    val next = saveChannel.tryReceive().getOrNull() ?: break
                    mergeCommand(next)
                }
                flushPending()
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

    fun initialize(repo: SettingsRepository) {
        if (initialized && settingsRepo != null) {
            settingsRepo = repo
            return
        }
        settingsRepo = repo
        initialized = true
        loadInitial()
        viewModelScope.launch {
            flushPending()
        }
    }

    private fun loadInitial() {
        val repo = settingsRepo ?: return
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
        val repo = settingsRepo ?: return
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
                saveChannel.trySend(SettingsSaveCommand.Local(newSettings, rev))
            }
            is SettingsIntent.UpdateFontSize -> {
                _uiState.update { it.copy(fontSize = intent.fontSize) }
                val rev = ++fontSizeRevision
                saveChannel.trySend(SettingsSaveCommand.FontSize(intent.fontSize, rev))
            }
            is SettingsIntent.UpdateSyncConfig -> {
                _uiState.update { it.copy(syncConfig = intent.config) }
                val rev = ++syncConfigRevision
                saveChannel.trySend(SettingsSaveCommand.SyncConfig(intent.config, rev))
            }
            is SettingsIntent.UpdateSyncSecrets -> {
                _uiState.update { it.copy(syncSecrets = intent.secrets) }
                val rev = ++syncSecretsRevision
                saveChannel.trySend(SettingsSaveCommand.SyncSecrets(intent.secrets, rev))
            }
            is SettingsIntent.Refresh -> mergeRefresh()
            is SettingsIntent.CaptureDynamicColor -> {
                val repo = settingsRepo ?: return
                viewModelScope.launch {
                    val records = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
                    _uiState.update { it.copy(paletteRecords = records) }
                }
            }
            is SettingsIntent.DeletePalette -> {
                val repo = settingsRepo ?: return
                viewModelScope.launch {
                    withContext(Dispatchers.IO) { repo.deletePaletteRecord(intent.deviceId, intent.fingerprint) }
                    val records = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
                    _uiState.update { it.copy(paletteRecords = records) }
                }
            }
            is SettingsIntent.DryRun -> executeSyncCommand(SyncCommandType.DRY_RUN)
            is SettingsIntent.TestConnection -> executeSyncCommand(SyncCommandType.TEST_CONNECTION)
            is SettingsIntent.PerformSync -> executeSyncCommand(SyncCommandType.PERFORM_SYNC)
        }
    }

    private fun executeSyncCommand(type: SyncCommandType) {
        val repo = settingsRepo ?: return
        val config = _uiState.value.syncConfig
        val secrets = _uiState.value.syncSecrets

        if (!SyncSession.lock.compareAndSet(false, true)) {
            _uiState.update { it.copy(syncCommandResult = "sync_already_running", lastCommandType = type) }
            return
        }

        val taskId = SyncSession.currentTaskId.incrementAndGet()

        val runningStateField: SettingsUiState.() -> SettingsUiState
        val successStateField: SettingsUiState.() -> SettingsUiState
        val failureStateField: SettingsUiState.() -> SettingsUiState

        when (type) {
            SyncCommandType.DRY_RUN -> {
                runningStateField = { copy(dryRunState = SyncCommandState.RUNNING, syncCommandResult = null, lastCommandType = SyncCommandType.DRY_RUN) }
                successStateField = { copy(dryRunState = SyncCommandState.SUCCESS) }
                failureStateField = { copy(dryRunState = SyncCommandState.FAILURE) }
            }
            SyncCommandType.TEST_CONNECTION -> {
                runningStateField = { copy(testConnectionState = SyncCommandState.RUNNING, syncCommandResult = null, lastCommandType = SyncCommandType.TEST_CONNECTION) }
                successStateField = { copy(testConnectionState = SyncCommandState.SUCCESS) }
                failureStateField = { copy(testConnectionState = SyncCommandState.FAILURE) }
            }
            SyncCommandType.PERFORM_SYNC -> {
                runningStateField = { copy(performSyncState = SyncCommandState.RUNNING, syncCommandResult = null, lastCommandType = SyncCommandType.PERFORM_SYNC) }
                successStateField = { copy(performSyncState = SyncCommandState.SUCCESS) }
                failureStateField = { copy(performSyncState = SyncCommandState.FAILURE) }
            }
        }

        _uiState.update { it.runningStateField() }

        viewModelScope.launch {
            try {
                val result = withContext(Dispatchers.IO) {
                    val saveResult = repo.saveSyncConfig(config)
                    if (saveResult is SettingsSaveResult.Failed) {
                        return@withContext "save_config_failed" to false
                    }
                    syncConfigPersistedRevision = syncConfigRevision

                    val secretsResult = repo.saveSyncSecrets(secrets)
                    if (secretsResult is SettingsSaveResult.Failed) {
                        return@withContext "save_secrets_failed" to false
                    }
                    syncSecretsPersistedRevision = syncSecretsRevision

                    val capability = repo.getSyncCapability()
                    if (!capability.canRun) {
                        return@withContext (capability.blockReasonCode ?: "sync_not_ready") to false
                    }

                    when (type) {
                        SyncCommandType.DRY_RUN -> {
                            when (val r = repo.performSyncDryRun(config)) {
                                is BridgeResult.Success -> {
                                    val plan = r.data
                                    val msg = buildString {
                                        append("↑${plan.filesToUpload.size} ↓${plan.filesToDownload.size}")
                                        if (plan.filesToDeleteRemote.isNotEmpty()) append(" 远端删${plan.filesToDeleteRemote.size}")
                                        if (plan.filesToDeleteLocal.isNotEmpty()) append(" 本地删${plan.filesToDeleteLocal.size}")
                                        if (plan.conflicts.isNotEmpty()) append(" ⚠冲突${plan.conflicts.size}")
                                    }
                                    msg to true
                                }
                                is BridgeResult.Error -> (r.message ?: "dry_run_error") to false
                                BridgeResult.NotLoaded -> "core_not_loaded" to false
                            }
                        }
                        SyncCommandType.TEST_CONNECTION -> {
                            when (val r = repo.performSyncDiagnostics(config)) {
                                is BridgeResult.Success -> {
                                    val diag = r.data
                                    val msg = buildString {
                                        append(if (diag.success) "✓" else "✗")
                                        append(" 网络:${if (diag.networkOk) "✓" else "✗"}")
                                        append(" 认证:${if (diag.authOk) "✓" else "✗"}")
                                        append(" 仓库:${if (diag.repoOk) "✓" else "✗"}")
                                        append(" 分支:${if (diag.branchOk) "✓" else "✗"}")
                                        if (!diag.success && diag.rawError != null) append(" 错误:${diag.rawError}")
                                    }
                                    msg to diag.success
                                }
                                is BridgeResult.Error -> (r.message ?: "diagnostics_error") to false
                                BridgeResult.NotLoaded -> "core_not_loaded" to false
                            }
                        }
                        SyncCommandType.PERFORM_SYNC -> {
                            when (val r = repo.performSync(config)) {
                                is BridgeResult.Success -> {
                                    val sync = r.data
                                    val msg = buildString {
                                        append("↑${sync.uploadedFiles.size} ↓${sync.downloadedFiles.size}")
                                        if (sync.error != null) append(" 错误:${sync.error}")
                                    }
                                    msg to (sync.error == null)
                                }
                                is BridgeResult.Error -> (r.message ?: "sync_error") to false
                                BridgeResult.NotLoaded -> "core_not_loaded" to false
                            }
                        }
                    }
                }

                if (SyncSession.currentTaskId.get() != taskId) {
                    _uiState.update { it.failureStateField() }
                    return@launch
                }

                _uiState.update { current ->
                    val (message, success) = result
                    if (success) {
                        current.successStateField().copy(syncCommandResult = message)
                    } else {
                        current.failureStateField().copy(syncCommandResult = message)
                    }
                }
            } finally {
                SyncSession.lock.set(false)
                val refreshedCapability = withContext(Dispatchers.IO) { repo.getSyncCapability() }
                val refreshedWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
                _uiState.update { it.copy(syncCapability = refreshedCapability, secureStorageWarning = refreshedWarning) }
            }
        }
    }

    private fun mergeRefresh() {
        val repo = settingsRepo ?: return
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
                    syncCommandResult = current.syncCommandResult,
                    lastCommandType = current.lastCommandType,
                )
            }
        }
    }
}

data class SettingsCategory(
    val section: SettingsSection,
    val titleResId: Int,
    val icon: ImageVector,
)

val settingsCategories = listOf(
    SettingsCategory(SettingsSection.Appearance, R.string.pref_category_appearance, SujianIcons.Palette),
    SettingsCategory(SettingsSection.Editor, R.string.pref_category_editor, SujianIcons.Edit),
    SettingsCategory(SettingsSection.Save, R.string.pref_category_save, SujianIcons.Save),
    SettingsCategory(SettingsSection.Sync, R.string.pref_category_sync, SujianIcons.CloudSync),
    SettingsCategory(SettingsSection.Ai, R.string.pref_category_ai, SujianIcons.AutoStories),
    SettingsCategory(SettingsSection.Diagnostics, R.string.pref_category_diagnostics, SujianIcons.BugReport),
    SettingsCategory(SettingsSection.Laboratory, R.string.pref_category_laboratory, SujianIcons.Science),
    SettingsCategory(SettingsSection.About, R.string.pref_category_about, SujianIcons.Info),
)

@Parcelize
private data class SettingsSelection(val section: SettingsSection) : Parcelable

@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
@Composable
fun SettingsRoute(
    onNavigateBack: (() -> Unit)? = null,
    onNavigateToDetail: ((SettingsSection) -> Unit)? = null,
    initialSection: SettingsSection? = null,
    selectedSection: SettingsSection? = null,
    onSectionChange: ((SettingsSection) -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val vm: SettingsViewModel = viewModel()
    val uiState by vm.uiState.collectAsState()
    val snackbarHostState = remember { androidx.compose.material3.SnackbarHostState() }

    LaunchedEffect(Unit) {
        vm.initialize(SettingsRepository(context))
    }

    LaunchedEffect(Unit) {
        vm.saveFailureEvents.collect { errorResId ->
            snackbarHostState.showSnackbar(context.getString(errorResId))
        }
    }

    if (onNavigateToDetail != null) {
        SettingsListPane(
            onNavigateToDetail = onNavigateToDetail,
            selectedSection = null,
            modifier = modifier,
        )
        androidx.compose.material3.SnackbarHost(hostState = snackbarHostState)
        return
    }

    if (initialSection != null && selectedSection == null) {
        BackHandler(enabled = onNavigateBack != null) {
            onNavigateBack?.invoke()
        }
        SettingsDetailPane(
            section = initialSection,
            state = uiState,
            onIntent = vm::handleIntent,
            modifier = modifier,
        )
        androidx.compose.material3.SnackbarHost(hostState = snackbarHostState)
        return
    }

    if (selectedSection != null && onSectionChange != null) {
        SujianListDetailScaffold<SettingsSelection>(
            modifier = modifier,
            listPane = {
                SettingsListPane(
                    onNavigateToDetail = onSectionChange,
                    selectedSection = selectedSection,
                )
            },
            detailPane = {
                SettingsDetailPane(
                    section = selectedSection,
                    state = uiState,
                    onIntent = vm::handleIntent,
                )
            },
        )
        androidx.compose.material3.SnackbarHost(hostState = snackbarHostState)
        return
    }

    SujianListDetailScaffold<SettingsSelection>(
        modifier = modifier,
        listPane = {
            SettingsListPane(
                onNavigateToDetail = { section -> navigateToDetail(SettingsSelection(section)) },
                selectedSection = currentContentKey?.section,
            )
        },
        detailPane = {
            val selection = currentContentKey
            if (selection != null) {
                SettingsDetailPane(
                    section = selection.section,
                    state = uiState,
                    onIntent = vm::handleIntent,
                )
            }
            BackHandler(enabled = selection != null && onNavigateBack != null) {
                navigateBack()
            }
        },
    )
    androidx.compose.material3.SnackbarHost(hostState = snackbarHostState)
}

@Composable
fun SettingsListPane(
    onNavigateToDetail: (SettingsSection) -> Unit,
    modifier: Modifier = Modifier,
    selectedSection: SettingsSection? = null,
) {
    val dims = LocalSujianDimensions.current
    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(dims.space16),
        verticalArrangement = Arrangement.spacedBy(dims.space4),
    ) {
        items(settingsCategories) { category ->
            SujianListItem(
                headline = stringResource(id = category.titleResId),
                leadingIcon = category.icon,
                selected = selectedSection == category.section,
                onClick = { onNavigateToDetail(category.section) },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
