package com.xiwei.sujian.ui.compose.settings

import android.annotation.SuppressLint
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
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SyncCoordinator
import com.xiwei.sujian.designsystem.component.SujianListItem
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.layout.SujianListDetailScaffoldWithNavigator
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.runtime.LocalSujianAppDependencies
import com.xiwei.sujian.ui.compose.navigation.SettingsSection
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.parcelize.Parcelize

class SettingsViewModel(
    internal val settingsRepo: SettingsRepository,
    internal val syncCoordinator: com.xiwei.sujian.data.SyncCoordinator,
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
                is SettingsSaveCommand.Local -> pendingCommands.copy(local = command)
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
        internal val coordinator: com.xiwei.sujian.data.SyncCoordinator,
    ) : androidx.lifecycle.ViewModelProvider.Factory {
        override fun <T : ViewModel> create(modelClass: Class<T>): T {
            return modelClass.cast(SettingsViewModel(repo, coordinator)) as T
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
        _uiState.update { it.copy(settings = newSettings) }
        saveChannel.trySend(QueueItem.Save(SettingsSaveCommand.Local(newSettings, ++localRevision)))
    }

    private fun updateFontSize(fontSize: Float) {
        _uiState.update { it.copy(fontSize = fontSize) }
        saveChannel.trySend(QueueItem.Save(SettingsSaveCommand.FontSize(fontSize, ++fontSizeRevision)))
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
// context.getString 在 LaunchedEffect 协程中显示 snackbar，无法用 stringResource（非 Composable 上下文）。
@SuppressLint("LocalContextGetResourceValueCall")
@Composable
fun SettingsRoute(
    modifier: Modifier = Modifier,
    detailSection: SettingsSection? = null,
    onDetailSectionChange: ((SettingsSection?) -> Unit)? = null,
) {
    val deps = LocalSujianAppDependencies.current
    val vm: SettingsViewModel =
        viewModel(factory = SettingsViewModel.Factory(deps.settingsRepository, deps.syncCoordinator))
    val uiState by vm.uiState.collectAsState()
    val snackbarHostState = rememberSettingsSnackbarHost(vm)
    // 列表—详情窗格与返回/可预见返回必须共享同一个 navigator 实例：
    // 窗格由 navigator.currentDestination 驱动，返回层级（详情→列表→离开设置）
    // 与根壳状态同步都依赖同一份导航历史。
    val navigator = rememberSettingsNavigator(detailSection, onDetailSectionChange)

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
 *
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
 * 列表—详情 navigator + 根壳状态双向同步：navigator 内部变化（预测性返回）
 * 回写根壳；根壳状态变化（分类点击/顶栏返回）驱动窗格；详情→列表返回由
 * 本 navigator 处理，列表→离开设置交给全局 NavDisplay。
 */
@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun rememberSettingsNavigator(
    detailSection: SettingsSection?,
    onDetailSectionChange: ((SettingsSection?) -> Unit)?,
) = run {
    val navigator = rememberListDetailPaneScaffoldNavigator<SettingsSelection>()
    SettingsNavigatorEffects(navigator, detailSection, onDetailSectionChange)
    navigator
}

@OptIn(androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun SettingsNavigatorEffects(
    navigator: androidx.compose.material3.adaptive.navigation.ThreePaneScaffoldNavigator<SettingsSelection>,
    detailSection: SettingsSection?,
    onDetailSectionChange: ((SettingsSection?) -> Unit)?,
) {
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
}

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
                    semanticId =
                        when (category.section) {
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
