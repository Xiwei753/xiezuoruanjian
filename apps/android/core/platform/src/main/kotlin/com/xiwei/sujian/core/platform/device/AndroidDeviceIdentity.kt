package com.xiwei.sujian.core.platform.device

import android.content.Context
import java.io.File
import java.util.UUID

/**
 * Android 设备标识 — :core:platform 层的平台能力封装。
 *
 * 在 noBackupFilesDir 下持久化一份 device_id，进程首次启动时生成，
 * 后续读取复用。写入采用 tmp + rename 的原子替换，避免半写状态。
 */
object AndroidDeviceIdentity {
    fun getOrCreateDeviceId(context: Context): String {
        val deviceIdFile = File(context.noBackupFilesDir, "device_id")
        if (deviceIdFile.exists()) {
            val existing = deviceIdFile.readText().trim()
            if (existing.isNotEmpty()) return existing
        }
        val newId = UUID.randomUUID().toString()
        val tmpFile = File(context.noBackupFilesDir, "device_id.tmp")
        tmpFile.writeText(newId)
        tmpFile.renameTo(deviceIdFile)
        return newId
    }
}
