package com.xiwei.sujian.feature.settings.ui

import android.annotation.SuppressLint
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.app.navigation.SettingsSection
import com.xiwei.sujian.app.theme.ThemeRepository
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import com.xiwei.sujian.feature.sync.data.SyncCoordinator
import com.xiwei.sujian.feature.sync.data.SyncRepository
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

class SettingsViewModel(
    internal val settingsRepo: SettingsRepository,
    internal val themeRepo: ThemeRepository,
    internal val syncRepo: SyncRepository,
    internal val syncCoordinator: com.xiwei.sujian.feature.sync.data.SyncCoordinator,
) : ViewModel() {
    /** internal 暴露 viewModelScope 供 extension functions 使用。 */
    internal val editorScope: kotlinx.coroutines.CoroutineScope get() = viewModelScope

    internal val _uiState = MutableStateFlow(SettingsUiState())
    val uiState: StateFlow<SettingsUiState> = _uiState.asStateFlow()

    internal sealed interface QueueItem {
        data class Save(val command: SettingsSaveCommand) : QueueItem

        data class Transaction(val command: SettingsTransactionCommand) : QueueItem
    }

    internal val saveChannel = Channel<QueueItem>(Channel.UNLIMITED)
    internal val _saveFailureEvents = Channel<Int>(Channel.BUFFERED)
    val saveFailureEvents = _saveFailureEvents.receiveAsFlow()

    internal var localRevision = 0L
    internal var fontSizeRevision = 0L

    // 作品级同步 revision
    internal var projectSyncConfigRevision = 0L
    internal var projectSyncSecretsRevision = 0L

    // 应用级同步 revision
    internal var appSyncConfigRevision = 0L
    internal var appSyncSecretsRevision = 0L

    internal var localPersistedRevision = 0L
    internal var fontSizePersistedRevision = 0L

    // 作品级同步 persisted revision
    internal var projectSyncConfigPersistedRevision = 0L
    internal var projectSyncSecretsPersistedRevision = 0L

    // 应用级同步 persisted revision
    internal var appSyncConfigPersistedRevision = 0L
    internal var appSyncSecretsPersistedRevision = 0L

    internal var pendingCommands = PendingCommands()

    internal fun hasUnsavedLocal() = localRevision != localPersistedRevision

    internal fun hasUnsavedFontSize() = fontSizeRevision != fontSizePersistedRevision

    internal fun hasUnsavedProjectSyncConfig() = projectSyncConfigRevision != projectSyncConfigPersistedRevision

    internal fun hasUnsavedProjectSyncSecrets() = projectSyncSecretsRevision != projectSyncSecretsPersistedRevision

    internal fun hasUnsavedAppSyncConfig() = appSyncConfigRevision != appSyncConfigPersistedRevision

    internal fun hasUnsavedAppSyncSecrets() = appSyncSecretsRevision != appSyncSecretsPersistedRevision

    init {
        loadInitial()
        // #600 评论 #7 / #618 三：只有外部同步拉取设置时才重新从 Core 加载设置状态；
        // 本机保存不再回环（本地保存只发 editorSettingsChanged，不进这里）。
        viewModelScope.launch {
            com.xiwei.sujian.feature.settings.data.CoreSettingsEvents.externalSettingsChanged.collect {
                reloadFromExternalSync()
            }
        }
        viewModelScope.launch {
            var nextItem: QueueItem? = null
            while (true) {
                val item = nextItem ?: saveChannel.receive()
                nextItem = null
                when (item) {
                    is QueueItem.Save -> {
                        mergeCommand(item.command)
                        while (true) {
                            val next = saveChannel.tryReceive().getOrNull()
                            if (next is QueueItem.Save) {
                                mergeCommand(next.command)
                            } else {
                                nextItem = next
                                break
                            }
                        }
                        flushPending()
                    }
                    is QueueItem.Transaction -> {
                        flushPending()
                        executeTransaction(item.command)
                    }
                }
            }
        }
    }

    internal fun mergeCommand(command: SettingsSaveCommand) {
        pendingCommands =
            when (command) {
                is SettingsSaveCommand.Local -> {
                    // #617 评论三：合并连续本地设置时，后一个非主题设置不能盖掉
                    // 前一个已记录的主题变化 — affectsTheme 取并集。
                    val affectsTheme =
                        command.affectsTheme || (pendingCommands.local?.affectsTheme == true)
                    pendingCommands.copy(local = command.copy(affectsTheme = affectsTheme))
                }
                is SettingsSaveCommand.FontSize -> pendingCommands.copy(fontSize = command)
                is SettingsSaveCommand.ProjectSyncConfig ->
                    pendingCommands.copy(projectSyncConfig = command)
                is SettingsSaveCommand.ProjectSyncSecrets ->
                    pendingCommands.copy(projectSyncSecrets = command)
                is SettingsSaveCommand.AppSyncConfig ->
                    pendingCommands.copy(appSyncConfig = command)
                is SettingsSaveCommand.AppSyncSecrets ->
                    pendingCommands.copy(appSyncSecrets = command)
            }
    }

    class Factory(
        internal val repo: SettingsRepository,
        internal val themeRepo: ThemeRepository,
        internal val syncRepo: SyncRepository,
        internal val coordinator: com.xiwei.sujian.feature.sync.data.SyncCoordinator,
    ) : androidx.lifecycle.ViewModelProvider.Factory {
        override fun <T : ViewModel> create(modelClass: Class<T>): T {
            return modelClass.cast(SettingsViewModel(repo, themeRepo, syncRepo, coordinator)) as T
        }
    }

    /**
     * #595 五：成功保存或重新加载 profile 后，一次性更新 syncConfig、syncSecrets、
     * syncProfileLoadState、syncCapability、secureStorageWarning —
     * 用户修好配置后旧红色错误提示立即消失，不再残留。
     */

    fun consumeSaveError() {
        _uiState.update { it.copy(saveErrorResId = null) }
    }

    fun handleIntent(intent: SettingsIntent) {
        when (intent) {
            is SettingsIntent.UpdateLocal -> updateLocalSettings(intent.transform(_uiState.value.settings))
            is SettingsIntent.UpdateFontSize -> updateFontSize(intent.fontSize)
            is SettingsIntent.UpdateProjectSyncConfig -> updateProjectSyncConfig(intent.config)
            is SettingsIntent.UpdateProjectSyncSecrets -> updateProjectSyncSecrets(intent.secrets)
            is SettingsIntent.UpdateAppSyncConfig -> updateAppSyncConfig(intent.config)
            is SettingsIntent.UpdateAppSyncSecrets -> updateAppSyncSecrets(intent.secrets)
            else -> handleActionIntent(intent)
        }
    }

    private fun updateLocalSettings(newSettings: LocalSettings) {
        // #617 评论三：保存前比较旧值，只有真正影响主题的字段变化才标记 affectsTheme。
        val previous = _uiState.value.settings
        val affectsTheme = newSettings.hasDifferentThemeFrom(previous)

        _uiState.update { it.copy(settings = newSettings) }
        saveChannel.trySend(
            QueueItem.Save(
                SettingsSaveCommand.Local(
                    settings = newSettings,
                    revision = ++localRevision,
                    affectsTheme = affectsTheme,
                ),
            ),
        )
    }

    private fun updateFontSize(fontSize: Float) {
        _uiState.update { it.copy(fontSize = fontSize) }
        saveChannel.trySend(QueueItem.Save(SettingsSaveCommand.FontSize(fontSize, ++fontSizeRevision)))
    }
}

// #617 评论三：本地设置中真正影响主题的字段集合 — 其余字段（自动保存、诊断、
// 编辑器选项、沉浸式全屏等）变化不触发主题重建。提取为 internal 顶层函数便于单测正反验证。
internal fun LocalSettings.hasDifferentThemeFrom(other: LocalSettings): Boolean =
    appearanceMode != other.appearanceMode ||
        colorSource != other.colorSource ||
        dynamicColorEnabled != other.dynamicColorEnabled ||
        selectedBuiltinThemeId != other.selectedBuiltinThemeId ||
        selectedPaletteId != other.selectedPaletteId

/**
 * 设置列表分组（手机列表按组呈现，组内每一项显示标题、说明或当前值与展开箭头）。
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

val settingsCategories =
    listOf(
        SettingsCategory(
            SettingsSection.Appearance,
            R.string.pref_category_appearance,
            SujianIcons.Palette,
            SettingsGroup.Appearance,
        ),
        SettingsCategory(
            SettingsSection.Editor,
            R.string.pref_category_editor,
            SujianIcons.Edit,
            SettingsGroup.Writing,
        ),
        SettingsCategory(SettingsSection.Save, R.string.pref_category_save, SujianIcons.Save, SettingsGroup.Writing),
        SettingsCategory(
            SettingsSection.Sync,
            R.string.pref_category_sync,
            SujianIcons.CloudSync,
            SettingsGroup.DataSync,
        ),
        SettingsCategory(
            SettingsSection.Ai,
            R.string.pref_category_ai,
            SujianIcons.AutoStories,
            SettingsGroup.Advanced,
        ),
        SettingsCategory(
            SettingsSection.Diagnostics,
            R.string.pref_category_diagnostics,
            SujianIcons.BugReport,
            SettingsGroup.Advanced,
        ),
        SettingsCategory(
            SettingsSection.Laboratory,
            R.string.pref_category_laboratory,
            SujianIcons.Science,
            SettingsGroup.Advanced,
        ),
        SettingsCategory(SettingsSection.About, R.string.pref_category_about, SujianIcons.Info, SettingsGroup.About),
    )

/**
 * 设置一级 destination 的唯一内容 — 同页折叠面板。
 *
 * 每个分类是一个 [SettingsExpandableSection]：点击标题行切换展开/折叠，
 * 展开内容在行内用 [AnimatedVisibility] 呈现，不导航到子页面，
 * 不建立第二套页面导航状态。展开状态用 [rememberSaveable] 跨配置变更保留。
 *
 * context.getString 在协程 collect 回调中解析错误文案，无法用 stringResource，
 * 因此本文件带单规则 SuppressLint。
 */
@SuppressLint("LocalContextGetResourceValueCall")
@Composable
fun SettingsRoute(modifier: Modifier = Modifier) {
    val deps = LocalSujianAppDependencies.current
    val vm: SettingsViewModel =
        viewModel(
            factory =
                SettingsViewModel.Factory(
                    deps.settingsRepository,
                    deps.themeRepository,
                    deps.syncRepository,
                    deps.syncCoordinator,
                ),
        )
    val uiState by vm.uiState.collectAsState()
    val snackbarHostState = rememberSettingsSnackbarHost(vm)
    val dims = LocalSujianDimensions.current

    val expansionState =
        rememberSaveable(saver = SettingsExpansionState.Saver) {
            SettingsExpansionState()
        }

    Box(modifier = modifier.fillMaxSize()) {
        LazyColumn(
            modifier = Modifier.fillMaxSize().testTag(SujianSemanticIds.SettingsScreen),
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
                    SettingsCategoryItem(
                        category = category,
                        uiState = uiState,
                        expanded = expansionState.isExpanded(category.section),
                        onExpandedChange = { isExpanded ->
                            expansionState.setExpanded(category.section, isExpanded)
                        },
                        onIntent = vm::handleIntent,
                    )
                }
            }
        }
        androidx.compose.material3.SnackbarHost(
            hostState = snackbarHostState,
            modifier = Modifier.align(Alignment.BottomCenter),
        )
    }
}

/**
 * 单个设置分类的折叠面板项 — 从 [SettingsRoute] 中提取以控制函数长度与认知复杂度。
 */
@Composable
private fun SettingsCategoryItem(
    category: SettingsCategory,
    uiState: SettingsUiState,
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
    onIntent: (SettingsIntent) -> Unit,
) {
    SettingsExpandableSection(
        title = stringResource(id = category.titleResId),
        summary = settingsCategorySummary(category),
        value = settingsCategoryValue(category, uiState),
        expanded = expanded,
        onExpandedChange = onExpandedChange,
        content = {
            when (category.section) {
                SettingsSection.Appearance -> AppearanceSettings(state = uiState, onIntent = onIntent)
                SettingsSection.Editor -> EditorSettings(state = uiState, onIntent = onIntent)
                SettingsSection.Save -> SaveSettings(state = uiState, onIntent = onIntent)
                SettingsSection.Sync -> SyncSettings(state = uiState, onIntent = onIntent)
                SettingsSection.Ai -> AiSettings(state = uiState, onIntent = onIntent)
                SettingsSection.Diagnostics -> DiagnosticsSettings(state = uiState, onIntent = onIntent)
                SettingsSection.Laboratory -> LaboratorySettings(state = uiState, onIntent = onIntent)
                SettingsSection.About -> AboutSettings(state = uiState, onIntent = onIntent)
            }
        },
    )
}

/**
 * 收集设置保存失败事件并在底部 snackbar 展示（协程上下文用
 * context.getString，非 Composable 上下文无法用 stringResource）。
 *
 * context.getString 在协程 collect 回调中解析错误文案，无法用 stringResource，
 * 因此本函数带单规则 SuppressLint。
 */
@SuppressLint("LocalContextGetResourceValueCall")
@Composable
private fun rememberSettingsSnackbarHost(vm: SettingsViewModel): androidx.compose.material3.SnackbarHostState {
    val context = LocalContext.current
    val snackbarHostState = remember { androidx.compose.material3.SnackbarHostState() }
    LaunchedEffect(Unit) {
        vm.saveFailureEvents.collect { errorResId ->
            snackbarHostState.showSnackbar(context.getString(errorResId))
        }
    }
    return snackbarHostState
}

/**
 * 设置列表项的功能说明（静态文案，不冒充状态）：
 * 与 [settingsCategoryValue] 的“真实当前值”独立展示，不再二选一。
 */
@Composable
internal fun settingsCategorySummary(category: SettingsCategory): String? =
    when (category.section) {
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
internal fun settingsCategoryValue(
    category: SettingsCategory,
    state: SettingsUiState,
): String? =
    when (category.section) {
        SettingsSection.Appearance -> appearanceModeValue(state.settings.appearanceMode)
        SettingsSection.Editor -> fontSizeValue(state.fontSize)
        SettingsSection.Save -> toggleValue(state.settings.autoSaveEnabled)
        SettingsSection.Sync -> toggleValue(state.projectSyncConfig.enabled == true)
        SettingsSection.Ai -> toggleValue(state.settings.aiEnabled)
        SettingsSection.Diagnostics -> toggleValue(state.settings.diagnosticsEnabled)
        SettingsSection.Laboratory,
        SettingsSection.About,
        -> null
    }

@Composable
private fun appearanceModeValue(mode: String): String =
    when (mode) {
        "light" -> stringResource(id = R.string.theme_light)
        "dark" -> stringResource(id = R.string.theme_dark)
        else -> stringResource(id = R.string.theme_system)
    }

@Composable
private fun fontSizeValue(fontSize: Float): String =
    stringResource(
        id = R.string.pref_value_font_size,
        if (fontSize % 1f == 0f) fontSize.toInt().toString() else fontSize.toString(),
    )

@Composable
private fun toggleValue(enabled: Boolean): String =
    if (enabled) {
        stringResource(id = R.string.pref_state_on)
    } else {
        stringResource(id = R.string.pref_state_off)
    }
