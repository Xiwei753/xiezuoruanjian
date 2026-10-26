package com.xiwei.sujian.app.theme.interop

import android.content.Context
import com.xiwei.sujian.app.theme.model.ThemePaletteRecord
import com.xiwei.sujian.core.interop.diagnostics.DiagnosticsInterop
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import java.io.File

/**
 * Android 设备配色快照的本地存储（Issue #1005）。
 *
 * 快照只保存在当前 Android 设备，不进入 `settings.sync.json`、Git 同步或近场同步。
 * 新目录位于 [AndroidPrivateDataRoot.localThemePalettes]，旧 Core catalog 的
 * `<appDataRoot>/themes/palettes/` 只在升级时读取并导入，不删除旧文件。
 */
class LocalThemePaletteStore(
    context: Context,
) {
    private val appContext = context.applicationContext
    private val root = AndroidPrivateDataRoot.localThemePalettes(appContext)
    private val legacyRoot = File(AndroidPrivateDataRoot.root(appContext), LEGACY_RELATIVE_ROOT)
    private val lock = Any()

    @Volatile
    private var legacyMigrationDone = false

    fun list(): List<ThemePaletteRecord> =
        synchronized(lock) {
            migrateLegacyLocked()
            runCatching {
                root.listFiles()
                    ?.filter { it.isDirectory }
                    ?.flatMap { deviceDir ->
                        deviceDir.listFiles()
                            ?.filter { it.isFile && it.extension == "json" }
                            ?.mapNotNull { readRecord(it) }
                            .orEmpty()
                    }
                    .orEmpty()
                    .sortedByDescending { it.capturedAtMs }
            }.onFailure {
                warn("Failed to list local palette records", it)
            }.getOrDefault(emptyList())
        }

    fun load(
        deviceId: String,
        fingerprint: String,
    ): ThemePaletteRecord? =
        synchronized(lock) {
            if (!isSafeSegment(deviceId) || !isSafeSegment(fingerprint)) return@synchronized null
            migrateLegacyLocked()
            readRecord(fileFor(root, deviceId, fingerprint))
        }

    fun save(record: ThemePaletteRecord): Boolean =
        synchronized(lock) {
            if (!ThemePaletteRecordJson.isRecordValid(record)) {
                warn("Rejected invalid local palette record: ${record.paletteId}")
                return@synchronized false
            }
            migrateLegacyLocked()
            writeRecord(root, record)
        }

    fun delete(
        deviceId: String,
        fingerprint: String,
    ): Boolean =
        synchronized(lock) {
            if (!isSafeSegment(deviceId) || !isSafeSegment(fingerprint)) return@synchronized false
            migrateLegacyLocked()
            val newDeleted = fileFor(root, deviceId, fingerprint).delete()
            // 旧文件也删除，避免下次升级导入把用户已删除的快照重新带回来。
            val legacyDeleted = fileFor(legacyRoot, deviceId, fingerprint).delete()
            newDeleted || legacyDeleted
        }

    /**
     * 一次性导入旧 Core catalog。读取失败/单条记录损坏只跳过，不删除旧文件。
     * 旧目录仍可能被旧版本写入，但新版本自身只写 [root]。
     */
    private fun migrateLegacyLocked() {
        if (legacyMigrationDone) return
        legacyMigrationDone = true
        if (!legacyRoot.isDirectory) return
        runCatching {
            legacyRoot.listFiles()?.filter { it.isDirectory }?.forEach { deviceDir ->
                deviceDir.listFiles()?.filter { it.isFile && it.extension == "json" }?.forEach { file ->
                    if (!fileFor(root, deviceDir.name, file.nameWithoutExtension).exists()) {
                        readRecord(file)?.let { writeRecord(root, it) }
                    }
                }
            }
        }.onFailure {
            warn("Failed to migrate legacy palette records", it)
        }
    }

    private fun readRecord(file: File): ThemePaletteRecord? {
        if (!file.isFile) return null
        return runCatching {
            ThemePaletteRecordJson.decode(file.readText(Charsets.UTF_8))
                ?.takeIf(ThemePaletteRecordJson::isRecordValid)
        }.onFailure {
            warn("Failed to read palette record ${file.absolutePath}", it)
        }.getOrNull()
    }

    private fun writeRecord(
        base: File,
        record: ThemePaletteRecord,
    ): Boolean {
        if (!isSafeSegment(record.sourceDeviceId) || !isSafeSegment(record.paletteFingerprint)) {
            warn("Rejected palette record with invalid path segment: ${record.paletteId}")
            return false
        }
        val file = fileFor(base, record.sourceDeviceId, record.paletteFingerprint)
        if (file.exists()) return true
        return runCatching {
            file.parentFile?.mkdirs()
            val temp = File(file.parentFile, "${file.name}.tmp")
            temp.writeText(ThemePaletteRecordJson.encode(record), Charsets.UTF_8)
            if (!temp.renameTo(file)) {
                file.writeText(temp.readText(Charsets.UTF_8), Charsets.UTF_8)
                temp.delete()
            }
            true
        }.onFailure {
            warn("Failed to write palette record ${file.absolutePath}", it)
        }.getOrDefault(false)
    }

    private fun fileFor(
        base: File,
        deviceId: String,
        fingerprint: String,
    ): File = File(File(base, deviceId), "$fingerprint.json")

    private fun isSafeSegment(value: String): Boolean =
        value.isNotEmpty() &&
            value != "." &&
            value != ".." &&
            value == File(value).name &&
            !value.contains('\\')

    private fun warn(
        message: String,
        error: Throwable? = null,
    ) {
        if (error == null) {
            DiagnosticsInterop.w(TAG, message)
        } else {
            DiagnosticsInterop.w(TAG, message, error)
        }
    }

    private companion object {
        const val TAG = "LocalThemePaletteStore"
        const val LEGACY_RELATIVE_ROOT = "themes/palettes"
    }
}
