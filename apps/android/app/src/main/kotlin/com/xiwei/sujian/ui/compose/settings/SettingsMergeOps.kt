package com.xiwei.sujian.ui.compose.settings

// ! # 设置刷新合并操作（从 SettingsSaveOps 拆分）— 降低 TooManyFunctions

import com.xiwei.sujian.model.LocalSettings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// #597 刷新合并需一次性读取全部字段并按未保存状态合并，与 loadInitial 结构对称。
fun SettingsViewModel.mergeRefresh() {
    val repo = settingsRepo
    editorScope.launch {
        val current = _uiState.value
        val settings = withContext(Dispatchers.IO) { repo.getLocalSettings() }
        val fontSize = withContext(Dispatchers.IO) { repo.getEffectiveFontSize() }
        // #595 八：刷新读取活动 generation 的完整 snapshot，不再读 live legacy 槽。
        // #600 评论 #3 问题二：按当前活动作品路由。
        val mergeProjectId = com.xiwei.sujian.data.ActiveProjectGate.currentProjectId()
        val committedProfile =
            if (mergeProjectId != null) {
                withContext(Dispatchers.IO) { repo.loadCommittedSyncProfile(mergeProjectId) }
            } else {
                com.xiwei.sujian.data.SyncProfileReadResult.Failed(
                    com.xiwei.sujian.data.SyncFailureKind.Fatal,
                    MSG_NO_ACTIVE_PROJECT,
                )
            }
        // #595 四：类型化加载状态 — Failed 时保留字段值，页面显示真实错误。
        val projectSyncProfileLoadState = committedProfile.toSyncProfileLoadState()
        val projectSyncCapability =
            if (mergeProjectId != null) {
                withContext(Dispatchers.IO) { repo.getSyncCapability(mergeProjectId) }
            } else {
                com.xiwei.sujian.model.SyncCapabilityData()
            }
        // #600 评论 #5：刷新应用级 profile。
        val committedAppProfile = withContext(Dispatchers.IO) { repo.loadCommittedAppSyncProfile() }
        val appSyncProfileLoadState = committedAppProfile.toAppSyncProfileLoadState()
        val secureStorageWarning = withContext(Dispatchers.IO) { repo.getSecureStorageWarning() }
        val builtinThemes = withContext(Dispatchers.IO) { repo.listBuiltinThemes() }
        val paletteRecords = withContext(Dispatchers.IO) { repo.listPaletteRecords() }
        val aiAvailable = withContext(Dispatchers.IO) { repo.aiAvailable() }
        val dataRootPath = withContext(Dispatchers.IO) { repo.dataRootDir() }
        _uiState.update {
            SettingsUiState(
                settings = mergeLoadedLocal(current.settings, settings),
                fontSize = mergeLoadedFontSize(current.fontSize, fontSize),
                projectSyncConfig =
                    mergeLoadedProjectSyncConfig(
                        current.projectSyncConfig,
                        projectSyncProfileLoadState,
                    ),
                projectSyncSecrets =
                    mergeLoadedProjectSyncSecrets(
                        current.projectSyncSecrets,
                        projectSyncProfileLoadState,
                    ),
                projectSyncCapability = projectSyncCapability,
                projectSyncProfileLoadState = projectSyncProfileLoadState,
                projectDryRunState = current.projectDryRunState,
                projectTestConnectionState = current.projectTestConnectionState,
                projectPerformSyncState = current.projectPerformSyncState,
                projectSyncResult = current.projectSyncResult,
                appSyncConfig = mergeLoadedAppSyncConfig(current.appSyncConfig, appSyncProfileLoadState),
                appSyncSecrets = mergeLoadedAppSyncSecrets(current.appSyncSecrets, appSyncProfileLoadState),
                appSyncProfileLoadState = appSyncProfileLoadState,
                appDryRunState = current.appDryRunState,
                appTestConnectionState = current.appTestConnectionState,
                appPerformSyncState = current.appPerformSyncState,
                appSyncResult = current.appSyncResult,
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

private fun SettingsViewModel.mergeLoadedProjectSyncConfig(
    current: com.xiwei.sujian.model.SyncConfig,
    loadState: SyncProfileLoadState,
): com.xiwei.sujian.model.SyncConfig =
    if (!hasUnsavedProjectSyncConfig()) loadState.confirmedConfig ?: current else current

private fun SettingsViewModel.mergeLoadedProjectSyncSecrets(
    current: com.xiwei.sujian.model.SyncSecrets,
    loadState: SyncProfileLoadState,
): com.xiwei.sujian.model.SyncSecrets =
    if (!hasUnsavedProjectSyncSecrets()) loadState.confirmedSecrets ?: current else current

private fun SettingsViewModel.mergeLoadedAppSyncConfig(
    current: com.xiwei.sujian.model.SyncConfig,
    loadState: SyncProfileLoadState,
): com.xiwei.sujian.model.SyncConfig = if (!hasUnsavedAppSyncConfig()) loadState.confirmedConfig ?: current else current

private fun SettingsViewModel.mergeLoadedAppSyncSecrets(
    current: com.xiwei.sujian.model.SyncSecrets,
    loadState: SyncProfileLoadState,
): com.xiwei.sujian.model.SyncSecrets =
    if (!hasUnsavedAppSyncSecrets()) loadState.confirmedSecrets ?: current else current
