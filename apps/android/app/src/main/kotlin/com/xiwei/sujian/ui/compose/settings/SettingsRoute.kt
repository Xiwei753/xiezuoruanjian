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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
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

    fun initialize(repo: SettingsRepository) {
        settingsRepo = repo
        refresh()
    }

    fun handleIntent(intent: SettingsIntent) {
        when (intent) {
            is SettingsIntent.UpdateLocal -> {
                val repo = settingsRepo ?: return
                val current = _uiState.value.settings
                val updated = intent.transform(current)
                repo.saveLocalSettings(updated)
                com.xiwei.sujian.ui.compose.theme.ThemeStore.reload()
                _uiState.value = _uiState.value.copy(settings = updated)
            }
            is SettingsIntent.UpdateFontSize -> {
                val repo = settingsRepo ?: return
                repo.setFontSize(intent.fontSize)
                _uiState.value = _uiState.value.copy(fontSize = intent.fontSize)
            }
            is SettingsIntent.UpdateSyncConfig -> {
                val repo = settingsRepo ?: return
                repo.saveSyncConfig(intent.config)
                _uiState.value = _uiState.value.copy(syncConfig = intent.config)
            }
            is SettingsIntent.UpdateSyncSecrets -> {
                val repo = settingsRepo ?: return
                repo.saveSyncSecrets(intent.secrets)
                _uiState.value = _uiState.value.copy(syncSecrets = intent.secrets)
            }
            is SettingsIntent.Refresh -> refresh()
            is SettingsIntent.CaptureDynamicColor -> {
                val repo = settingsRepo ?: return
                _uiState.value = _uiState.value.copy(
                    paletteRecords = repo.listPaletteRecords()
                )
            }
            is SettingsIntent.DeletePalette -> {
                val repo = settingsRepo ?: return
                repo.deletePaletteRecord(intent.deviceId, intent.fingerprint)
                _uiState.value = _uiState.value.copy(
                    paletteRecords = repo.listPaletteRecords()
                )
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
            _uiState.value = SettingsUiState(
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
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val vm: SettingsViewModel = viewModel()
    val uiState by vm.uiState.collectAsState()

    LaunchedEffect(Unit) {
        vm.initialize(SettingsRepository(context))
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
}

@Composable
fun SettingsListPane(
    onNavigateToDetail: (SettingsSection) -> Unit,
    modifier: Modifier = Modifier,
    selectedSection: SettingsSection? = null,
) {
    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
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
