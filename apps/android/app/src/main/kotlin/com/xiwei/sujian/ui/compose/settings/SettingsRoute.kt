package com.xiwei.sujian.ui.compose.settings

import android.os.Parcelable
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
import com.xiwei.sujian.data.SettingsRepository
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

data class SettingsUiState(
    val settings: LocalSettings = LocalSettings(),
    val fontSize: Float = 16f,
    val syncConfig: com.xiwei.sujian.model.SyncConfig = com.xiwei.sujian.model.SyncConfig(),
    val syncSecrets: com.xiwei.sujian.model.SyncSecrets = com.xiwei.sujian.model.SyncSecrets(),
    val builtinThemes: List<uniffi.writer_core.BuiltinThemeDto> = emptyList(),
    val paletteRecords: List<uniffi.writer_core.ThemePaletteRecordDto> = emptyList(),
    val aiAvailable: Boolean = false,
    val workspacePath: String = "",
    val versionInfo: String = "",
    val saveError: String? = null,
)

sealed interface SettingsIntent {
    data class UpdateLocal(val transform: (LocalSettings) -> LocalSettings) : SettingsIntent
    data class UpdateFontSize(val fontSize: Float) : SettingsIntent
    data class UpdateSyncConfig(val config: com.xiwei.sujian.model.SyncConfig) : SettingsIntent
    data class UpdateSyncSecrets(val secrets: com.xiwei.sujian.model.SyncSecrets) : SettingsIntent
    data object Refresh : SettingsIntent
    data object CaptureDynamicColor : SettingsIntent
    data class DeletePalette(val deviceId: String, val fingerprint: String) : SettingsIntent
}

class SettingsViewModel : ViewModel() {
    private var settingsRepo: SettingsRepository? = null
    private val _uiState = MutableStateFlow(SettingsUiState())
    val uiState: StateFlow<SettingsUiState> = _uiState.asStateFlow()

    private val _snackbarEvents = Channel<String>(Channel.BUFFERED)
    val snackbarEvents = _snackbarEvents.receiveAsFlow()

    private sealed interface SaveCommand {
        data class SaveLocalSettings(val settings: LocalSettings, val previous: LocalSettings) : SaveCommand
        data class SaveFontSize(val fontSize: Float, val previous: Float) : SaveCommand
        data class SaveSyncConfig(val config: com.xiwei.sujian.model.SyncConfig, val previous: com.xiwei.sujian.model.SyncConfig) : SaveCommand
        data class SaveSyncSecrets(val secrets: com.xiwei.sujian.model.SyncSecrets, val previous: com.xiwei.sujian.model.SyncSecrets) : SaveCommand
    }

    private val saveChannel = Channel<SaveCommand>(Channel.UNLIMITED)

    fun initialize(repo: SettingsRepository) {
        settingsRepo = repo
        refresh()
        viewModelScope.launch {
            for (cmd in saveChannel) {
                executeSave(cmd)
            }
        }
    }

    private suspend fun executeSave(cmd: SaveCommand) {
        val repo = settingsRepo ?: return
        when (cmd) {
            is SaveCommand.SaveLocalSettings -> {
                val success = withContext(Dispatchers.IO) {
                    repo.saveLocalSettings(cmd.settings)
                }
                if (success) {
                    com.xiwei.sujian.ui.compose.theme.ThemeStore.reload()
                } else {
                    _uiState.update { it.copy(settings = cmd.previous, saveError = "保存本地设置失败") }
                    _snackbarEvents.send("保存本地设置失败")
                }
            }
            is SaveCommand.SaveFontSize -> {
                val success = withContext(Dispatchers.IO) { repo.setFontSize(cmd.fontSize) }
                if (!success) {
                    _uiState.update { it.copy(fontSize = cmd.previous, saveError = "保存字体大小失败") }
                    _snackbarEvents.send("保存字体大小失败")
                }
            }
            is SaveCommand.SaveSyncConfig -> {
                val success = withContext(Dispatchers.IO) { repo.saveSyncConfig(cmd.config) }
                if (!success) {
                    _uiState.update { it.copy(syncConfig = cmd.previous, saveError = "保存同步配置失败") }
                    _snackbarEvents.send("保存同步配置失败")
                }
            }
            is SaveCommand.SaveSyncSecrets -> {
                val success = withContext(Dispatchers.IO) { repo.saveSyncSecrets(cmd.secrets) }
                if (!success) {
                    _uiState.update { it.copy(syncSecrets = cmd.previous, saveError = "保存同步密钥失败") }
                    _snackbarEvents.send("保存同步密钥失败")
                }
            }
        }
    }

    fun consumeSaveError() {
        _uiState.update { it.copy(saveError = null) }
    }

    fun handleIntent(intent: SettingsIntent) {
        when (intent) {
            is SettingsIntent.UpdateLocal -> {
                val current = _uiState.value.settings
                val updated = intent.transform(current)
                _uiState.update { it.copy(settings = updated) }
                saveChannel.trySend(SaveCommand.SaveLocalSettings(updated, current))
            }
            is SettingsIntent.UpdateFontSize -> {
                val previous = _uiState.value.fontSize
                val fontSize = intent.fontSize
                _uiState.update { it.copy(fontSize = fontSize) }
                saveChannel.trySend(SaveCommand.SaveFontSize(fontSize, previous))
            }
            is SettingsIntent.UpdateSyncConfig -> {
                val previous = _uiState.value.syncConfig
                val config = intent.config
                _uiState.update { it.copy(syncConfig = config) }
                saveChannel.trySend(SaveCommand.SaveSyncConfig(config, previous))
            }
            is SettingsIntent.UpdateSyncSecrets -> {
                val previous = _uiState.value.syncSecrets
                val secrets = intent.secrets
                _uiState.update { it.copy(syncSecrets = secrets) }
                saveChannel.trySend(SaveCommand.SaveSyncSecrets(secrets, previous))
            }
            is SettingsIntent.Refresh -> refresh()
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
        }
    }

    private fun refresh() {
        val repo = settingsRepo ?: return
        viewModelScope.launch {
            val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
            val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
            val syncConfig = withContext(Dispatchers.IO) { repo.loadSyncConfig() }
            val syncSecrets = withContext(Dispatchers.IO) { repo.loadSyncSecrets() }
            val builtinThemes = withContext(Dispatchers.IO) { repo.listBuiltinThemes() }
            val paletteRecords = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
            val aiAvailable = withContext(Dispatchers.IO) { repo.aiAvailable() }
            val workspacePath = withContext(Dispatchers.IO) { repo.workspaceDir() }
            _uiState.update {
                SettingsUiState(
                    settings = settings,
                    fontSize = fontSize,
                    syncConfig = syncConfig,
                    syncSecrets = syncSecrets,
                    builtinThemes = builtinThemes,
                    paletteRecords = paletteRecords,
                    aiAvailable = aiAvailable,
                    workspacePath = workspacePath,
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
        vm.snackbarEvents.collect { message ->
            snackbarHostState.showSnackbar(message)
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

    if (initialSection != null) {
        SettingsDetailPane(
            section = initialSection,
            state = uiState,
            onIntent = vm::handleIntent,
            modifier = modifier,
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
