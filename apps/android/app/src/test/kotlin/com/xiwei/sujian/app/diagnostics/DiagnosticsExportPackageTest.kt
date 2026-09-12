package com.xiwei.sujian.app.diagnostics

import com.xiwei.sujian.core.diagnostics.DiagnosticsBuildIdentity
import com.xiwei.sujian.core.diagnostics.DiagnosticsExporter
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.diagnostics.LogRequest
import com.xiwei.sujian.core.diagnostics.PersistentLogWriter
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.zip.ZipFile

/**
 * Issue #612 评论七收口：诊断包导出完整性与线程契约正反测试。
 *
 * 修复前（反）：导出在设置页 UI 线程同步执行（logcat 子进程 + 多文件 + zip 属于
 * 数 MB 级 I/O），大日志场景可能卡死界面触发 ANR；
 * 修复后（正）：导出由后台线程执行仍能产出完整诊断包，且失败时返回 null
 * 不抛异常（UI 显示失败提示）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class DiagnosticsExportPackageTest {
    private companion object {
        val CURRENT_LOG_ENTRY = "logs/sujian-current-${DiagnosticsBuildIdentity.fromBuildConfig().buildKey}.log"
    }

    private lateinit var context: android.content.Context

    @Before
    fun setUp() {
        context = androidx.test.core.app.ApplicationProvider.getApplicationContext()
        DiagnosticsLogger.init(context, isEnabled = true, isVerbose = true)
        PersistentLogWriter.flushBlocking()
        PersistentLogWriter.clearLogs()
    }

    @After
    fun tearDown() {
        PersistentLogWriter.flushBlocking()
        PersistentLogWriter.clearLogs()
    }

    /** 正：后台线程导出，包内包含评论七要求的全部条目，日志内容已 flush 落盘。 */
    @Test
    fun exportFromBackgroundThreadProducesCompletePackage() {
        PersistentLogWriter.enqueue(
            LogRequest(
                level = "INFO",
                tag = "export-test",
                message = "export-package-probe",
                timestampMs = 1_000L,
                threadName = "test",
            ),
        )
        val logsDir = AndroidPrivateDataRoot.logs(context)
        logsDir.mkdirs()
        File(logsDir, "last_crash.txt").writeText("probe crash content")

        val executor = Executors.newSingleThreadExecutor()
        val zipFile: File?
        try {
            val future = executor.submit<File?> { DiagnosticsExporter.export(context) }
            zipFile = future.get(60, TimeUnit.SECONDS)
        } finally {
            executor.shutdownNow()
        }

        assertNotNull("export must produce a zip from background thread", zipFile)
        ZipFile(zipFile).use { zip ->
            val entries = zip.entries().asSequence().map { it.name }.toSet()
            val expected =
                setOf(
                    CURRENT_LOG_ENTRY,
                    "logcat.txt",
                    "process_exits.json",
                    "threads.txt",
                    "jank_summary.json",
                    "app_settings_sanitized.json",
                    "sync_state_sanitized.json",
                    "editor_snapshot.json",
                    "last_crash.txt",
                    // #665：导出包必须带统一 diagnostics_manifest.json
                    "diagnostics_manifest.json",
                    // #623：保留旧的身份和设备信息文件
                    "current_device.json",
                    "build_identity.json",
                )
            for (name in expected) {
                assertTrue("package must contain $name, got $entries", name in entries)
            }
            assertDiagnosticsManifestInZip(zip)
            val logText =
                zip.getEntry(CURRENT_LOG_ENTRY)
                    ?.let { zip.getInputStream(it).readBytes().toString(Charsets.UTF_8) }
                    .orEmpty()
            assertTrue(
                "app log must be flushed into package, got: $logText",
                logText.contains("export-package-probe"),
            )
            val jankJson =
                zip.getEntry("jank_summary.json")
                    ?.let { zip.getInputStream(it).readBytes().toString(Charsets.UTF_8) }
                    .orEmpty()
            assertTrue("jank_summary.json must contain totalFrames", jankJson.contains("totalFrames"))
            assertTrue("jank_summary.json must contain jankFrames", jankJson.contains("jankFrames"))
        }
        zipFile.delete()
    }

    /**
     * #665：diagnostics_manifest.json 必须携带统一身份信息 —
     * platform、构建身份、runtime 和 collection 状态。
     *
     * #665 评论 5643315523：用 Gson 严格解析为 JsonObject 验证可解析性，
     * 确保成功导出的包一定存在可解析的 diagnostics_manifest.json。
     */
    private fun assertDiagnosticsManifestInZip(zip: ZipFile) {
        val manifestJson =
            zip.getEntry("diagnostics_manifest.json")
                ?.let { zip.getInputStream(it).readBytes().toString(Charsets.UTF_8) }
                .orEmpty()
        assertTrue(
            "diagnostics_manifest.json must exist in zip",
            manifestJson.isNotEmpty(),
        )
        // 严格解析为 JSON 对象，验证可解析性
        val manifestObj =
            com.google.gson.JsonParser.parseString(manifestJson).asJsonObject
        val expectedIdentity = DiagnosticsBuildIdentity.fromBuildConfig()
        assertEquals(
            "diagnostics_manifest.json must carry schemaVersion=1",
            1,
            manifestObj.get("schemaVersion").asInt,
        )
        assertEquals(
            "diagnostics_manifest.json must carry platform=android",
            "android",
            manifestObj.get("platform").asString,
        )
        assertNotNull(
            "diagnostics_manifest.json must carry versionCode",
            manifestObj.get("versionCode"),
        )
        assertTrue(
            "diagnostics_manifest.json must carry gitCommitSha, got: $manifestJson",
            manifestJson.contains(expectedIdentity.gitCommitSha),
        )
        assertTrue(
            "diagnostics_manifest.json must carry flavor, got: $manifestJson",
            manifestJson.contains(expectedIdentity.flavor),
        )
        assertTrue(
            "diagnostics_manifest.json must carry buildType, got: $manifestJson",
            manifestJson.contains(expectedIdentity.buildType),
        )
        assertTrue(
            "diagnostics_manifest.json must carry applicationId, got: $manifestJson",
            manifestJson.contains(expectedIdentity.applicationId),
        )
        assertNotNull(
            "diagnostics_manifest.json must carry collection status",
            manifestObj.get("collection"),
        )
    }

    /**
     * 反：缓存目录不可写时导出失败返回 null（不抛异常、不崩溃）。
     *
     * #665 评论 5643315523：此场景同时覆盖 manifest 写失败路径 — tempDir 不可写时
     * writeDiagnosticsManifest 中 writeText 抛异常返回 false，export 检查后返回 null，
     * 不产出没有 diagnostics_manifest.json 的 zip 包。
     */
    @Test
    fun exportReturnsNullWhenCacheDirIsNotWritable() {
        val cache = context.cacheDir
        cache.deleteRecursively()
        cache.writeText("occupied by a file")

        val zipFile = DiagnosticsExporter.export(context)
        assertNull("export must return null on failure", zipFile)
    }

    /**
     * 反（评论 3.4）：flushBlocking 失败（调用线程被中断）时 export 必须直接返回
     * null 表示导出失败，不得继续打一个可能缺日志的 zip。
     */
    @Test
    fun exportReturnsNullWhenFlushFails() {
        val executor = Executors.newSingleThreadExecutor()
        try {
            val future =
                executor.submit<File?> {
                    Thread.currentThread().interrupt()
                    DiagnosticsExporter.export(context)
                }
            val zipFile = future.get(60, TimeUnit.SECONDS)
            assertNull("export must return null when flush fails", zipFile)
        } finally {
            executor.shutdownNow()
        }
    }

    /** 正：导出不把脱敏字段原样带出（token 落盘前已被 redact 清除）。 */
    @Test
    fun exportedLogsAreRedacted() {
        PersistentLogWriter.enqueue(
            LogRequest(
                level = "WARN",
                tag = "export-test",
                message = "login token=ghp_123456789012345678901234567890123456 failed",
                timestampMs = 2_000L,
                threadName = "test",
            ),
        )
        val executor = Executors.newSingleThreadExecutor()
        val zipFile: File?
        try {
            val future = executor.submit<File?> { DiagnosticsExporter.export(context) }
            zipFile = future.get(60, TimeUnit.SECONDS)
        } finally {
            executor.shutdownNow()
        }
        assertNotNull(zipFile)
        ZipFile(zipFile).use { zip ->
            val logText =
                zip.getEntry(CURRENT_LOG_ENTRY)
                    ?.let { zip.getInputStream(it).readBytes().toString(Charsets.UTF_8) }
                    .orEmpty()
            assertTrue("exported log must contain the probe line", logText.contains("login token="))
            assertTrue(
                "token value must be redacted in exported log",
                !logText.contains("ghp_123456789012345678901234567890123456"),
            )
            assertEquals("redacted marker must be present", true, logText.contains("[REDACTED]"))
        }
        zipFile.delete()
    }
}
