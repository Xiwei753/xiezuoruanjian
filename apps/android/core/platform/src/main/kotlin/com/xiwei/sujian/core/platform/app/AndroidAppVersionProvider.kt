package com.xiwei.sujian.core.platform.app

import android.content.Context

/**
 * Android 应用版本提供者 — :core:platform 层的平台能力封装。
 *
 * 通过 PackageManager 读取当前包的 versionName，读取失败时回退到 "unknown"。
 */
object AndroidAppVersionProvider {
    fun getAppVersion(context: Context): String {
        return try {
            val pi = context.packageManager.getPackageInfo(context.packageName, 0)
            pi.versionName ?: "unknown"
        } catch (_: Exception) {
            "unknown"
        }
    }
}
