package com.xiwei.sujian.app.theme

import android.content.Context
import com.xiwei.sujian.app.theme.interop.LocalThemePaletteStore
import com.xiwei.sujian.app.theme.interop.ThemeDtoMapper
import com.xiwei.sujian.app.theme.interop.ThemePaletteRecordJson
import com.xiwei.sujian.app.theme.model.BuiltinTheme
import com.xiwei.sujian.app.theme.model.DeviceInfo
import com.xiwei.sujian.app.theme.model.ThemeColorScheme
import com.xiwei.sujian.app.theme.model.ThemePaletteRecord
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.diagnostics.DiagnosticsInterop

/**
 * ThemeRepository — 主题仓库层（#602 Phase 7 从 SettingsRepository 拆分）。
 *
 * 集中所有主题相关读写：设备信息、内置主题、设备配色快照、动态色保存与设备类型识别。
 * 设备配色快照只保存在当前 Android 设备（Issue #1005），不经过 Core 跨端 catalog。
 *
 * ## 架构定位
 * - ThemeStore / ThemeController / SettingsViewModel → ThemeRepository
 * - 设备信息/内置主题 → SettingsBridge → JNI → Rust Core
 * - 设备配色快照 → [LocalThemePaletteStore] → Android 私有目录
 *
 * ## 职责边界
 * - **做**：读写设备信息、内置主题、本地调色板记录，识别设备类型，保存动态色
 * - **不做**：设置同步通知（在 SettingsRepository）、UI 状态（在 ThemeStore）
 */
class ThemeRepository(
    context: Context,
    private val appBridge: AppServiceBridge,
) {
    private val appContext = context.applicationContext
    private val settingsBridge = appBridge.settingsBridge
    private val paletteStore = LocalThemePaletteStore(appContext)
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

    fun listPaletteRecords(): List<ThemePaletteRecord> = paletteStore.list()

    fun loadPaletteRecord(
        deviceId: String,
        fingerprint: String,
    ): ThemePaletteRecord? = paletteStore.load(deviceId, fingerprint)

    fun deletePaletteRecord(
        deviceId: String,
        fingerprint: String,
    ): Boolean = paletteStore.delete(deviceId, fingerprint)

    fun saveDynamicColorPaletteToCatalog(
        lightScheme: ThemeColorScheme,
        darkScheme: ThemeColorScheme,
        deviceClass: String? = null,
    ) {
        try {
            val deviceInfo = loadDeviceInfo()
            val deviceId = deviceInfo.deviceId.ifEmpty { "legacy" }
            val effectiveDeviceClass =
                deviceClass
                    ?: deviceInfo.deviceClass.ifEmpty { detectDeviceClass() }
            val fingerprint = ThemePaletteRecordJson.fingerprint(lightScheme, darkScheme)
            val record =
                ThemePaletteRecord(
                    schemaVersion = 1u,
                    paletteId = "$deviceId:$fingerprint",
                    paletteFingerprint = fingerprint,
                    source = "android_dynamic_color",
                    sourcePlatform = "android",
                    sourceDeviceId = deviceId,
                    sourceDeviceClass = effectiveDeviceClass,
                    capturedAtMs = System.currentTimeMillis(),
                    variant = "system_selected",
                    lightScheme = lightScheme,
                    darkScheme = darkScheme,
                )
            if (!paletteStore.save(record)) {
                warn("Failed to save local palette record ${record.paletteId}")
            }
        } catch (e: Exception) {
            DiagnosticsInterop.w("ThemeRepository", "Failed to save palette locally", e)
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
        DiagnosticsInterop.w("ThemeRepository", msg)
    }
}
