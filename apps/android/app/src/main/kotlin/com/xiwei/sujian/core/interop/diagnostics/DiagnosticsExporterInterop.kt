package com.xiwei.sujian.core.interop.diagnostics

import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.content.FileProvider
import com.google.gson.GsonBuilder
import com.xiwei.sujian.R
import com.xiwei.sujian.core.diagnostics.JankStatsController
import com.xiwei.sujian.core.diagnostics.LogcatSnapshotCollector
import com.xiwei.sujian.core.diagnostics.ProcessExitCollector
import com.xiwei.sujian.core.diagnostics.ThreadDumpCollector
import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.sync.data.SyncRepository
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.text.SimpleDateFormat
import java.time.Instant
import java.util.Date
import java.util.Locale
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream

/**
 * 诊断导出 interop — Issue #670 评论 5651060802。
 *
 * 替代旧的 `core.diagnostics.DiagnosticsExporter`。底层日志 flush/clear/export
 * 通过 [DiagnosticsInterop] 转发到 Rust `writer_diagnostics`，不再自己定义
 * 日志格式/轮转/落盘。
 *
 * 平台附件（logcat / processExit / threadDump / jank / settings / sync state /
 * editor snapshot / device info / build identity）仍由本对象在 Kotlin 端收集，
 * 因为这些是 Android 平台特有数据，需要访问 Android API（ActivityManager、
 * DisplayMetrics、JankStats 等）。附件内容经 [DiagnosticsInterop.redact] 脱敏后
 * 落盘，最终与 Rust 导出的日志 zip 合并。
 */
object DiagnosticsExporterInterop {
    private const val DIAGNOSTICS_DIR = "diagnostics"
    private const val TAG = "DiagnosticsExporter"

    fun export(context: Context): File? {
        return try {
            if (!DiagnosticsInterop.flushBlocking()) {
                DiagnosticsInterop.e(TAG, "flushBlocking failed; aborting export")
                return null
            }
            val cacheDir = File(context.cacheDir, DIAGNOSTICS_DIR)
            if (!cacheDir.exists()) cacheDir.mkdirs()
            cacheDir.listFiles()?.forEach { it.delete() }

            val timestamp = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
            val zipFile = File(cacheDir, "sujian-diagnostics-$timestamp.zip")

            val tempDir = File(cacheDir, "temp_$timestamp")
            tempDir.mkdirs()

            val logsStatus = writeLogs(context, tempDir)
            val crashStatus = writeCrashFile(tempDir)
            val logcatStatus = writeLogcat(tempDir)
            val processExitsStatus = writeProcessExits(context, tempDir)
            val threadDumpStatus = writeThreadDump(tempDir)
            val settingsStatus = writeAppSettingsSanitized(context, tempDir)
            val syncStatus = writeSyncStateSanitized(context, tempDir)
            val editorStatus = writeEditorSnapshot(tempDir)
            val jankStatus = writeJankSummary(tempDir)

            writeDeviceInfo(context, tempDir)
            writeBuildIdentity(tempDir)
            if (!writeDiagnosticsManifest(
                    context,
                    tempDir,
                    mapOf(
                        "logs" to logsStatus,
                        "crash" to crashStatus,
                        "logcat" to logcatStatus,
                        "processExits" to processExitsStatus,
                        "threadDump" to threadDumpStatus,
                        "settings" to settingsStatus,
                        "sync" to syncStatus,
                        "editor" to editorStatus,
                        "jank" to jankStatus,
                    ),
                )
            ) {
                DiagnosticsInterop.e(TAG, "diagnostics_manifest.json write failed; aborting export")
                return null
            }

            zipDirectory(tempDir, zipFile)
            tempDir.deleteRecursively()

            zipFile
        } catch (e: Exception) {
            DiagnosticsInterop.e(TAG, "Export failed", e)
            null
        }
    }

    fun shareZip(
        context: Context,
        zipFile: File,
    ) {
        try {
            val uri =
                FileProvider.getUriForFile(
                    context,
                    "${context.packageName}.fileprovider",
                    zipFile,
                )
            val shareIntent =
                Intent(Intent.ACTION_SEND).apply {
                    type = "application/zip"
                    putExtra(Intent.EXTRA_STREAM, uri)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
            context.startActivity(
                Intent.createChooser(shareIntent, context.getString(R.string.share_diagnostics_title)),
            )
        } catch (e: Exception) {
            DiagnosticsInterop.e(TAG, "Share failed", e)
        }
    }

    fun getDeviceInfoJson(context: Context): String {
        val info = collectDeviceInfo(context)
        val gson = GsonBuilder().setPrettyPrinting().create()
        return DiagnosticsInterop.redact(gson.toJson(info))
    }

    /**
     * 决定 crash 文件导出副本。提取为 internal 供单测。
     */
    internal fun planCrashFileCopies(
        primary: File,
        fallback: File,
    ): List<Pair<String, File>> {
        val copies = mutableListOf<Pair<String, File>>()
        if (primary.exists()) copies.add("last_crash.txt" to primary)
        if (fallback.exists() && fallback != primary) {
            copies.add("last_crash_fallback.txt" to fallback)
        }
        return copies
    }

    /**
     * 复制 Rust writer 已落盘的日志文件到 destDir/logs/。
     * Rust exportDiagnostics 生成日志 zip，这里解包复制到导出目录。
     * 返回 "ok" / "missing" / "error"。
     */
    private fun writeLogs(
        context: Context,
        destDir: File,
    ): String {
        val logsDir = File(destDir, "logs")
        logsDir.mkdirs()
        // 通过 Rust exportDiagnostics 导出日志到临时目录，再解包复制。
        val rustOutputDir = File(context.cacheDir, "rust_diagnostics_export")
        rustOutputDir.mkdirs()
        rustOutputDir.listFiles()?.forEach { it.delete() }
        val rustZipPath = DiagnosticsInterop.exportDiagnostics(rustOutputDir.absolutePath)
        if (rustZipPath == null) {
            // Rust 导出失败，回退为空日志目录标记。
            return "missing"
        }
        val rustZipFile = File(rustZipPath)
        return try {
            // 解压 Rust 生成的 zip 到 logsDir。
            java.util.zip.ZipInputStream(FileInputStream(rustZipFile)).use { zis ->
                var entry = zis.nextEntry
                while (entry != null) {
                    val outFile = File(logsDir, entry.name)
                    outFile.parentFile?.mkdirs()
                    FileOutputStream(outFile).use { fos ->
                        zis.copyTo(fos)
                    }
                    zis.closeEntry()
                    entry = zis.nextEntry
                }
            }
            rustZipFile.delete()
            "ok"
        } catch (_: Exception) {
            rustZipFile.delete()
            "error"
        }
    }

    private fun writeCrashFile(destDir: File): String {
        val primary = DiagnosticsInterop.getCrashFile() ?: return "not_found"
        val fallback = DiagnosticsInterop.getFallbackCrashFile() ?: primary
        val copies = planCrashFileCopies(primary, fallback)
        if (copies.isEmpty()) return "not_found"
        var allOk = true
        for ((name, file) in copies) {
            try {
                val content = file.readText()
                val redacted = DiagnosticsInterop.redact(content)
                File(destDir, name).writeText(redacted)
            } catch (_: Exception) {
                allOk = false
            }
        }
        return if (allOk) "ok" else "error"
    }

    private fun writeLogcat(destDir: File): String =
        try {
            LogcatSnapshotCollector.collect(destDir)
            "ok"
        } catch (e: Exception) {
            DiagnosticsInterop.w(TAG, "Logcat capture failed", e)
            "error"
        }

    private fun writeProcessExits(
        context: Context,
        destDir: File,
    ): String =
        try {
            ProcessExitCollector.collect(context, destDir)
            "ok"
        } catch (e: Exception) {
            DiagnosticsInterop.w(TAG, "Process exit capture failed", e)
            "error"
        }

    private fun writeThreadDump(destDir: File): String =
        try {
            ThreadDumpCollector.collect(destDir)
            "ok"
        } catch (e: Exception) {
            DiagnosticsInterop.w(TAG, "Thread dump failed", e)
            "error"
        }

    private fun writeJankSummary(destDir: File): String =
        try {
            val summary = JankStatsController.getSummary()
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsInterop.redact(gson.toJson(summary))
            File(destDir, "jank_summary.json").writeText(json)
            "ok"
        } catch (e: Exception) {
            val safeMsg = DiagnosticsInterop.redact(e.message ?: "unknown")
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            File(destDir, "jank_summary.json").writeText(errorJson)
            "error"
        }

    private fun writeDeviceInfo(
        context: Context,
        destDir: File,
    ) {
        val info = collectDeviceInfo(context)
        val gson = GsonBuilder().setPrettyPrinting().create()
        val json = DiagnosticsInterop.redact(gson.toJson(info))
        File(destDir, "current_device.json").writeText(json)
    }

    private fun writeBuildIdentity(destDir: File) {
        try {
            val identity = DiagnosticsInterop.buildIdentity()
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsInterop.redact(gson.toJson(identity))
            File(destDir, "build_identity.json").writeText(json)
        } catch (e: Exception) {
            val safeMsg = DiagnosticsInterop.redact(e.message ?: "unknown")
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            File(destDir, "build_identity.json").writeText(errorJson)
        }
    }

    private fun writeDiagnosticsManifest(
        context: Context,
        destDir: File,
        collectionStatus: Map<String, String>,
    ): Boolean {
        return try {
            val identity = DiagnosticsInterop.buildIdentity()
            val deviceInfo = collectDeviceInfo(context)
            val supportedAbis = Build.SUPPORTED_ABIS.toList()
            val arch = supportedAbis.firstOrNull() ?: "unknown"
            val exportedAt = Instant.now().toString()

            val manifest =
                mapOf(
                    "schemaVersion" to 1,
                    "platform" to "android",
                    *identity.toManifestFields().toList().toTypedArray(),
                    "exportedAt" to exportedAt,
                    "arch" to arch,
                    "runtime" to
                        mapOf(
                            "sdkVersion" to deviceInfo["sdkVersion"],
                            "release" to deviceInfo["release"],
                            "securityPatch" to deviceInfo["securityPatch"],
                            "supportedAbis" to supportedAbis,
                            "applicationId" to identity.applicationId,
                        ),
                    "system" to
                        mapOf(
                            "brand" to deviceInfo["brand"],
                            "manufacturer" to deviceInfo["manufacturer"],
                            "model" to deviceInfo["model"],
                            "device" to deviceInfo["device"],
                            "product" to deviceInfo["product"],
                            "screenWidthPx" to deviceInfo["screenWidthPx"],
                            "screenHeightPx" to deviceInfo["screenHeightPx"],
                            "densityDpi" to deviceInfo["densityDpi"],
                            "density" to deviceInfo["density"],
                            "scaledDensity" to deviceInfo["scaledDensity"],
                            "supportedAbis" to deviceInfo["supportedAbis"],
                        ),
                    "collection" to collectionStatus,
                )

            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsInterop.redact(gson.toJson(manifest))
            File(destDir, "diagnostics_manifest.json").writeText(json)
            true
        } catch (e: Exception) {
            DiagnosticsInterop.e(TAG, "Failed to write diagnostics manifest", e)
            false
        }
    }

    private fun writeAppSettingsSanitized(
        context: Context,
        destDir: File,
    ): String =
        try {
            val repo =
                SettingsRepository(
                    context,
                    com.xiwei.sujian.app.di.AppServiceProvider.getAppServiceBridge(context),
                )
            val settings = repo.getLocalSettings()
            val sanitized =
                mapOf(
                    "themeMode" to settings.themeMode,
                    "editorFontSize" to settings.editorFontSize,
                    "editorLineSpacingMultiplier" to settings.editorLineSpacingMultiplier,
                    "autoSaveEnabled" to settings.autoSaveEnabled,
                    "autoSaveDelayMs" to settings.autoSaveDelayMs,
                    "autoIndentEnabled" to settings.autoIndentEnabled,
                    "autoIndentWidth" to settings.autoIndentWidth,
                    "editorTypingAnimationEnabled" to settings.editorTypingAnimationEnabled,
                    "editorSmoothCursorEnabled" to settings.editorSmoothCursorEnabled,
                    "editorTypingAnimationDurationMs" to settings.editorTypingAnimationDurationMs,
                    "editorSmoothCursorDurationMs" to settings.editorSmoothCursorDurationMs,
                    "aiEnabled" to settings.aiEnabled,
                    "diagnosticsEnabled" to settings.diagnosticsEnabled,
                    "diagnosticsVerbose" to settings.diagnosticsVerbose,
                )
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsInterop.redact(gson.toJson(sanitized))
            File(destDir, "app_settings_sanitized.json").writeText(json)
            "ok"
        } catch (e: Exception) {
            val safeMsg = DiagnosticsInterop.redact(e.message ?: "unknown")
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            File(destDir, "app_settings_sanitized.json").writeText(errorJson)
            "error"
        }

    private fun writeSyncStateSanitized(
        context: Context,
        destDir: File,
    ): String =
        try {
            val repo =
                SyncRepository(
                    context,
                    com.xiwei.sujian.app.di.AppServiceProvider.getAppServiceBridge(context),
                )
            val projectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
            val sanitized =
                if (projectId != null) {
                    val syncState = repo.loadSyncState(projectId)
                    mapOf(
                        "projectId" to projectId,
                        "status" to syncState.status.name,
                        "lastSyncTime" to syncState.lastSyncTime,
                        "lastError" to syncState.lastError?.let { DiagnosticsInterop.redact(it) },
                        "conflictCount" to (syncState.conflicts?.size ?: 0),
                    )
                } else {
                    mapOf(
                        "projectId" to null,
                        "status" to "no_active_project",
                    )
                }
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsInterop.redact(gson.toJson(sanitized))
            File(destDir, "sync_state_sanitized.json").writeText(json)
            "ok"
        } catch (e: Exception) {
            val safeMsg = DiagnosticsInterop.redact(e.message ?: "unknown")
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            File(destDir, "sync_state_sanitized.json").writeText(errorJson)
            "error"
        }

    private fun writeEditorSnapshot(destDir: File): String =
        try {
            val snapshot = EditorEventRingBuffer.getSnapshot()
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsInterop.redact(gson.toJson(snapshot))
            File(destDir, "editor_snapshot.json").writeText(json)
            "ok"
        } catch (e: Exception) {
            val safeMsg = DiagnosticsInterop.redact(e.message ?: "unknown")
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            File(destDir, "editor_snapshot.json").writeText(errorJson)
            "error"
        }

    private fun collectDeviceInfo(context: Context): Map<String, Any?> {
        val displayMetrics = context.resources.displayMetrics
        return mapOf(
            "platform" to "android",
            "brand" to Build.BRAND,
            "manufacturer" to Build.MANUFACTURER,
            "model" to Build.MODEL,
            "device" to Build.DEVICE,
            "product" to Build.PRODUCT,
            "sdkVersion" to Build.VERSION.SDK_INT,
            "release" to Build.VERSION.RELEASE,
            "securityPatch" to Build.VERSION.SECURITY_PATCH,
            "supportedAbis" to Build.SUPPORTED_ABIS.toList(),
            "screenWidthPx" to displayMetrics.widthPixels,
            "screenHeightPx" to displayMetrics.heightPixels,
            "densityDpi" to displayMetrics.densityDpi,
            "density" to displayMetrics.density,
            "scaledDensity" to displayMetrics.scaledDensity,
        )
    }

    private fun zipDirectory(
        sourceDir: File,
        zipFile: File,
    ) {
        ZipOutputStream(FileOutputStream(zipFile)).use { zos ->
            sourceDir.walkTopDown().forEach { file ->
                if (file.isDirectory) return@forEach
                val entryName = file.relativeTo(sourceDir).path.replace('\\', '/')
                zos.putNextEntry(ZipEntry(entryName))
                FileInputStream(file).use { fis ->
                    fis.copyTo(zos)
                }
                zos.closeEntry()
            }
        }
    }
}
