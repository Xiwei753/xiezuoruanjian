package com.xiwei.sujian.data

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import java.util.Locale
import java.util.TimeZone

object BridgeProvider {
    @Volatile
    private var appServiceInstance: AppServiceBridge? = null

    @Volatile
    private var networkCallbackRegistered = false

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
                val bridge = AppServiceBridge(holder)
                registerNetworkCallback(appContext, bridge)
                bridge
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

    private fun registerNetworkCallback(context: Context, bridge: AppServiceBridge) {
        if (networkCallbackRegistered) return
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return
        val request = NetworkRequest.Builder()
            .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .build()
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                updateNetworkStateFromSystem(context, bridge)
            }

            override fun onLost(network: Network) {
                updateNetworkStateFromSystem(context, bridge)
            }

            override fun onCapabilitiesChanged(network: Network, networkCapabilities: NetworkCapabilities) {
                updateNetworkStateFromSystem(context, bridge)
            }
        }
        cm.registerNetworkCallback(request, callback)
        networkCallbackRegistered = true
    }

    private fun updateNetworkStateFromSystem(context: Context, bridge: AppServiceBridge) {
        val (isConnected, isMetered) = detectNetworkState(context)
        try {
            bridge.holder.service.updateNetworkState(isConnected, isMetered, null, null)
        } catch (_: Exception) {
        }
    }

    fun detectNetworkState(context: Context): Pair<Boolean, Boolean> {
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
