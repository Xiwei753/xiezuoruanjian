package com.xiwei.sujian.data

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import java.util.Locale
import java.util.TimeZone

object BridgeProvider {
    @Volatile
    private var appServiceInstance: AppServiceBridge? = null

    fun getAppServiceBridge(context: Context): AppServiceBridge {
        return appServiceInstance ?: synchronized(this) {
            appServiceInstance ?: run {
                val appContext = context.applicationContext
                val workspacePath = WorkspaceManager.getWorkspaceDir(appContext).absolutePath
                val (isConnected, isMetered) = detectNetworkState(appContext)
                val holder = WriterAppServiceHolder.createFromContext(
                    workspacePath = workspacePath,
                    filesDir = appContext.filesDir.absolutePath,
                    cacheDir = appContext.cacheDir.absolutePath,
                    noBackupDir = appContext.noBackupFilesDir.absolutePath,
                    deviceId = resolveDeviceId(appContext),
                    appVersion = resolveAppVersion(appContext),
                    locale = Locale.getDefault().toLanguageTag(),
                    timezone = TimeZone.getDefault().id,
                    isConnected = isConnected,
                    isMetered = isMetered,
                )
                AppServiceBridge(holder).also { appServiceInstance = it }
            }
        }
    }

    fun getWorkspaceBridge(context: Context): WorkspaceBridge = WorkspaceBridge(getAppServiceBridge(context))
    fun getWritingBridge(context: Context): WritingBridge = WritingBridge(getAppServiceBridge(context))
    fun getStatsBridge(context: Context): StatsBridge = getAppServiceBridge(context).statsBridge
    fun getStarmapBridge(context: Context): StarMapBridge = getAppServiceBridge(context).starMapBridge
    fun getSettingsBridge(context: Context): SettingsBridge = getAppServiceBridge(context).settingsBridge
    fun getSyncBridge(context: Context): SyncBridge = getAppServiceBridge(context).syncBridge
    fun getActionBridge(context: Context): ActionBridge = ActionBridge(getAppServiceBridge(context))
    fun getLayoutPolicyBridge(context: Context): LayoutPolicyBridge = getAppServiceBridge(context).layoutPolicyBridge
    fun getScreenPolicyBridge(context: Context): ScreenPolicyBridge = getAppServiceBridge(context).screenPolicyBridge
    fun getAiStatus(context: Context): Boolean = getAppServiceBridge(context).aiAvailable()

    private fun detectNetworkState(context: Context): Pair<Boolean, Boolean> {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
            ?: return Pair(false, false)
        val network = cm.activeNetwork ?: return Pair(false, false)
        val caps = cm.getNetworkCapabilities(network) ?: return Pair(false, false)
        val isConnected = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                && caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
        val isMetered = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
        return Pair(isConnected, isMetered)
    }

    private fun resolveDeviceId(context: Context): String {
        val deviceIdFile = java.io.File(context.noBackupFilesDir, "device_id")
        if (deviceIdFile.exists()) {
            val existing = deviceIdFile.readText().trim()
            if (existing.isNotEmpty()) return existing
        }
        val newId = java.util.UUID.randomUUID().toString()
        val tmpFile = java.io.File(context.noBackupFilesDir, "device_id.tmp")
        tmpFile.writeText(newId)
        tmpFile.renameTo(deviceIdFile)
        return newId
    }

    private fun resolveAppVersion(context: Context): String {
        return try {
            val pi = context.packageManager.getPackageInfo(context.packageName, 0)
            pi.versionName ?: "unknown"
        } catch (_: Exception) {
            "unknown"
        }
    }
}
