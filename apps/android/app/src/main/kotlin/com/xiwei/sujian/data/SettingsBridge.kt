package com.xiwei.sujian.data

import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.model.SyncableSettings
import uniffi.writer_core.WriterException

/**
 * 设置 领域 Bridge。
 *
 * 从 AppServiceBridge 拆出，负责本地设置和可同步设置相关操作。
 */
class SettingsBridge internal constructor(private val holder: WriterAppServiceHolder) {
    companion object {
        private const val TAG = "SettingsBridge"
    }

    fun loadLocalSettings(): BridgeResult<LocalSettings> = holder.wrapResult {
        holder.service.loadLocalSettings().toModel()
    }

    fun getLocalSettings(): BridgeResult<LocalSettings> = loadLocalSettings()

    fun saveLocalSettings(settings: LocalSettings): BridgeResult<Boolean> {
        return try {
            val res = holder.service.saveLocalSettings(settings.toDto())
            BridgeResult.Success(res, ResultEnvelope(success = true, data = res, changedEntities = listOf(ChangedEntity("SettingsSaved"))))
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library is not loaded", e)
            BridgeResult.NotLoaded
        } catch (e: WriterException) {
            DiagnosticsLogger.e(TAG, "Native exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error(e.toWireErrorCode(), e.message ?: "Unknown native exception"))
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error("UNKNOWN", e.message ?: "Unknown error"))
        }
    }

    fun loadSyncableSettings(): BridgeResult<SyncableSettings> = holder.wrapResult {
        holder.service.loadSyncableSettings().toModel()
    }

    fun getSyncableSettings(): BridgeResult<SyncableSettings> = loadSyncableSettings()

    fun saveSyncableSettings(settings: SyncableSettings): BridgeResult<Boolean> {
        return try {
            val res = holder.service.saveSyncableSettings(settings.toDto())
            BridgeResult.Success(res, ResultEnvelope(success = true, data = res, changedEntities = listOf(ChangedEntity("SettingsSaved"))))
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library is not loaded", e)
            BridgeResult.NotLoaded
        } catch (e: WriterException) {
            DiagnosticsLogger.e(TAG, "Native exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error(e.toWireErrorCode(), e.message ?: "Unknown native exception"))
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Exception: ${e.message}", e)
            BridgeResult.Error(ResultEnvelope.error("UNKNOWN", e.message ?: "Unknown error"))
        }
    }

    fun ensureDeviceInfo(platform: String, deviceClass: String): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.ensureDeviceInfo(platform, deviceClass)
    }

    fun loadDeviceInfo(): BridgeResult<com.xiwei.sujian.model.DeviceInfo> = holder.wrapResult {
        val dto = holder.service.loadDeviceInfo()
        com.xiwei.sujian.model.DeviceInfo(
            deviceId = dto.deviceId,
            deviceClass = dto.deviceClass,
            platform = dto.platform,
        )
    }

    fun savePaletteRecord(record: uniffi.writer_core.ThemePaletteRecordDto): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.savePaletteRecord(record)
    }

    fun listPaletteRecords(): BridgeResult<List<uniffi.writer_core.ThemePaletteRecordDto>> = holder.wrapResult {
        holder.service.listPaletteRecords()
    }

    fun loadPaletteRecord(deviceId: String, fingerprint: String): BridgeResult<uniffi.writer_core.ThemePaletteRecordDto> = holder.wrapResult {
        holder.service.loadPaletteRecord(deviceId, fingerprint)
    }

    fun deletePaletteRecord(deviceId: String, fingerprint: String): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.deletePaletteRecord(deviceId, fingerprint)
    }

    fun migrateLegacyThemePalette(): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.migrateLegacyThemePalette()
    }

    fun computePaletteFingerprint(
        lightScheme: uniffi.writer_core.ThemeColorSchemeDto,
        darkScheme: uniffi.writer_core.ThemeColorSchemeDto
    ): String {
        return holder.service.computePaletteFingerprint(lightScheme, darkScheme)
    }

    fun listBuiltinThemes(): List<uniffi.writer_core.BuiltinThemeDto> {
        return holder.service.listBuiltinThemes()
    }
}
