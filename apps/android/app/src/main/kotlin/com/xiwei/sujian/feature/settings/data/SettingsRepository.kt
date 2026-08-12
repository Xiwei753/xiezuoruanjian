package com.xiwei.sujian.feature.settings.data
import android.content.Context
import androidx.core.content.edit
import com.xiwei.sujian.app.di.AppServiceProvider
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import com.xiwei.sujian.feature.settings.data.model.SyncableSettings
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * SettingsRepository — 设置仓库层
 *
 * 对设置、同步、native 状态领域 Bridge 的封装，提供统一的设置读写接口。
 *
 * ## 架构定位
 * - ViewModel/Activity → SettingsRepository → SettingsBridge/SyncBridge → legacy internal adapter → JNI → Rust Core
 *
 * ## 职责边界
 * - **做**：加载/保存本地设置、可同步设置、同步配置和密钥
 * - **不做**：业务逻辑（只做类型转换和错误处理）
 *
 * ## 使用场景
 * - EditorViewModel 加载编辑器设置
 * - Compose SettingsRoute 保存用户设置
 * - SyncPage 加载/保存同步配置
 */
class SettingsRepository(
    context: Context,
    bridge: AppServiceBridge? = null,
    preferencesSuffix: String = "",
) {
    private val appContext = context.applicationContext
    private val appBridge = bridge ?: AppServiceProvider.getAppServiceBridge(context)
    private val settingsBridge = appBridge.settingsBridge
    private val statsBridge = appBridge.statsBridge
    private val diagPrefs =
        appContext.getSharedPreferences(
            if (preferencesSuffix.isNotEmpty()) "sujian_diagnostics_$preferencesSuffix" else "sujian_diagnostics",
            android.content.Context.MODE_PRIVATE,
        )

    // #617 评论四：本地设置的可观察状态 — 系统栏执行层（ImmersiveSystemBarsEffect）
    // 只认这一份状态，实验设置写入后立即反映到窗口能力。
    private val _localSettingsState = MutableStateFlow(LocalSettings())
    val localSettingsState: StateFlow<LocalSettings> = _localSettingsState.asStateFlow()

    @Volatile
    var lastWarning: String? = null
        private set

    fun consumeWarning(): String? {
        val w = lastWarning
        lastWarning = null
        return w
    }

    private fun warn(msg: String) {
        lastWarning = msg
        DiagnosticsLogger.w("SettingsRepository", msg)
    }

    fun getLocalSettings(): LocalSettings {
        val fromCore =
            when (val result = settingsBridge.getLocalSettings()) {
                is BridgeResult.Success -> result.data ?: LocalSettings()
                is BridgeResult.Error -> {
                    warn("Failed to load local settings: ${result.fullEnvelope}")
                    LocalSettings()
                }
                BridgeResult.NotLoaded -> LocalSettings()
            }
        val resolved =
            fromCore.copy(
                diagnosticsEnabled = diagPrefs.getBoolean("diagnostics_enabled", true),
                diagnosticsVerbose = diagPrefs.getBoolean("diagnostics_verbose", true),
                useSelfRenderEditorOnAndroid = diagPrefs.getBoolean("use_self_render_editor_on_android", true),
                experimentalFullscreenMode = diagPrefs.getBoolean("experimental_fullscreen_mode", false),
            )
        _localSettingsState.value = resolved
        return resolved
    }

    fun saveLocalSettings(settings: LocalSettings): SettingsSaveResult {
        val coreSettings =
            settings.copy(
                diagnosticsEnabled = false,
                diagnosticsVerbose = false,
                useSelfRenderEditorOnAndroid = false,
                experimentalFullscreenMode = false,
            )
        return when (val result = settingsBridge.saveLocalSettings(coreSettings)) {
            is BridgeResult.Success -> {
                com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.settingsSaved("local_settings", "ok")
                val effectiveVerbose = if (settings.diagnosticsEnabled) settings.diagnosticsVerbose else false
                val resolved = settings.copy(diagnosticsVerbose = effectiveVerbose)
                diagPrefs.edit {
                    putBoolean("diagnostics_enabled", settings.diagnosticsEnabled)
                    putBoolean("diagnostics_verbose", effectiveVerbose)
                    putBoolean("use_self_render_editor_on_android", settings.useSelfRenderEditorOnAndroid)
                    putBoolean("experimental_fullscreen_mode", settings.experimentalFullscreenMode)
                }
                // #617 评论四：保存成功后同步可观察状态，实验开关立即驱动窗口能力。
                _localSettingsState.value = resolved
                CoreSettingsEvents.record(result.envelope)
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save local settings: ${result.fullEnvelope}")
                com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.settingsSaved("local_settings", "error")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.LOCAL_SETTINGS, 0L)))
            }
            BridgeResult.NotLoaded -> {
                com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.settingsSaved("local_settings", "not_loaded")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.LOCAL_SETTINGS, 0L)))
            }
        }
    }

    fun getSyncableSettings(): SyncableSettings {
        return when (val result = settingsBridge.getSyncableSettings()) {
            is BridgeResult.Success -> result.data ?: SyncableSettings()
            is BridgeResult.Error -> {
                warn("Failed to load syncable settings: ${result.fullEnvelope}")
                val defaultSettings = SyncableSettings()
                defaultSettings
            }
            BridgeResult.NotLoaded -> SyncableSettings()
        }
    }

    fun saveSyncableSettings(settings: SyncableSettings): SettingsSaveResult {
        return when (val result = settingsBridge.saveSyncableSettings(settings)) {
            is BridgeResult.Success -> {
                CoreSettingsEvents.record(result.envelope)
                com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.settingsSaved("font_size", "ok")
                SettingsSaveResult.Success
            }
            is BridgeResult.Error -> {
                warn("Failed to save syncable settings: ${result.fullEnvelope}")
                com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.settingsSaved("font_size", "error")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.FONT_SIZE, 0L)))
            }
            BridgeResult.NotLoaded -> {
                com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.settingsSaved("font_size", "not_loaded")
                SettingsSaveResult.Failed(listOf(SaveFailure(SaveField.FONT_SIZE, 0L)))
            }
        }
    }

    fun getEffectiveFontSize(): Float {
        val syncable = getSyncableSettings()
        if (syncable.fontSize > 0.0) {
            return syncable.fontSize.toFloat()
        }
        val local = getLocalSettings()
        if (local.editorFontSize > 0.0f) {
            return local.editorFontSize
        }
        return 16f
    }

    fun setFontSize(fontSize: Float): SettingsSaveResult {
        val syncable = getSyncableSettings()
        return saveSyncableSettings(syncable.copy(fontSize = fontSize.toDouble()))
    }

    /**
     * #600 评论 #7: 外部同步拉取后通知设置已变化 — 复用 CoreSettingsEvents 事件总线,
     * 让 SettingsViewModel / ThemeController 等监听方重新从 Core 读取最新设置。
     */
    fun notifySyncableSettingsChangedExternally() {
        CoreSettingsEvents.markEditorChanged()
    }

    /**
     * #600 评论 #7: 外部同步拉取后通知主题调色板已变化 — 复用 CoreSettingsEvents 事件总线,
     * 让 ThemeController 重新加载 palette catalog。
     */
    fun notifyPaletteCatalogChangedExternally() {
        CoreSettingsEvents.markEditorChanged()
    }

    fun aiAvailable(): Boolean {
        return AppServiceProvider.getAppServiceBridge(appContext).aiAvailable()
    }

    fun getSecureStorageWarning(): String? {
        return settingsBridge.getSecureStorageWarning()
    }

    fun dismissMigrationWarning() {
        settingsBridge.dismissMigrationWarning()
    }
}
