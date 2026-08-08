package com.xiwei.sujian.core.platform.network

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.util.Log

/**
 * Android 网络状态监控 — :core:platform 层的平台能力封装。
 *
 * 只依赖 Android 平台 API，不引用 app 模块或 interop 层的任何类型，
 * 避免向 :core:platform 引入反向依赖。日志使用 android.util.Log，
 * 因为 :core:platform 不持有 app 模块的 DiagnosticsLogger。
 */
object AndroidNetworkMonitor {
    private const val TAG = "AndroidNetworkMonitor"

    @Volatile
    private var networkCallbackRegistered = false

    /**
     * 检测当前网络状态，返回 (isConnected, isMetered)。
     *
     * isConnected 要求同时具备 NET_CAPABILITY_INTERNET 与 NET_CAPABILITY_VALIDATED；
     * isMetered 由缺失 NET_CAPABILITY_NOT_METERED 推导。
     */
    fun detectNetworkState(context: Context): Pair<Boolean, Boolean> {
        val cm =
            context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
                ?: return Pair(false, false)
        val network = cm.activeNetwork ?: return Pair(false, false)
        val caps = cm.getNetworkCapabilities(network) ?: return Pair(false, false)
        val isConnected =
            caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
        val isMetered = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
        return Pair(isConnected, isMetered)
    }

    /**
     * 注册默认网络回调。回调触发时调用 [onNetworkChanged]，
     * 由调用方决定如何把状态变化推送到上层（例如刷新 AppServiceBridge）。
     *
     * 回调只注册一次，重复调用会被 [networkCallbackRegistered] 短路。
     */
    fun registerNetworkCallback(
        context: Context,
        onNetworkChanged: () -> Unit,
    ) {
        if (networkCallbackRegistered) return
        val cm =
            context.getSystemService(ConnectivityManager::class.java)
                ?: run {
                    Log.w(TAG, "ConnectivityManager not available, network monitoring disabled")
                    return
                }
        try {
            val callback =
                object : ConnectivityManager.NetworkCallback() {
                    override fun onAvailable(network: Network) {
                        onNetworkChanged()
                    }

                    override fun onLost(network: Network) {
                        onNetworkChanged()
                    }

                    override fun onCapabilitiesChanged(
                        network: Network,
                        networkCapabilities: NetworkCapabilities,
                    ) {
                        onNetworkChanged()
                    }
                }
            cm.registerDefaultNetworkCallback(callback)
            networkCallbackRegistered = true
            Log.i(TAG, "Default network callback registered")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to register default network callback", e)
        }
    }
}
