package com.xiwei.sujian.feature.settings.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.app.theme.ThemeRepository
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import com.xiwei.sujian.feature.sync.data.SyncCoordinator
import com.xiwei.sujian.feature.sync.data.SyncRepository
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

// ! # 设置 ViewModel（#618 六 复审：从 SettingsRoute 拆出，Route 只留 Compose 路由、
// 分类列表与渲染函数）。八份分节投影用 Eagerly：投影只是内存 StateFlow.map，
// ViewModel 活着时始终保持最新，真正减少的是 Compose 下游重组。
class SettingsViewModel(
    internal val settingsRepo: SettingsRepository,
    internal val themeRepo: ThemeRepository,
    internal val syncRepo: SyncRepository,
    internal val syncCoordinator: SyncCoordinator,
) : ViewModel() {
    /** internal 暴露 viewModelScope 供 extension functions 使用。 */
    internal val editorScope: kotlinx.coroutines.CoroutineScope get() = viewModelScope

    internal val _uiState = MutableStateFlow(SettingsUiState())
    val uiState: StateFlow<SettingsUiState> = _uiState.asStateFlow()

    // #618 六：只读分节状态 — 由 _uiState 派生（map -> distinctUntilChanged -> stateIn），
    // 不新建第二份可写状态。每个分类只订阅自己消费的字段投影：切换实验室开关时只有
    // laboratoryState 发新值，设置根节点与其它分类不重组。
    //
    // #618 六 复审：投影只是内存 StateFlow.map，不需要 WhileSubscribed 反复停掉再重启；
    // 直接用 Eagerly — ViewModel 活着时八份投影始终保持最新，真正减少的是 Compose 下游重组。
    val appearanceState: StateFlow<AppearanceSectionState> =
        uiState
            .map { state ->
                AppearanceSectionState(
                    appearanceMode = state.settings.appearanceMode,
                    colorSource = state.settings.colorSource,
                    dynamicColorEnabled = state.settings.dynamicColorEnabled,
                    fontSize = state.fontSize,
                    lineSpacing = state.settings.editorLineSpacingMultiplier,
                )
            }
            .distinctUntilChanged()
            .stateIn(
                viewModelScope,
                SharingStarted.Eagerly,
                AppearanceSectionState(
                    _uiState.value.settings.appearanceMode,
                    _uiState.value.settings.colorSource,
                    _uiState.value.settings.dynamicColorEnabled,
                    _uiState.value.fontSize,
                    _uiState.value.settings.editorLineSpacingMultiplier,
                ),
            )

    val editorState: StateFlow<EditorSectionState> =
        uiState
            .map { state ->
                EditorSectionState(
                    fontSize = state.fontSize,
                    autoIndentEnabled = state.settings.autoIndentEnabled,
                    autoIndentWidth = state.settings.autoIndentWidth,
                    typingAnimationEnabled = state.settings.editorTypingAnimationEnabled,
                    typingAnimationDurationMs = state.settings.editorTypingAnimationDurationMs,
                    smoothCursorEnabled = state.settings.editorSmoothCursorEnabled,
                    smoothCursorDurationMs = state.settings.editorSmoothCursorDurationMs,
                )
            }
            .distinctUntilChanged()
            .stateIn(
                viewModelScope,
                SharingStarted.Eagerly,
                EditorSectionState(
                    _uiState.value.fontSize,
                    _uiState.value.settings.autoIndentEnabled,
                    _uiState.value.settings.autoIndentWidth,
                    _uiState.value.settings.editorTypingAnimationEnabled,
                    _uiState.value.settings.editorTypingAnimationDurationMs,
                    _uiState.value.settings.editorSmoothCursorEnabled,
                    _uiState.value.settings.editorSmoothCursorDurationMs,
                ),
            )

    val saveState: StateFlow<SaveSectionState> =
        uiState
            .map { state ->
                SaveSectionState(
                    autoSaveEnabled = state.settings.autoSaveEnabled,
                    autoSaveDelayMs = state.settings.autoSaveDelayMs,
                )
            }
            .distinctUntilChanged()
            .stateIn(
                viewModelScope,
                SharingStarted.Eagerly,
                SaveSectionState(
                    _uiState.value.settings.autoSaveEnabled,
                    _uiState.value.settings.autoSaveDelayMs,
                ),
            )

    val syncState: StateFlow<SyncSectionState> =
        uiState
            .map { state ->
                SyncSectionState(
                    syncConfig = state.syncConfig,
                    syncSecrets = state.syncSecrets,
                    syncCapability = state.syncCapability,
                    syncProfileLoadState = state.syncProfileLoadState,
                    dryRunState = state.dryRunState,
                    testConnectionState = state.testConnectionState,
                    performSyncState = state.performSyncState,
                    syncResult = state.syncResult,
                    secureStorageWarning = state.secureStorageWarning,
                )
            }
            .distinctUntilChanged()
            .stateIn(
                viewModelScope,
                SharingStarted.Eagerly,
                SyncSectionState(
                    _uiState.value.syncConfig,
                    _uiState.value.syncSecrets,
                    _uiState.value.syncCapability,
                    _uiState.value.syncProfileLoadState,
                    _uiState.value.dryRunState,
                    _uiState.value.testConnectionState,
                    _uiState.value.performSyncState,
                    _uiState.value.syncResult,
                    _uiState.value.secureStorageWarning,
                ),
            )

    val aiState: StateFlow<AiSectionState> =
        uiState
            .map { state ->
                AiSectionState(
                    available = state.aiAvailable,
                    enabled = state.settings.aiEnabled,
                )
            }
            .distinctUntilChanged()
            .stateIn(
                viewModelScope,
                SharingStarted.Eagerly,
                AiSectionState(
                    _uiState.value.aiAvailable,
                    _uiState.value.settings.aiEnabled,
                ),
            )

    val diagnosticsState: StateFlow<DiagnosticsSectionState> =
        uiState
            .map { state ->
                DiagnosticsSectionState(
                    enabled = state.settings.diagnosticsEnabled,
                    verbose = state.settings.diagnosticsVerbose,
                )
            }
            .distinctUntilChanged()
            .stateIn(
                viewModelScope,
                SharingStarted.Eagerly,
                DiagnosticsSectionState(
                    _uiState.value.settings.diagnosticsEnabled,
                    _uiState.value.settings.diagnosticsVerbose,
                ),
            )

    val laboratoryState: StateFlow<LaboratorySectionState> =
        uiState
            .map { state ->
                LaboratorySectionState(
                    immersiveFullscreen = state.settings.experimentalFullscreenMode,
                )
            }
            .distinctUntilChanged()
            .stateIn(
                viewModelScope,
                SharingStarted.Eagerly,
                LaboratorySectionState(
                    _uiState.value.settings.experimentalFullscreenMode,
                ),
            )

    val aboutState: StateFlow<AboutSectionState> =
        uiState
            .map { state ->
                AboutSectionState(
                    dataRootPath = state.dataRootPath,
                    versionInfo = state.versionInfo,
                )
            }
            .distinctUntilChanged()
            .stateIn(
                viewModelScope,
                SharingStarted.Eagerly,
                AboutSectionState(
                    _uiState.value.dataRootPath,
                    _uiState.value.versionInfo,
                ),
            )

    // #630 评论15 项2：行级状态投影 — 每个 Lazy item 只 collect 自己那份最小字段，
    // 避免任一字段变化导致同分类所有可见 item 重组。
    // 统一 helper：selector 从 SettingsUiState 提取 T，distinctUntilChanged 过滤重复。
    private fun <T> rowFlow(
        initial: T,
        selector: (SettingsUiState) -> T,
    ): StateFlow<T> =
        uiState.map(selector).distinctUntilChanged()
            .stateIn(viewModelScope, SharingStarted.Eagerly, initial)

    // ── Appearance 行级 ──
    val appearanceModeRow =
        rowFlow(_uiState.value.settings.appearanceMode) { it.settings.appearanceMode }
    val colorSourceRow =
        rowFlow(_uiState.value.settings.colorSource) { it.settings.colorSource }
    val fontSizeRow = rowFlow(_uiState.value.fontSize) { it.fontSize }
    val lineSpacingRow =
        rowFlow(_uiState.value.settings.editorLineSpacingMultiplier) { it.settings.editorLineSpacingMultiplier }

    // ── Editor 行级 ──
    val autoIndentRow =
        rowFlow(_uiState.value.settings.autoIndentEnabled) { it.settings.autoIndentEnabled }
    val autoIndentWidthRow =
        rowFlow(_uiState.value.settings.autoIndentWidth) { it.settings.autoIndentWidth }
    val typingAnimationRow =
        rowFlow(_uiState.value.settings.editorTypingAnimationEnabled) { it.settings.editorTypingAnimationEnabled }
    val typingAnimationDurationRow =
        rowFlow(_uiState.value.settings.editorTypingAnimationDurationMs) { it.settings.editorTypingAnimationDurationMs }
    val smoothCursorRow =
        rowFlow(_uiState.value.settings.editorSmoothCursorEnabled) { it.settings.editorSmoothCursorEnabled }
    val smoothCursorDurationRow =
        rowFlow(_uiState.value.settings.editorSmoothCursorDurationMs) { it.settings.editorSmoothCursorDurationMs }

    // ── Save 行级 ──
    val autoSaveRow =
        rowFlow(_uiState.value.settings.autoSaveEnabled) { it.settings.autoSaveEnabled }
    val autoSaveDelayRow =
        rowFlow(_uiState.value.settings.autoSaveDelayMs) { it.settings.autoSaveDelayMs }

    // ── Sync 行级 — 每行只订阅自己需要的最小字段投影，避免任一 sync 字段变化导致全部可见 sync item 重组 ──

    data class SyncEnabledRowState(val enabled: Boolean)

    data class SyncAutoSyncRowState(val enabled: Boolean, val autoSync: Boolean)

    data class SyncRemoteUrlRowState(val enabled: Boolean, val remoteUrl: String)

    data class SyncBranchRowState(val enabled: Boolean, val branch: String)

    data class SyncTokenRowState(val enabled: Boolean, val token: String)

    data class SyncIntervalRowState(val enabled: Boolean, val intervalSeconds: Int)

    data class SyncActionsRowState(
        val enabled: Boolean,
        val capability: com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData,
        val dryRun: SyncCommandState,
        val test: SyncCommandState,
        val perform: SyncCommandState,
    )

    val syncEnabledRow =
        rowFlow(SyncEnabledRowState(_uiState.value.syncConfig.enabled == true)) {
            SyncEnabledRowState(it.syncConfig.enabled == true)
        }
    val syncAutoSyncRow =
        rowFlow(
            SyncAutoSyncRowState(
                _uiState.value.syncConfig.enabled == true,
                _uiState.value.syncConfig.autoSync == true,
            ),
        ) {
            SyncAutoSyncRowState(
                enabled = it.syncConfig.enabled == true,
                autoSync = it.syncConfig.autoSync == true,
            )
        }
    val syncRemoteUrlRow =
        rowFlow(
            SyncRemoteUrlRowState(
                _uiState.value.syncConfig.enabled == true,
                _uiState.value.syncConfig.remoteUrl.orEmpty(),
            ),
        ) {
            SyncRemoteUrlRowState(
                enabled = it.syncConfig.enabled == true,
                remoteUrl = it.syncConfig.remoteUrl.orEmpty(),
            )
        }
    val syncBranchRow =
        rowFlow(
            SyncBranchRowState(
                _uiState.value.syncConfig.enabled == true,
                _uiState.value.syncConfig.branch.orEmpty(),
            ),
        ) {
            SyncBranchRowState(
                enabled = it.syncConfig.enabled == true,
                branch = it.syncConfig.branch.orEmpty(),
            )
        }
    val syncTokenRow =
        rowFlow(
            SyncTokenRowState(
                _uiState.value.syncConfig.enabled == true,
                _uiState.value.syncSecrets.token.orEmpty(),
            ),
        ) {
            SyncTokenRowState(
                enabled = it.syncConfig.enabled == true,
                token = it.syncSecrets.token.orEmpty(),
            )
        }
    val syncIntervalRow =
        rowFlow(
            SyncIntervalRowState(
                _uiState.value.syncConfig.enabled == true,
                _uiState.value.syncConfig.syncIntervalSeconds ?: 300,
            ),
        ) {
            SyncIntervalRowState(
                enabled = it.syncConfig.enabled == true,
                intervalSeconds = it.syncConfig.syncIntervalSeconds ?: 300,
            )
        }
    val syncActionsRow =
        rowFlow(
            SyncActionsRowState(
                enabled = _uiState.value.syncConfig.enabled == true,
                capability = _uiState.value.syncCapability,
                dryRun = _uiState.value.dryRunState,
                test = _uiState.value.testConnectionState,
                perform = _uiState.value.performSyncState,
            ),
        ) {
            SyncActionsRowState(
                enabled = it.syncConfig.enabled == true,
                capability = it.syncCapability,
                dryRun = it.dryRunState,
                test = it.testConnectionState,
                perform = it.performSyncState,
            )
        }
    val syncResultRow =
        rowFlow(
            Pair(_uiState.value.syncResult, _uiState.value.syncProfileLoadState),
        ) { Pair(it.syncResult, it.syncProfileLoadState) }

    // ── Ai 行级 ──
    val aiAvailableRow =
        rowFlow(_uiState.value.aiAvailable) { it.aiAvailable }
    val aiEnabledRow =
        rowFlow(_uiState.value.settings.aiEnabled) { it.settings.aiEnabled }

    // ── Diagnostics 行级 ──
    val diagnosticsEnabledRow =
        rowFlow(_uiState.value.settings.diagnosticsEnabled) { it.settings.diagnosticsEnabled }
    val diagnosticsVerboseRow =
        rowFlow(_uiState.value.settings.diagnosticsVerbose) { it.settings.diagnosticsVerbose }

    // ── Laboratory 行级 ──
    val immersiveFullscreenRow =
        rowFlow(_uiState.value.settings.experimentalFullscreenMode) { it.settings.experimentalFullscreenMode }

    internal sealed interface QueueItem {
        data class Save(val command: SettingsSaveCommand) : QueueItem

        data class Transaction(val command: SettingsTransactionCommand) : QueueItem
    }

    internal val saveChannel = Channel<QueueItem>(Channel.UNLIMITED)
    internal val _saveFailureEvents = Channel<Int>(Channel.BUFFERED)
    val saveFailureEvents = _saveFailureEvents.receiveAsFlow()

    internal var localRevision = 0L
    internal var fontSizeRevision = 0L

    // 同步 revision（全量同步只有一份）
    internal var syncConfigRevision = 0L
    internal var syncSecretsRevision = 0L

    internal var localPersistedRevision = 0L
    internal var fontSizePersistedRevision = 0L

    // 同步 persisted revision
    internal var syncConfigPersistedRevision = 0L
    internal var syncSecretsPersistedRevision = 0L

    internal var pendingCommands = PendingCommands()

    internal fun hasUnsavedLocal() = localRevision != localPersistedRevision

    internal fun hasUnsavedFontSize() = fontSizeRevision != fontSizePersistedRevision

    internal fun hasUnsavedSyncConfig() = syncConfigRevision != syncConfigPersistedRevision

    internal fun hasUnsavedSyncSecrets() = syncSecretsRevision != syncSecretsPersistedRevision

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
                    // #630 评论二：affectsEditor 同样取并集 — 后一个非编辑器设置不能盖掉
                    // 前一个已记录的编辑器变化。
                    val affectsTheme =
                        command.affectsTheme || (pendingCommands.local?.affectsTheme == true)
                    val affectsEditor =
                        command.affectsEditor || (pendingCommands.local?.affectsEditor == true)
                    pendingCommands.copy(
                        local = command.copy(affectsTheme = affectsTheme, affectsEditor = affectsEditor),
                    )
                }
                is SettingsSaveCommand.FontSize -> pendingCommands.copy(fontSize = command)
                is SettingsSaveCommand.SyncConfig ->
                    pendingCommands.copy(syncConfig = command)
                is SettingsSaveCommand.SyncSecrets ->
                    pendingCommands.copy(syncSecrets = command)
            }
    }

    class Factory(
        internal val repo: SettingsRepository,
        internal val themeRepo: ThemeRepository,
        internal val syncRepo: SyncRepository,
        internal val coordinator: SyncCoordinator,
    ) : ViewModelProvider.Factory {
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
            is SettingsIntent.UpdateSyncConfig -> updateSyncConfig(intent.transform)
            is SettingsIntent.UpdateSyncSecrets -> updateSyncSecrets(intent.transform)
            else -> handleActionIntent(intent)
        }
    }

    private fun updateLocalSettings(newSettings: LocalSettings) {
        // #617 评论三：保存前比较旧值，只有真正影响主题的字段变化才标记 affectsTheme。
        // #630 评论二：同时标记 affectsEditor — 只有真正影响正文运行时的字段变化才通知
        // 编辑器重读设置；自动保存/AI/诊断/全屏/主题颜色不再触发编辑器重载。
        // previous==newSettings 时本就无需保存，直接早返回。
        val previous = _uiState.value.settings
        if (newSettings == previous) return
        val affectsTheme = newSettings.hasDifferentThemeFrom(previous)
        val affectsEditor = newSettings.hasDifferentEditorFrom(previous)

        _uiState.update { it.copy(settings = newSettings) }
        saveChannel.trySend(
            QueueItem.Save(
                SettingsSaveCommand.Local(
                    settings = newSettings,
                    revision = ++localRevision,
                    affectsTheme = affectsTheme,
                    affectsEditor = affectsEditor,
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

// #630 评论二：本地设置中真正影响正文运行时的字段集合 — 字号 fallback、行距、
// 首行缩进开关/宽度、文字动画开关/时长、光标动画开关/时长、协同动画、Android 自渲染
// 编辑器开关。自动保存、AI、诊断、沉浸式全屏、主题颜色变化都不算 editor change，
// 不触发编辑器重读设置。提取为 internal 顶层函数便于单测正反验证。
internal fun LocalSettings.hasDifferentEditorFrom(other: LocalSettings): Boolean =
    editorFontSize != other.editorFontSize ||
        editorLineSpacingMultiplier != other.editorLineSpacingMultiplier ||
        autoIndentEnabled != other.autoIndentEnabled ||
        autoIndentWidth != other.autoIndentWidth ||
        editorTypingAnimationEnabled != other.editorTypingAnimationEnabled ||
        editorTypingAnimationDurationMs != other.editorTypingAnimationDurationMs ||
        editorSmoothCursorEnabled != other.editorSmoothCursorEnabled ||
        editorSmoothCursorDurationMs != other.editorSmoothCursorDurationMs ||
        editorCoordinatedTextCursorAnimationEnabled != other.editorCoordinatedTextCursorAnimationEnabled ||
        useSelfRenderEditorOnAndroid != other.useSelfRenderEditorOnAndroid
