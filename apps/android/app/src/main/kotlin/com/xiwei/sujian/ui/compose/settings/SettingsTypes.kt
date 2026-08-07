package com.xiwei.sujian.ui.compose.settings

// ! # 设置页类型声明（从 SettingsRoute 拆分）

import com.xiwei.sujian.data.SyncFailureKind
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.model.SyncTrigger

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
        val config: com.xiwei.sujian.model.SyncConfig,
        val secrets: com.xiwei.sujian.model.SyncSecrets,
    ) : SyncProfileLoadState

    data class Unconfigured(
        val config: com.xiwei.sujian.model.SyncConfig,
        val secrets: com.xiwei.sujian.model.SyncSecrets,
    ) : SyncProfileLoadState

    data class Failed(val kind: SyncFailureKind, val message: String?) : SyncProfileLoadState
}

/** 已确认（非失败）状态下可用的 config — Failed/Loading 返回 null，保留当前 UI 值。 */
internal val SyncProfileLoadState.confirmedConfig: com.xiwei.sujian.model.SyncConfig?
    get() =
        when (this) {
            is SyncProfileLoadState.Ready -> config
            is SyncProfileLoadState.Unconfigured -> config
            is SyncProfileLoadState.Failed -> null
            SyncProfileLoadState.Loading -> null
        }

/** 已确认（非失败）状态下可用的 secrets — Failed/Loading 返回 null，保留当前 UI 值。 */
internal val SyncProfileLoadState.confirmedSecrets: com.xiwei.sujian.model.SyncSecrets?
    get() =
        when (this) {
            is SyncProfileLoadState.Ready -> secrets
            is SyncProfileLoadState.Unconfigured -> secrets
            is SyncProfileLoadState.Failed -> null
            SyncProfileLoadState.Loading -> null
        }

/** #595 四：SyncProfileReadResult → 设置页加载状态 — Failed 不再被静默转成 null。 */
internal fun com.xiwei.sujian.data.SyncProfileReadResult.toSyncProfileLoadState(): SyncProfileLoadState =
    when (this) {
        is com.xiwei.sujian.data.SyncProfileReadResult.Found ->
            SyncProfileLoadState.Ready(snapshot.config, snapshot.secrets)
        is com.xiwei.sujian.data.SyncProfileReadResult.NotConfigured ->
            SyncProfileLoadState.Unconfigured(snapshot.config, snapshot.secrets)
        is com.xiwei.sujian.data.SyncProfileReadResult.Failed ->
            SyncProfileLoadState.Failed(kind, message)
    }

data class SettingsUiState(
    val settings: LocalSettings = LocalSettings(),
    val fontSize: Float = 16f,
    val syncConfig: com.xiwei.sujian.model.SyncConfig = com.xiwei.sujian.model.SyncConfig(),
    val syncSecrets: com.xiwei.sujian.model.SyncSecrets = com.xiwei.sujian.model.SyncSecrets(),
    val syncCapability: com.xiwei.sujian.model.SyncCapabilityData = com.xiwei.sujian.model.SyncCapabilityData(),
    val secureStorageWarning: String? = null,
    val builtinThemes: List<com.xiwei.sujian.model.BuiltinTheme> = emptyList(),
    val paletteRecords: List<com.xiwei.sujian.model.ThemePaletteRecord> = emptyList(),
    val aiAvailable: Boolean = false,
    val dataRootPath: String = "",
    val versionInfo: String = "",
    val saveErrorResId: Int? = null,
    val dryRunState: SyncCommandState = SyncCommandState.IDLE,
    val testConnectionState: SyncCommandState = SyncCommandState.IDLE,
    val performSyncState: SyncCommandState = SyncCommandState.IDLE,
    val structuredSyncResult: StructuredSyncResult? = null,
    val lastCommandType: SyncCommandType? = null,
/** #595 四：同步 profile 加载状态 — Failed 时保留字段值并显示真实错误。 */
    val syncProfileLoadState: SyncProfileLoadState = SyncProfileLoadState.Loading,
)

sealed interface SettingsIntent {
    data class UpdateLocal(val transform: (LocalSettings) -> LocalSettings) : SettingsIntent

    data class UpdateFontSize(val fontSize: Float) : SettingsIntent

    data class UpdateSyncConfig(val config: com.xiwei.sujian.model.SyncConfig) : SettingsIntent

    data class UpdateSyncSecrets(val secrets: com.xiwei.sujian.model.SyncSecrets) : SettingsIntent

    data object Refresh : SettingsIntent

    data object CaptureDynamicColor : SettingsIntent

    data class DeletePalette(val deviceId: String, val fingerprint: String) : SettingsIntent

    data object DryRun : SettingsIntent

    data object TestConnection : SettingsIntent

    data object PerformSync : SettingsIntent
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

    data class SyncConfig(
        val config: com.xiwei.sujian.model.SyncConfig,
        val revision: Long,
    ) : SettingsSaveCommand

    data class SyncSecrets(
        val secrets: com.xiwei.sujian.model.SyncSecrets,
        val revision: Long,
    ) : SettingsSaveCommand
}

sealed interface SettingsTransactionCommand {
    val config: com.xiwei.sujian.model.SyncConfig
    val configRevision: Long
    val secrets: com.xiwei.sujian.model.SyncSecrets
    val secretsRevision: Long

    data class SaveAndRunSync(
        override val config: com.xiwei.sujian.model.SyncConfig,
        override val configRevision: Long,
        override val secrets: com.xiwei.sujian.model.SyncSecrets,
        override val secretsRevision: Long,
        val trigger: SyncTrigger,
    ) : SettingsTransactionCommand

    data class SaveAndRunDryRun(
        override val config: com.xiwei.sujian.model.SyncConfig,
        override val configRevision: Long,
        override val secrets: com.xiwei.sujian.model.SyncSecrets,
        override val secretsRevision: Long,
    ) : SettingsTransactionCommand

    data class SaveAndRunDiagnostics(
        override val config: com.xiwei.sujian.model.SyncConfig,
        override val configRevision: Long,
        override val secrets: com.xiwei.sujian.model.SyncSecrets,
        override val secretsRevision: Long,
    ) : SettingsTransactionCommand
}

internal data class PendingCommands(
    val local: SettingsSaveCommand.Local? = null,
    val fontSize: SettingsSaveCommand.FontSize? = null,
    val syncConfig: SettingsSaveCommand.SyncConfig? = null,
    val syncSecrets: SettingsSaveCommand.SyncSecrets? = null,
) {
    /** 所有待保存命令均为空 — 拆分复杂条件，避免 ComplexCondition。 */
    fun isEmpty(): Boolean = local == null && fontSize == null && syncConfig == null && syncSecrets == null
}

enum class SyncCommandType { DRY_RUN, TEST_CONNECTION, PERFORM_SYNC }

internal data class SyncCommandIoResult(
    val configSaved: Boolean,
    val secretsSaved: Boolean,
    val structuredResult: StructuredSyncResult,
) {
    val isSuccess: Boolean get() = structuredResult.statusCode == "ok"
}
