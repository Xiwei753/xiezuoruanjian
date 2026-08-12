package com.xiwei.sujian.core.diagnostics

import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.content.FileProvider
import com.google.gson.GsonBuilder
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import com.xiwei.sujian.feature.settings.data.SettingsRepository
import com.xiwei.sujian.feature.sync.data.SyncRepository
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream

object DiagnosticsExporter {
    private const val DIAGNOSTICS_DIR = "diagnostics"

    fun export(context: Context): File? {
        return try {
            // Issue #612 评论 3.4：flush 失败（writer 死亡超时/调用线程中断）直接返回
            // 导出失败，不要继续打一个可能缺日志的 zip。
            if (!DiagnosticsLogger.flushBlocking()) {
                DiagnosticsLogger.e("DiagnosticsExporter", "flushBlocking failed; aborting export")
                return null
            }
            val cacheDir = File(context.cacheDir, DIAGNOSTICS_DIR)
            if (!cacheDir.exists()) cacheDir.mkdirs()
            cacheDir.listFiles()?.forEach { it.delete() }

            val timestamp = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
            val zipFile = File(cacheDir, "sujian-diagnostics-$timestamp.zip")

            val tempDir = File(cacheDir, "temp_$timestamp")
            tempDir.mkdirs()

            writeLogs(tempDir)
            writeCrashFile(context, tempDir)
            writeLogcat(tempDir)
            writeProcessExits(context, tempDir)
            writeThreadDump(tempDir)
            writeDeviceInfo(context, tempDir)
            writeAppSettingsSanitized(context, tempDir)
            writeSyncStateSanitized(context, tempDir)
            writeEditorSnapshot(tempDir)
            writeJankSummary(tempDir)

            zipDirectory(tempDir, zipFile)
            tempDir.deleteRecursively()

            zipFile
        } catch (e: Exception) {
            DiagnosticsLogger.e("DiagnosticsExporter", "Export failed", e)
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
            DiagnosticsLogger.e("DiagnosticsExporter", "Share failed", e)
        }
    }

    fun getDeviceInfoJson(context: Context): String {
        val info = collectDeviceInfo(context)
        val gson = GsonBuilder().setPrettyPrinting().create()
        return DiagnosticsLogger.redact(gson.toJson(info))
    }

    private fun writeLogs(destDir: File) {
        val logsDir = File(destDir, "logs")
        logsDir.mkdirs()
        // 落盘保证由 export() 入口的 flushBlocking() 统一完成（导出顺序第一步）；
        // 此处只复制 writer 已落盘的滚动日志文件。
        DiagnosticsLogger.getLogFiles().forEach { logFile ->
            try {
                val content = logFile.readText()
                val redacted = DiagnosticsLogger.redact(content)
                File(logsDir, logFile.name).writeText(redacted)
            } catch (_: Exception) {
            }
        }
    }

    private fun writeCrashFile(
        context: Context,
        destDir: File,
    ) {
        val primary = DiagnosticsLogger.getCrashFile() ?: return
        val fallback = DiagnosticsLogger.getFallbackCrashFile() ?: primary
        for ((name, file) in planCrashFileCopies(primary, fallback)) {
            try {
                val content = file.readText()
                val redacted = DiagnosticsLogger.redact(content)
                File(destDir, name).writeText(redacted)
            } catch (_: Exception) {
            }
        }
    }

    /**
     * 决定 crash 文件导出副本：主位置（外部 logsDir，或仅有的回退位置）始终
     * 以 last_crash.txt 导出；当两处都有文件时，回退位置额外以
     * last_crash_fallback.txt 导出（Issue #612 评论二.4 “导出时两处都收集”）。
     * 提取为 internal 纯函数便于单测正反验证。
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

    private fun writeLogcat(destDir: File) {
        try {
            LogcatSnapshotCollector.collect(destDir)
        } catch (e: Exception) {
            DiagnosticsLogger.w("DiagnosticsExporter", "Logcat capture failed", e)
        }
    }

    private fun writeProcessExits(
        context: Context,
        destDir: File,
    ) {
        try {
            ProcessExitCollector.collect(context, destDir)
        } catch (e: Exception) {
            DiagnosticsLogger.w("DiagnosticsExporter", "Process exit capture failed", e)
        }
    }

    private fun writeThreadDump(destDir: File) {
        try {
            ThreadDumpCollector.collect(destDir)
        } catch (e: Exception) {
            DiagnosticsLogger.w("DiagnosticsExporter", "Thread dump failed", e)
        }
    }

    private fun writeJankSummary(destDir: File) {
        try {
            val summary = JankStatsController.getSummary()
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsLogger.redact(gson.toJson(summary))
            File(destDir, "jank_summary.json").writeText(json)
        } catch (e: Exception) {
            val safeMsg = DiagnosticsLogger.redact(e.message ?: "unknown")
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            File(destDir, "jank_summary.json").writeText(errorJson)
        }
    }

    private fun writeDeviceInfo(
        context: Context,
        destDir: File,
    ) {
        val info = collectDeviceInfo(context)
        val gson = GsonBuilder().setPrettyPrinting().create()
        val json = DiagnosticsLogger.redact(gson.toJson(info))
        File(destDir, "current_device.json").writeText(json)
    }

    private fun writeAppSettingsSanitized(
        context: Context,
        destDir: File,
    ) {
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
            val json = DiagnosticsLogger.redact(gson.toJson(sanitized))
            File(destDir, "app_settings_sanitized.json").writeText(json)
        } catch (e: Exception) {
            val safeMsg = DiagnosticsLogger.redact(e.message ?: "unknown")
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            File(destDir, "app_settings_sanitized.json").writeText(errorJson)
        }
    }

    private fun writeSyncStateSanitized(
        context: Context,
        destDir: File,
    ) {
        try {
            val repo =
                SyncRepository(
                    context,
                    com.xiwei.sujian.app.di.AppServiceProvider.getAppServiceBridge(context),
                )
            // #600：sync 已改为 per-project — 诊断导出当前活动作品的同步状态。
            val projectId = com.xiwei.sujian.app.state.ActiveProjectGate.currentProjectId()
            val sanitized =
                if (projectId != null) {
                    val syncState = repo.loadSyncState(projectId)
                    mapOf(
                        "projectId" to projectId,
                        "status" to syncState.status.name,
                        "backendType" to syncState.backendType,
                        "transport" to syncState.transport,
                        "lastSyncTime" to syncState.lastSyncTime,
                        "lastError" to syncState.lastError?.let { DiagnosticsLogger.redact(it) },
                        "conflictCount" to (syncState.conflicts?.size ?: 0),
                    )
                } else {
                    mapOf(
                        "projectId" to null,
                        "status" to "no_active_project",
                    )
                }
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsLogger.redact(gson.toJson(sanitized))
            File(destDir, "sync_state_sanitized.json").writeText(json)
        } catch (e: Exception) {
            val safeMsg = DiagnosticsLogger.redact(e.message ?: "unknown")
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            File(destDir, "sync_state_sanitized.json").writeText(errorJson)
        }
    }

    private fun writeEditorSnapshot(destDir: File) {
        try {
            val snapshot = EditorEventRingBuffer.getSnapshot()
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = DiagnosticsLogger.redact(gson.toJson(snapshot))
            File(destDir, "editor_snapshot.json").writeText(json)
        } catch (e: Exception) {
            val safeMsg = DiagnosticsLogger.redact(e.message ?: "unknown")
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            File(destDir, "editor_snapshot.json").writeText(errorJson)
        }
    }

    private fun collectDeviceInfo(context: Context): Map<String, Any?> {
        val displayMetrics = context.resources.displayMetrics
        return mapOf(
            "brand" to Build.BRAND,
            "manufacturer" to Build.MANUFACTURER,
            "model" to Build.MODEL,
            "device" to Build.DEVICE,
            "product" to Build.PRODUCT,
            "sdkVersion" to Build.VERSION.SDK_INT,
            "release" to Build.VERSION.RELEASE,
            "securityPatch" to Build.VERSION.SECURITY_PATCH,
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
