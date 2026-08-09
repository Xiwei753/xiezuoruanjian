package com.xiwei.sujian.app.di

import android.content.Context
import com.xiwei.sujian.app.layout.interop.LayoutPolicyBridge
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.platform.app.AndroidAppVersionProvider
import com.xiwei.sujian.core.platform.device.AndroidDeviceIdentity
import com.xiwei.sujian.core.platform.network.AndroidNetworkMonitor
import com.xiwei.sujian.feature.starmap.data.interop.StarMapBridge
import com.xiwei.sujian.feature.stats.data.interop.StatsBridge
import java.util.Locale
import java.util.TimeZone

/**
 * AppServiceProvider — 应用级服务桥接提供者（原 BridgeProvider）。
 *
 * 只保留四个桥接获取入口：AppService / Stats / StarMap / LayoutPolicy。
 * 平台相关逻辑已下沉到 :core:platform：
 * - 网络状态检测与回调注册 → AndroidNetworkMonitor
 * - 设备 ID 持久化 → AndroidDeviceIdentity
 * - 应用版本读取 → AndroidAppVersionProvider
 *
 * UI 层不应直接引用此类（架构分层规则 #597），应通过各 RepositoryProvider 间接访问。
 */
object AppServiceProvider {
    private const val TAG = "AppServiceProvider"

    @Volatile
    private var appServiceInstance: AppServiceBridge? = null

    fun getAppServiceBridge(context: Context): AppServiceBridge {
        val testProvider = SujianAppDependencies.getTestProvider()
        if (testProvider != null) {
            return testProvider(context).appServiceBridge
        }
        return appServiceInstance ?: synchronized(this) {
            appServiceInstance ?: run {
                val appContext = context.applicationContext
                val (isConnected, isMetered) = AndroidNetworkMonitor.detectNetworkState(appContext)
                val holder =
                    WriterAppServiceHolder.createFromContext(
                        context = appContext,
                        cacheDir = appContext.cacheDir.absolutePath,
                        noBackupDir = appContext.noBackupFilesDir.absolutePath,
                        deviceId = AndroidDeviceIdentity.getOrCreateDeviceId(appContext),
                        appVersion = AndroidAppVersionProvider.getAppVersion(appContext),
                        locale = Locale.getDefault().toLanguageTag(),
                        timezone = TimeZone.getDefault().id,
                        isConnected = isConnected,
                        isMetered = isMetered,
                    )
                val bridge = AppServiceBridge(holder)
                AndroidNetworkMonitor.registerNetworkCallback(appContext) {
                    refreshNetworkState(appContext, bridge)
                }
                bridge
            }
        }
    }

    fun getStatsBridge(context: Context): StatsBridge = getAppServiceBridge(context).statsBridge

    fun getStarmapBridge(context: Context): StarMapBridge = getAppServiceBridge(context).starMapBridge

    fun getLayoutPolicyBridge(context: Context): LayoutPolicyBridge = getAppServiceBridge(context).layoutPolicyBridge

    private fun refreshNetworkState(
        context: Context,
        bridge: AppServiceBridge,
    ) {
        val (isConnected, isMetered) = AndroidNetworkMonitor.detectNetworkState(context)
        try {
            bridge.holder.service.updateNetworkState(isConnected, isMetered, null, null)
            DiagnosticsLogger.d(TAG, "Network state updated: connected=$isConnected, metered=$isMetered")
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library not loaded, skipping network state update", e)
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "Failed to update network state", e)
        }
    }
}
