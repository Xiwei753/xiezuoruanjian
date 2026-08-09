package com.xiwei.sujian.feature.settings.ui

import com.xiwei.sujian.feature.sync.data.model.SyncFailureKind
import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger

// ! # 设置页类型声明（从 SettingsRoute 拆分）

enum class SyncCommandState { IDLE, RUNNING, SUCCESS, FAILURE }

/**
 * #595 四：设置页同步 profile 加载状态。
 *
 * - [Loading]：初始加载中。
 * - [Ready]：config + secrets 均读取成功且凭据非空。
 * - [Unconfigured]：config 读取成功但未配置 token — 显示空 token 是正常状态。
 * - [Failed]：安全存储读取失败/原生库未加载/配置损坏 — 保留上一次已确认的字段值，
 *   同时显示真实错误，不再通过 toConfigSecretsOrNull() 静默退化为默认值。
 */
sealed interface SyncProfileLoadState {
    data object Loading : SyncProfileLoadState

    data class Ready(
        val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
    ) : SyncProfileLoadState

    data class Unconfigured(
        val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
    ) : SyncProfileLoadState

    data class Failed(val kind: SyncFailureKind, val message: String?) : SyncProfileLoadState
}

/** 已确认（非失败）状态下可用的 config — Failed/Loading 返回 null，保留当前 UI 值。 */
internal val SyncProfileLoadState.confirmedConfig: com.xiwei.sujian.feature.sync.data.model.SyncConfig?
    get() =
        when (this) {
            is SyncProfileLoadState.Ready -> config
            is SyncProfileLoadState.Unconfigured -> config
            is SyncProfileLoadState.Failed -> null
            SyncProfileLoadState.Loading -> null
        }

/** 已确认（非失败）状态下可用的 secrets — Failed/Loading 返回 null，保留当前 UI 值。 */
internal val SyncProfileLoadState.confirmedSecrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets?
    get() =
        when (this) {
            is SyncProfileLoadState.Ready -> secrets
            is SyncProfileLoadState.Unconfigured -> secrets
            is SyncProfileLoadState.Failed -> null
            SyncProfileLoadState.Loading -> null
        }

/** #595 四：SyncProfileReadResult → 设置页加载状态 — Failed 不再被静默转成 null。 */
internal fun com.xiwei.sujian.feature.sync.data.SyncProfileReadResult.toSyncProfileLoadState(): SyncProfileLoadState =
    when (this) {
        is com.xiwei.sujian.feature.sync.data.SyncProfileReadResult.Found ->
            SyncProfileLoadState.Ready(snapshot.config, snapshot.secrets)
        is com.xiwei.sujian.feature.sync.data.SyncProfileReadResult.NotConfigured ->
            SyncProfileLoadState.Unconfigured(snapshot.config, snapshot.secrets)
        is com.xiwei.sujian.feature.sync.data.SyncProfileReadResult.Failed ->
            SyncProfileLoadState.Failed(kind, message)
    }

data class SettingsUiState(
    val settings: LocalSettings = LocalSettings(),
    val fontSize: Float = 16f,
    // 作品级同步
    val projectSyncConfig: com.xiwei.sujian.feature.sync.data.model.SyncConfig =
        com.xiwei.sujian.feature.sync.data.model.SyncConfig(),
    val projectSyncSecrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets =
        com.xiwei.sujian.feature.sync.data.model.SyncSecrets(),
    val projectSyncCapability: SyncCapabilityData = SyncCapabilityData(),
    val projectSyncProfileLoadState: SyncProfileLoadState = SyncProfileLoadState.Loading,
    val projectDryRunState: SyncCommandState = SyncCommandState.IDLE,
    val projectTestConnectionState: SyncCommandState = SyncCommandState.IDLE,
    val projectPerformSyncState: SyncCommandState = SyncCommandState.IDLE,
    val projectSyncResult: StructuredSyncResult? = null,
    // 应用级同步
    val appSyncConfig: com.xiwei.sujian.feature.sync.data.model.SyncConfig =
        com.xiwei.sujian.feature.sync.data.model.SyncConfig(),
    val appSyncSecrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets =
        com.xiwei.sujian.feature.sync.data.model.SyncSecrets(),
    val appSyncProfileLoadState: SyncProfileLoadState = SyncProfileLoadState.Loading,
    val appDryRunState: SyncCommandState = SyncCommandState.IDLE,
    val appTestConnectionState: SyncCommandState = SyncCommandState.IDLE,
    val appPerformSyncState: SyncCommandState = SyncCommandState.IDLE,
    val appSyncResult: StructuredSyncResult? = null,
    // 共用
    val secureStorageWarning: String? = null,
    val builtinThemes: List<com.xiwei.sujian.app.theme.model.BuiltinTheme> = emptyList(),
    val paletteRecords: List<com.xiwei.sujian.app.theme.model.ThemePaletteRecord> = emptyList(),
    val aiAvailable: Boolean = false,
    val dataRootPath: String = "",
    val versionInfo: String = "",
    val saveErrorResId: Int? = null,
    val lastCommandType: SyncCommandType? = null,
)

sealed interface SettingsIntent {
    data class UpdateLocal(val transform: (LocalSettings) -> LocalSettings) : SettingsIntent

    data class UpdateFontSize(val fontSize: Float) : SettingsIntent

    // 作品级同步
    data class UpdateProjectSyncConfig(val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig) : SettingsIntent

    data class UpdateProjectSyncSecrets(
        val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
    ) : SettingsIntent

    // 应用级同步
    data class UpdateAppSyncConfig(val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig) : SettingsIntent

    data class UpdateAppSyncSecrets(val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets) : SettingsIntent

    data object Refresh : SettingsIntent

    data object CaptureDynamicColor : SettingsIntent

    data class DeletePalette(val deviceId: String, val fingerprint: String) : SettingsIntent

    // 作品级命令
    data object DryRun : SettingsIntent

    data object TestConnection : SettingsIntent

    data object PerformSync : SettingsIntent

    // 应用级命令
    data object AppDryRun : SettingsIntent

    data object AppTestConnection : SettingsIntent

    data object AppPerformSync : SettingsIntent
}

sealed interface SettingsSaveCommand {
    data class Local(
        val settings: LocalSettings,
        val revision: Long,
    ) : SettingsSaveCommand

    data class FontSize(
        val fontSize: Float,
        val revision: Long,
    ) : SettingsSaveCommand

    data class ProjectSyncConfig(
        val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        val revision: Long,
    ) : SettingsSaveCommand

    data class ProjectSyncSecrets(
        val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
        val revision: Long,
    ) : SettingsSaveCommand

    data class AppSyncConfig(
        val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        val revision: Long,
    ) : SettingsSaveCommand

    data class AppSyncSecrets(
        val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
        val revision: Long,
    ) : SettingsSaveCommand
}

sealed interface SettingsTransactionCommand {
    val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig
    val configRevision: Long
    val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets
    val secretsRevision: Long

    data class SaveAndRunSync(
        override val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        override val configRevision: Long,
        override val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
        override val secretsRevision: Long,
        val trigger: SyncTrigger,
    ) : SettingsTransactionCommand

    data class SaveAndRunDryRun(
        override val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        override val configRevision: Long,
        override val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
        override val secretsRevision: Long,
    ) : SettingsTransactionCommand

    data class SaveAndRunDiagnostics(
        override val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        override val configRevision: Long,
        override val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
        override val secretsRevision: Long,
    ) : SettingsTransactionCommand

    // #600 评论 #4 问题三：应用级同步事务命令 — 设置/全局星图/主题调色板。
    // 与作品级事务对称，但提交到应用级 profile（commitAppSyncProfile），
    // 执行应用级同步 API（runAppSync / performAppSyncDryRun / performAppSyncDiagnostics）。
    data class SaveAndRunAppSync(
        override val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        override val configRevision: Long,
        override val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
        override val secretsRevision: Long,
        val trigger: SyncTrigger,
    ) : SettingsTransactionCommand

    data class SaveAndRunAppDryRun(
        override val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        override val configRevision: Long,
        override val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
        override val secretsRevision: Long,
    ) : SettingsTransactionCommand

    data class SaveAndRunAppDiagnostics(
        override val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        override val configRevision: Long,
        override val secrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets,
        override val secretsRevision: Long,
    ) : SettingsTransactionCommand
}

internal data class PendingCommands(
    val local: SettingsSaveCommand.Local? = null,
    val fontSize: SettingsSaveCommand.FontSize? = null,
    val projectSyncConfig: SettingsSaveCommand.ProjectSyncConfig? = null,
    val projectSyncSecrets: SettingsSaveCommand.ProjectSyncSecrets? = null,
    val appSyncConfig: SettingsSaveCommand.AppSyncConfig? = null,
    val appSyncSecrets: SettingsSaveCommand.AppSyncSecrets? = null,
) {
    /** 所有待保存命令均为空 — 拆分复杂条件，避免 ComplexCondition。 */
    fun isEmpty(): Boolean =
        local == null &&
            fontSize == null &&
            projectSyncConfig == null &&
            projectSyncSecrets == null &&
            appSyncConfig == null &&
            appSyncSecrets == null
}

enum class SyncCommandType {
    DRY_RUN,
    TEST_CONNECTION,
    PERFORM_SYNC,
    DRY_RUN_APP,
    TEST_CONNECTION_APP,
    PERFORM_APP_SYNC,
}

internal data class SyncCommandIoResult(
    val configSaved: Boolean,
    val secretsSaved: Boolean,
    val structuredResult: StructuredSyncResult,
) {
    val isSuccess: Boolean get() = structuredResult.statusCode == "ok"
}
