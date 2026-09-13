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
import java.text.SimpleDateFormat
import java.time.Instant
import java.util.Date
import java.util.Locale
import uniffi.writer_core.DiagnosticAttachmentDto
import uniffi.writer_core.exportDiagnostics as nativeExportDiagnostics

/**
 * 诊断导出 interop — Issue #670 评论 5651060802 / 5651816143 修改 3。
 *
 * 替代旧的 `core.diagnostics.DiagnosticsExporter`。底层日志 flush/clear/export
 * 通过 [DiagnosticsInterop] 转发到 Rust `writer_diagnostics`，不再自己定义
 * 日志格式/轮转/落盘。
 *
 * 平台附件（logcat / processExit / threadDump / jank / settings / sync state /
 * editor snapshot / device info / build identity）仍由本对象在 Kotlin 端收集，
 * 因为这些是 Android 平台特有数据，需要访问 Android API（ActivityManager、
 * DisplayMetrics、JankStats 等）。
 *
 * Issue #670 评论 5651816143 修改 3：附件打包只由 Rust `core/writer_diagnostics/src/export.rs`
 * 生成一次。本对象只负责：
 * 1. 收集所有附件为 `List<DiagnosticAttachmentDto>`（content 为字节）
 * 2. 调用 UniFFI `exportDiagnostics(outputDir, attachments)` 生成最终 zip
 * 3. shareZip 分享最终 zip
 *
 * 不再自己 `writeDiagnosticsManifest()` / `zipDirectory()` / Rust zip 解压再重打包。
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

            // 收集所有平台附件为 DiagnosticAttachmentDto 列表。
            // 附件内容仍由平台采集器写入临时目录，再读回字节交给 Rust。
            // Rust exportDiagnostics 负责写附件、生成 manifest、打 zip（只生成一次）。
            val tempDir = File(cacheDir, "temp_attachments")
            tempDir.mkdirs()
            tempDir.listFiles()?.forEach { it.delete() }

            val attachments = mutableListOf<DiagnosticAttachmentDto>()

            // crash 文件
            addCrashAttachments(attachments)

            // logcat — LogcatSnapshotCollector.collect(destDir) 写文件到 destDir
            runCatching { LogcatSnapshotCollector.collect(tempDir) }
                .onFailure { DiagnosticsInterop.w(TAG, "Logcat capture failed", it) }
            // processExits
            runCatching { ProcessExitCollector.collect(context, tempDir) }
                .onFailure { DiagnosticsInterop.w(TAG, "Process exit capture failed", it) }
            // threadDump
            runCatching { ThreadDumpCollector.collect(tempDir) }
                .onFailure { DiagnosticsInterop.w(TAG, "Thread dump failed", it) }
            // jank summary
            addJankSummaryAttachment(attachments)
            // app settings
            addAppSettingsAttachment(context, attachments)
            // sync state
            addSyncStateAttachment(context, attachments)
            // editor snapshot
            addEditorSnapshotAttachment(attachments)
            // device info
            addDeviceInfoAttachment(context, attachments)
            // build identity
            addBuildIdentityAttachment(attachments)

            // 把 tempDir 中由 collector 写入的文件也读回作为附件。
            tempDir.walkTopDown().forEach { file ->
                if (file.isFile) {
                    val relPath = file.relativeTo(tempDir).path.replace('\\', '/')
                    val content = file.readBytes()
                    attachments.add(DiagnosticAttachmentDto(relPath, content))
                }
            }

            // 调用 UniFFI exportDiagnostics 生成最终 zip（附件打包只由 Rust 生成一次）。
            val zipPath = nativeExportDiagnostics(cacheDir.absolutePath, attachments)
            val zipFile = File(zipPath)

            // 清理临时附件目录。
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

    // ── 附件收集 ──────────────────────────────────────────────────
    //
    // 每个附件收集器把内容读回字节交给 Rust。附件脱敏由 Rust export.rs 统一处理
    // （Issue #670 评论 5651816143 修改 4），Kotlin 端不再复制一套脱敏规则。

    private fun addCrashAttachments(attachments: MutableList<DiagnosticAttachmentDto>) {
        val primary = DiagnosticsInterop.getCrashFile() ?: return
        val fallback = DiagnosticsInterop.getFallbackCrashFile() ?: primary
        val copies = planCrashFileCopies(primary, fallback)
        for ((name, file) in copies) {
            runCatching {
                attachments.add(DiagnosticAttachmentDto(name, file.readBytes()))
            }
        }
    }

    private fun addJankSummaryAttachment(attachments: MutableList<DiagnosticAttachmentDto>) {
        runCatching {
            val summary = JankStatsController.getSummary()
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = gson.toJson(summary)
            attachments.add(
                DiagnosticAttachmentDto("jank_summary.json", json.toByteArray(Charsets.UTF_8)),
            )
        }.onFailure { e ->
            val safeMsg = e.message ?: "unknown"
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            attachments.add(
                DiagnosticAttachmentDto(
                    "jank_summary.json",
                    errorJson.toByteArray(Charsets.UTF_8),
                ),
            )
        }
    }

    private fun addAppSettingsAttachment(
        context: Context,
        attachments: MutableList<DiagnosticAttachmentDto>,
    ) {
        runCatching {
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
            val json = gson.toJson(sanitized)
            attachments.add(
                DiagnosticAttachmentDto(
                    "app_settings_sanitized.json",
                    json.toByteArray(Charsets.UTF_8),
                ),
            )
        }.onFailure { e ->
            val safeMsg = e.message ?: "unknown"
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            attachments.add(
                DiagnosticAttachmentDto(
                    "app_settings_sanitized.json",
                    errorJson.toByteArray(Charsets.UTF_8),
                ),
            )
        }
    }

    private fun addSyncStateAttachment(
        context: Context,
        attachments: MutableList<DiagnosticAttachmentDto>,
    ) {
        runCatching {
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
                        "lastError" to syncState.lastError,
                        "conflictCount" to (syncState.conflicts?.size ?: 0),
                    )
                } else {
                    mapOf(
                        "projectId" to null,
                        "status" to "no_active_project",
                    )
                }
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = gson.toJson(sanitized)
            attachments.add(
                DiagnosticAttachmentDto(
                    "sync_state_sanitized.json",
                    json.toByteArray(Charsets.UTF_8),
                ),
            )
        }.onFailure { e ->
            val safeMsg = e.message ?: "unknown"
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            attachments.add(
                DiagnosticAttachmentDto(
                    "sync_state_sanitized.json",
                    errorJson.toByteArray(Charsets.UTF_8),
                ),
            )
        }
    }

    private fun addEditorSnapshotAttachment(attachments: MutableList<DiagnosticAttachmentDto>) {
        runCatching {
            val snapshot = EditorEventRingBuffer.getSnapshot()
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = gson.toJson(snapshot)
            attachments.add(
                DiagnosticAttachmentDto(
                    "editor_snapshot.json",
                    json.toByteArray(Charsets.UTF_8),
                ),
            )
        }.onFailure { e ->
            val safeMsg = e.message ?: "unknown"
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            attachments.add(
                DiagnosticAttachmentDto(
                    "editor_snapshot.json",
                    errorJson.toByteArray(Charsets.UTF_8),
                ),
            )
        }
    }

    private fun addDeviceInfoAttachment(
        context: Context,
        attachments: MutableList<DiagnosticAttachmentDto>,
    ) {
        runCatching {
            val info = collectDeviceInfo(context)
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = gson.toJson(info)
            attachments.add(
                DiagnosticAttachmentDto(
                    "current_device.json",
                    json.toByteArray(Charsets.UTF_8),
                ),
            )
        }
    }

    private fun addBuildIdentityAttachment(attachments: MutableList<DiagnosticAttachmentDto>) {
        runCatching {
            val identity = DiagnosticsInterop.buildIdentity()
            val gson = GsonBuilder().setPrettyPrinting().create()
            val json = gson.toJson(identity)
            attachments.add(
                DiagnosticAttachmentDto(
                    "build_identity.json",
                    json.toByteArray(Charsets.UTF_8),
                ),
            )
        }.onFailure { e ->
            val safeMsg = e.message ?: "unknown"
            val errorJson = GsonBuilder().create().toJson(mapOf("error" to safeMsg))
            attachments.add(
                DiagnosticAttachmentDto(
                    "build_identity.json",
                    errorJson.toByteArray(Charsets.UTF_8),
                ),
            )
        }
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
}
