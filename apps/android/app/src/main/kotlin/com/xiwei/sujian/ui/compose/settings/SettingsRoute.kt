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
import com.xiwei.sujian.data.SaveField
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
    val saveErrorResId: Int? = null,
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

    private val saveChannel = Channel<SettingsUiState>(Channel.UNLIMITED)
    private val _saveFailureEvents = Channel<Int>(Channel.BUFFERED)
    val saveFailureEvents = _saveFailureEvents.receiveAsFlow()

    fun initialize(repo: SettingsRepository) {
        settingsRepo = repo
        refresh()
        viewModelScope.launch {
            for (snapshot in saveChannel) {
                var latest = snapshot
                while (true) {
                    val next = saveChannel.tryReceive().getOrNull() ?: break
                    latest = next
                }
                executeSave(latest)
            }
        }
    }

    private suspend fun executeSave(snapshot: SettingsUiState) {
        val repo = settingsRepo ?: return
        var failedField: SaveField? = null

        val settingsResult = withContext(Dispatchers.IO) { repo.saveLocalSettings(snapshot.settings) }
        when (settingsResult) {
            is SettingsSaveResult.Success -> {
                com.xiwei.sujian.ui.compose.theme.ThemeStore.reload()
            }
            is SettingsSaveResult.Failed -> {
                if (failedField == null) failedField = settingsResult.field
            }
        }

        val fontSizeResult = withContext(Dispatchers.IO) { repo.setFontSize(snapshot.fontSize) }
        if (failedField == null && fontSizeResult is SettingsSaveResult.Failed) {
            failedField = fontSizeResult.field
        }

        val syncConfigResult = withContext(Dispatchers.IO) { repo.saveSyncConfig(snapshot.syncConfig) }
        if (failedField == null && syncConfigResult is SettingsSaveResult.Failed) {
            failedField = syncConfigResult.field
        }

        val syncSecretsResult = withContext(Dispatchers.IO) { repo.saveSyncSecrets(snapshot.syncSecrets) }
        if (failedField == null && syncSecretsResult is SettingsSaveResult.Failed) {
            failedField = syncSecretsResult.field
        }

        if (failedField != null) {
            refreshSaveableFieldsFromRepository()
            val errorResId = when (failedField) {
                SaveField.LOCAL_SETTINGS -> R.string.save_local_settings_failed
                SaveField.FONT_SIZE -> R.string.save_font_size_failed
                SaveField.SYNC_CONFIG -> R.string.save_sync_config_failed
                SaveField.SYNC_SECRETS -> R.string.save_sync_secrets_failed
            }
            _uiState.update { it.copy(saveErrorResId = errorResId) }
            _saveFailureEvents.send(errorResId)
        } else {
            _uiState.update { it.copy(saveErrorResId = null) }
        }
    }

    private suspend fun refreshSaveableFieldsFromRepository() {
        val repo = settingsRepo ?: return
        val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
        val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
        val syncConfig = withContext(Dispatchers.IO) { repo.loadSyncConfig() }
        val syncSecrets = withContext(Dispatchers.IO) { repo.loadSyncSecrets() }
        _uiState.update { current ->
            current.copy(
                settings = settings,
                fontSize = fontSize,
                syncConfig = syncConfig,
                syncSecrets = syncSecrets,
            )
        }
    }

    fun consumeSaveError() {
        _uiState.update { it.copy(saveErrorResId = null) }
    }

    fun handleIntent(intent: SettingsIntent) {
        when (intent) {
            is SettingsIntent.UpdateLocal -> {
                _uiState.update { current ->
                    current.copy(settings = intent.transform(current.settings))
                }
                saveChannel.trySend(_uiState.value)
            }
            is SettingsIntent.UpdateFontSize -> {
                _uiState.update { it.copy(fontSize = intent.fontSize) }
                saveChannel.trySend(_uiState.value)
            }
            is SettingsIntent.UpdateSyncConfig -> {
                _uiState.update { it.copy(syncConfig = intent.config) }
                saveChannel.trySend(_uiState.value)
            }
            is SettingsIntent.UpdateSyncSecrets -> {
                _uiState.update { it.copy(syncSecrets = intent.secrets) }
                saveChannel.trySend(_uiState.value)
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
