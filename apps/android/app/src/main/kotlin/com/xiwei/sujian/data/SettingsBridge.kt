package com.xiwei.sujian.data

import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.model.SyncableSettings

/**
 * 设置 领域 Bridge。
 *
 * 从 AppServiceBridge 拆出，负责本地设置和可同步设置相关操作。
 */
class SettingsBridge internal constructor(private val holder: WriterAppServiceHolder) {
    fun loadLocalSettings(): BridgeResult<LocalSettings> =
        holder.wrapResult {
            holder.service.loadLocalSettings().toModel()
        }

    fun getLocalSettings(): BridgeResult<LocalSettings> = loadLocalSettings()

    fun saveLocalSettings(settings: LocalSettings): BridgeResult<Boolean> {
        return when (val result = holder.wrapResult { holder.service.saveLocalSettings(settings.toDto()) }) {
            is BridgeResult.Success ->
                BridgeResult.Success(
                    result.data,
                    ResultEnvelope(
                        success = true,
                        data = result.data,
                        changedEntities = listOf(ChangedEntity("SettingsSaved")),
                    ),
                )
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun loadSyncableSettings(): BridgeResult<SyncableSettings> =
        holder.wrapResult {
            holder.service.loadSyncableSettings().toModel()
        }

    fun getSyncableSettings(): BridgeResult<SyncableSettings> = loadSyncableSettings()

    fun saveSyncableSettings(settings: SyncableSettings): BridgeResult<Boolean> {
        return when (val result = holder.wrapResult { holder.service.saveSyncableSettings(settings.toDto()) }) {
            is BridgeResult.Success ->
                BridgeResult.Success(
                    result.data,
                    ResultEnvelope(
                        success = true,
                        data = result.data,
                        changedEntities = listOf(ChangedEntity("SettingsSaved")),
                    ),
                )
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun ensureDeviceInfo(
        platform: String,
        deviceClass: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.ensureDeviceInfo(platform, deviceClass)
        }

    fun loadDeviceInfo(): BridgeResult<com.xiwei.sujian.model.DeviceInfo> =
        holder.wrapResult {
            val dto = holder.service.loadDeviceInfo()
            com.xiwei.sujian.model.DeviceInfo(
                deviceId = dto.deviceId,
                deviceClass = dto.deviceClass,
                platform = dto.platform,
            )
        }

    fun savePaletteRecord(record: uniffi.writer_core.ThemePaletteRecordDto): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.savePaletteRecord(record)
        }

    fun listPaletteRecords(): BridgeResult<List<uniffi.writer_core.ThemePaletteRecordDto>> =
        holder.wrapResult {
            holder.service.listPaletteRecords()
        }

    fun loadPaletteRecord(
        deviceId: String,
        fingerprint: String,
    ): BridgeResult<uniffi.writer_core.ThemePaletteRecordDto> =
        holder.wrapResult {
            holder.service.loadPaletteRecord(deviceId, fingerprint)
        }

    fun deletePaletteRecord(
        deviceId: String,
        fingerprint: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.deletePaletteRecord(deviceId, fingerprint)
        }

    fun migrateLegacyThemePalette(): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.migrateLegacyThemePalette()
        }

    fun computePaletteFingerprint(
        lightScheme: uniffi.writer_core.ThemeColorSchemeDto,
        darkScheme: uniffi.writer_core.ThemeColorSchemeDto,
    ): BridgeResult<String> =
        holder.wrapResult {
            holder.service.computePaletteFingerprint(lightScheme, darkScheme)
        }

    fun listBuiltinThemes(): BridgeResult<List<uniffi.writer_core.BuiltinThemeDto>> =
        holder.wrapResult {
            holder.service.listBuiltinThemes()
        }

    fun getSecureStorageWarning(): String? {
        return holder.secureStorageWarning
    }

    fun dismissMigrationWarning() {
        holder.dismissMigrationWarning()
    }
}
