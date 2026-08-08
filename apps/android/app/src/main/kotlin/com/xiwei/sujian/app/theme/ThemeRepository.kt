package com.xiwei.sujian.app.theme

import android.content.Context
import com.xiwei.sujian.app.di.AppServiceProvider
import com.xiwei.sujian.app.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.app.theme.model.BuiltinTheme
import com.xiwei.sujian.app.theme.model.DeviceInfo
import com.xiwei.sujian.app.theme.model.ThemeColorScheme
import com.xiwei.sujian.app.theme.model.ThemePaletteRecord
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.ThemeDtoMapper

/**
 * ThemeRepository — 主题仓库层（#602 Phase 7 从 SettingsRepository 拆分）。
 *
 * 集中所有主题相关读写：设备信息、内置主题、调色板记录、动态色保存与设备类型识别。
 * 设置同步通知（notifyPaletteCatalogChangedExternally）仍保留在 SettingsRepository，
 * 因为它属于设置同步事件总线，不属于主题读写。
 *
 * ## 架构定位
 * - ThemeStore / ThemeController / SettingsViewModel → ThemeRepository → SettingsBridge → JNI → Rust Core
 *
 * ## 职责边界
 * - **做**：读写设备信息、内置主题、调色板记录，识别设备类型，保存动态色到 catalog
 * - **不做**：设置同步通知（在 SettingsRepository）、UI 状态（在 ThemeStore）
 */
class ThemeRepository(
    context: Context,
    bridge: AppServiceBridge? = null,
) {
    private val appContext = context.applicationContext
    private val appBridge = bridge ?: AppServiceProvider.getAppServiceBridge(context)
    private val settingsBridge = appBridge.settingsBridge
    private val defaultDeviceClass = "phone"

    /**
     * 确保设备信息已写入 app-meta/device/current_device.json。
     * 通过 Core 层 ensure_device_info 实现，不依赖 SharedPreferences。
     */
    fun ensureDeviceInfo(
        platform: String,
        deviceClass: String,
    ): Boolean {
        return when (val result = settingsBridge.ensureDeviceInfo(platform, deviceClass)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to write device info: ${result.fullEnvelope}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun loadDeviceInfo(): DeviceInfo {
        return when (val result = settingsBridge.loadDeviceInfo()) {
            is BridgeResult.Success -> result.data
            else -> DeviceInfo()
        }
    }

    fun listBuiltinThemes(): List<BuiltinTheme> {
        return when (val result = settingsBridge.listBuiltinThemes()) {
            is BridgeResult.Success -> (result.data ?: emptyList()).map { ThemeDtoMapper.fromDto(it) }
            is BridgeResult.Error -> {
                warn("Failed to list builtin themes: ${result.fullEnvelope}")
                emptyList()
            }
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    fun listPaletteRecords(): List<ThemePaletteRecord> {
        return when (val result = settingsBridge.listPaletteRecords()) {
            is BridgeResult.Success -> (result.data ?: emptyList()).map { ThemeDtoMapper.fromDto(it) }
            is BridgeResult.Error -> {
                warn("Failed to list palette records: ${result.fullEnvelope}")
                emptyList()
            }
            BridgeResult.NotLoaded -> emptyList()
        }
    }

    fun loadPaletteRecord(
        deviceId: String,
        fingerprint: String,
    ): ThemePaletteRecord? {
        return when (val result = settingsBridge.loadPaletteRecord(deviceId, fingerprint)) {
            is BridgeResult.Success -> result.data?.let { ThemeDtoMapper.fromDto(it) }
            is BridgeResult.Error -> {
                warn("Failed to load palette record: ${result.fullEnvelope}")
                null
            }
            BridgeResult.NotLoaded -> null
        }
    }

    fun deletePaletteRecord(
        deviceId: String,
        fingerprint: String,
    ): Boolean {
        return when (val result = settingsBridge.deletePaletteRecord(deviceId, fingerprint)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> {
                warn("Failed to delete palette record: ${result.fullEnvelope}")
                false
            }
            BridgeResult.NotLoaded -> false
        }
    }

    fun saveDynamicColorPaletteToCatalog(
        lightScheme: ThemeColorScheme,
        darkScheme: ThemeColorScheme,
        deviceClass: String? = null,
    ) {
        try {
            val lightDto = ThemeDtoMapper.toDto(lightScheme)
            val darkDto = ThemeDtoMapper.toDto(darkScheme)
            val deviceInfo = loadDeviceInfo()
            val deviceId = deviceInfo.deviceId.ifEmpty { "legacy" }
            val effectiveDeviceClass =
                deviceClass
                    ?: deviceInfo.deviceClass.ifEmpty { detectDeviceClass() }

            val fingerprint =
                when (val r = settingsBridge.computePaletteFingerprint(lightDto, darkDto)) {
                    is BridgeResult.Success -> r.data
                    is BridgeResult.Error -> {
                        warn("Failed to compute palette fingerprint: ${r.fullEnvelope}")
                        return
                    }
                    BridgeResult.NotLoaded -> return
                }
            val paletteId = "$deviceId:$fingerprint"

            val record =
                uniffi.writer_core.ThemePaletteRecordDto(
                    schemaVersion = 1u,
                    paletteId = paletteId,
                    paletteFingerprint = fingerprint,
                    source = "android_dynamic_color",
                    sourcePlatform = "android",
                    sourceDeviceId = deviceId,
                    sourceDeviceClass = effectiveDeviceClass,
                    capturedAtMs = System.currentTimeMillis(),
                    variant = "system_selected",
                    lightScheme = lightDto,
                    darkScheme = darkDto,
                )

            when (val saveResult = settingsBridge.savePaletteRecord(record)) {
                is BridgeResult.Error -> warn("Failed to save palette record: ${saveResult.fullEnvelope}")
                BridgeResult.NotLoaded -> warn("Native library not loaded, cannot save palette record")
                is BridgeResult.Success -> {}
            }
        } catch (e: Exception) {
            DiagnosticsLogger.w("ThemeRepository", "Failed to save palette to catalog", e)
        }
    }

    private fun detectDeviceClass(): String {
        val config = appContext.resources?.configuration ?: return defaultDeviceClass
        val smallestWidthDp = config.smallestScreenWidthDp
        return when {
            smallestWidthDp >= 600 -> "tablet"
            else -> defaultDeviceClass
        }
    }

    fun detectDeviceClassFromFoldFeature(
        hasFoldFeature: Boolean,
        smallestWidthDp: Int,
    ): String {
        return when {
            hasFoldFeature -> "foldable"
            smallestWidthDp >= 600 -> "tablet"
            else -> defaultDeviceClass
        }
    }

    private fun warn(msg: String) {
        DiagnosticsLogger.w("ThemeRepository", msg)
    }
}
