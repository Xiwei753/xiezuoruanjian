package com.xiwei.sujian.feature.settings.ui

import com.xiwei.sujian.feature.settings.data.model.LocalSettings
import com.xiwei.sujian.feature.sync.data.SyncFailureKind
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncTrigger

// ! # 设置页类型声明（从 SettingsRoute 拆分）
//
// #630 评论 #1+#2：同步配置只有一份 — 全量同步覆盖设置/星图/主题/全部作品，
// 不再区分作品级与应用级两套 UI 状态/Intent/SaveCommand。

// #630 评论16：同步配置/凭据 Intent 用 transform 函数，每个 row 不需要抓住整份 config。
typealias SyncConfigTransform = SyncConfig.() -> SyncConfig

typealias SyncSecretsTransform = SyncSecrets.() -> SyncSecrets

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
    // 同步（全量：设置/星图/主题/全部作品，单一一份）
    val syncConfig: com.xiwei.sujian.feature.sync.data.model.SyncConfig =
        com.xiwei.sujian.feature.sync.data.model.SyncConfig(),
    val syncSecrets: com.xiwei.sujian.feature.sync.data.model.SyncSecrets =
        com.xiwei.sujian.feature.sync.data.model.SyncSecrets(),
    val syncCapability: SyncCapabilityData = SyncCapabilityData(),
    val syncProfileLoadState: SyncProfileLoadState = SyncProfileLoadState.Loading,
    val dryRunState: SyncCommandState = SyncCommandState.IDLE,
    val testConnectionState: SyncCommandState = SyncCommandState.IDLE,
    val performSyncState: SyncCommandState = SyncCommandState.IDLE,
    val syncResult: StructuredSyncResult? = null,
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

    data class UpdateSyncConfig(
        val transform: SyncConfigTransform,
    ) : SettingsIntent

    data class UpdateSyncSecrets(
        val transform: SyncSecretsTransform,
    ) : SettingsIntent

    data object Refresh : SettingsIntent

    data object CaptureDynamicColor : SettingsIntent

    data class DeletePalette(val deviceId: String, val fingerprint: String) : SettingsIntent

    data object DryRun : SettingsIntent

    data object TestConnection : SettingsIntent

    data object PerformSync : SettingsIntent
}

sealed interface SettingsSaveCommand {
    // #617 评论三：本地保存命令携带 affectsTheme — 只有真正影响主题的字段
    // （外观模式/颜色来源/动态色/内置主题/调色板）变化时才需要重建主题；
    // 自动保存、诊断、编辑器选项、沉浸式全屏等普通设置不再触发 ThemeStore.reload()。
    // #630 评论二：affectsEditor 标记真正影响正文运行时的字段（字号 fallback/行距/
    // 首行缩进开关与宽度/文字动画开关与时长/光标动画开关与时长/协同动画/Android 自渲染
    // 编辑器开关）变化 — 只有这类保存才通知编辑器重读设置；自动保存、AI、诊断、
    // 沉浸式全屏、主题颜色等普通保存不再触发编辑器重载。默认 false 以减少对其它构造点的破坏。
    data class Local(
        val settings: LocalSettings,
        val revision: Long,
        val affectsTheme: Boolean,
        val affectsEditor: Boolean = false,
    ) : SettingsSaveCommand

    data class FontSize(
        val fontSize: Float,
        val revision: Long,
    ) : SettingsSaveCommand

    data class SyncConfig(
        val config: com.xiwei.sujian.feature.sync.data.model.SyncConfig,
        val revision: Long,
    ) : SettingsSaveCommand

    data class SyncSecrets(
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
}

internal data class PendingCommands(
    val local: SettingsSaveCommand.Local? = null,
    val fontSize: SettingsSaveCommand.FontSize? = null,
    val syncConfig: SettingsSaveCommand.SyncConfig? = null,
    val syncSecrets: SettingsSaveCommand.SyncSecrets? = null,
) {
    /** 所有待保存命令均为空 — 拆分复杂条件，避免 ComplexCondition。 */
    fun isEmpty(): Boolean =
        local == null &&
            fontSize == null &&
            syncConfig == null &&
            syncSecrets == null
}

enum class SyncCommandType {
    DRY_RUN,
    TEST_CONNECTION,
    PERFORM_SYNC,
}

internal data class SyncCommandIoResult(
    val configSaved: Boolean,
    val secretsSaved: Boolean,
    val structuredResult: StructuredSyncResult,
) {
    val isSuccess: Boolean get() = structuredResult.statusCode == "ok"
}
