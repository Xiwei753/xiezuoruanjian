package com.xiwei.sujian.feature.settings.ui

// ! # 设置刷新合并操作（从 SettingsSaveOps 拆分）— 降低 TooManyFunctions
//
// #630 评论 #1+#2：同步 profile 只有一份，刷新合并只读一份。

import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// #597 刷新合并需一次性读取全部字段并按未保存状态合并，与 loadInitial 结构对称。
fun SettingsViewModel.mergeRefresh() {
    val repo = settingsRepo
    val syncRepoLocal = syncRepo
    editorScope.launch {
        val current = _uiState.value
        val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
        val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
        // #595 八：刷新读取活动 generation 的完整 snapshot，不再读 live legacy 槽。
        // #630 评论 #1+#2：同步 profile 只有一份。
        val committedProfile = withContext(Dispatchers.IO) { syncRepoLocal.loadCommittedSyncProfile() }
        val syncProfileLoadState = committedProfile.toSyncProfileLoadState()
        val syncCapability = withContext(Dispatchers.IO) { syncRepoLocal.getSyncCapability() }
        val secureStorageWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
        val builtinThemes = withContext(Dispatchers.IO) { themeRepo.listBuiltinThemes() }
        val paletteRecords = withContext(Dispatchers.IO) { themeRepo.listPaletteRecords() }
        val aiAvailable = withContext(Dispatchers.IO) { repo.aiAvailable() }
        // #649 评论 5559763924：数据根目录路径由 Repository 持有的 appContext 解析，
        // 不再直接调 AndroidDataRoot 无 context 重载。
        val dataRootPath = withContext(Dispatchers.IO) { repo.dataRootPath() }
        _uiState.update {
            SettingsUiState(
                settings = mergeLoadedLocal(current.settings, settings),
                fontSize = mergeLoadedFontSize(current.fontSize, fontSize),
                syncConfig = mergeLoadedSyncConfig(current.syncConfig, syncProfileLoadState),
                syncSecrets = mergeLoadedSyncSecrets(current.syncSecrets, syncProfileLoadState),
                syncCapability = syncCapability,
                syncProfileLoadState = syncProfileLoadState,
                dryRunState = current.dryRunState,
                testConnectionState = current.testConnectionState,
                performSyncState = current.performSyncState,
                syncResult = current.syncResult,
                secureStorageWarning = secureStorageWarning,
                builtinThemes = builtinThemes,
                paletteRecords = paletteRecords,
                aiAvailable = aiAvailable,
                dataRootPath = dataRootPath,
                lastCommandType = current.lastCommandType,
            )
        }
    }
}

// #597：未保存字段合并 — 刷新加载值只在用户没有未保存编辑时覆盖当前值。
private fun SettingsViewModel.mergeLoadedLocal(
    current: LocalSettings,
    loaded: LocalSettings,
): LocalSettings = if (!hasUnsavedLocal()) loaded else current

private fun SettingsViewModel.mergeLoadedFontSize(
    current: Float,
    loaded: Float,
): Float = if (!hasUnsavedFontSize()) loaded else current

private fun SettingsViewModel.mergeLoadedSyncConfig(
    current: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
    loadState: SyncProfileLoadState,
): com.xiwei.sujian.feature.sync.data.model.SyncConfig =
    if (!hasUnsavedSyncConfig()) loadState.confirmedConfig ?: current else current

private fun SettingsViewModel.mergeLoadedSyncSecrets(
    current: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
    loadState: SyncProfileLoadState,
): com.xiwei.sujian.feature.sync.data.model.SyncSecrets =
    if (!hasUnsavedSyncSecrets()) loadState.confirmedSecrets ?: current else current
