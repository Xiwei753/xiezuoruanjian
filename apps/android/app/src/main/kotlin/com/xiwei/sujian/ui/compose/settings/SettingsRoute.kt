package com.xiwei.sujian.ui.compose.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AutoStories
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.CloudSync
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Science
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
import com.xiwei.sujian.designsystem.layout.SujianScreenScaffold
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.ui.compose.navigation.SettingsSection
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

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
    SettingsCategory(SettingsSection.Appearance, R.string.pref_category_appearance, Icons.Default.Palette),
    SettingsCategory(SettingsSection.Editor, R.string.pref_category_editor, Icons.Default.Edit),
    SettingsCategory(SettingsSection.Save, R.string.pref_category_save, Icons.Default.Save),
    SettingsCategory(SettingsSection.Sync, R.string.pref_category_sync, Icons.Default.CloudSync),
    SettingsCategory(SettingsSection.Ai, R.string.pref_category_ai, Icons.Default.AutoStories),
    SettingsCategory(SettingsSection.Diagnostics, R.string.pref_category_diagnostics, Icons.Default.BugReport),
    SettingsCategory(SettingsSection.Laboratory, R.string.pref_category_laboratory, Icons.Default.Science),
    SettingsCategory(SettingsSection.About, R.string.pref_category_about, Icons.Default.Info),
)

@Composable
fun SettingsRoute(
    onNavigateBack: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val vm: SettingsViewModel = viewModel()
    val uiState by vm.uiState.collectAsState()
    var selectedSection by remember { mutableStateOf<SettingsSection?>(null) }

    LaunchedEffect(Unit) {
        vm.initialize(SettingsRepository(context))
    }

    val windowWidthSizeClass = currentWindowAdaptiveInfo().windowSizeClass.windowWidthSizeClass
    val isExpanded = windowWidthSizeClass == androidx.window.core.layout.WindowWidthSizeClass.EXPANDED

    if (isExpanded) {
        androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold(
            navigationSuiteItems = {},
            modifier = modifier.fillMaxSize(),
        ) {
            androidx.compose.foundation.layout.Row(modifier = Modifier.fillMaxSize()) {
                androidx.compose.foundation.layout.Box(modifier = Modifier.weight(1f)) {
                    SettingsListPane(
                        onNavigateToDetail = { section -> selectedSection = section },
                        selectedSection = selectedSection,
                    )
                }
                androidx.compose.foundation.layout.Box(modifier = Modifier.weight(1f)) {
                    if (selectedSection != null) {
                        SettingsDetailPane(
                            section = selectedSection!!,
                            state = uiState,
                            onIntent = vm::handleIntent,
                        )
                    }
                }
            }
        }
    } else {
        if (selectedSection != null) {
            val section = selectedSection!!
            SujianScreenScaffold(
                title = settingsCategories.find { it.section == section }?.let {
                    stringResource(id = it.titleResId)
                } ?: "",
                navigationIcon = Icons.AutoMirrored.Filled.ArrowBack,
                onNavigationClick = { selectedSection = null },
            ) { innerPadding ->
                SettingsDetailPane(
                    section = section,
                    state = uiState,
                    onIntent = vm::handleIntent,
                    modifier = Modifier.padding(innerPadding),
                )
            }
        } else {
            SujianScreenScaffold(
                title = stringResource(id = R.string.action_settings),
                onNavigateBack = onNavigateBack,
            ) { innerPadding ->
                SettingsListPane(
                    onNavigateToDetail = { section -> selectedSection = section },
                    selectedSection = selectedSection,
                    modifier = Modifier.padding(innerPadding),
                )
            }
        }
    }
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
