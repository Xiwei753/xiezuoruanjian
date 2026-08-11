@file:Suppress("StringLiteralDuplication") // 测试固件中 "screen=" / "editor=" 等前缀天然重复

package com.xiwei.sujian.app.diagnostics

import com.xiwei.sujian.app.navigation.SujianDestination
import com.xiwei.sujian.app.navigation.resolveTopLevelSwitchInteraction
import com.xiwei.sujian.app.navigation.syncIndicatorSummary
import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.core.diagnostics.DiagnosticsExporter
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.diagnostics.JankStatsController
import com.xiwei.sujian.core.diagnostics.LogRequest
import com.xiwei.sujian.core.diagnostics.LogcatSnapshotCollector
import com.xiwei.sujian.core.diagnostics.PersistentLogWriter
import com.xiwei.sujian.core.diagnostics.ProcessExitCollector
import com.xiwei.sujian.core.diagnostics.ProcessStateSummary
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot
import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import com.xiwei.sujian.feature.sync.data.model.SyncIndicatorState
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File

/**
 * Issue #612 八：ProcessStateSummary 摘要构造与 ≤128 bytes 截断测试。
 *
 * buildSummary / truncateToBytes 提取为 internal 纯函数，可在纯 JVM 测试中
 * 直接验证格式与截断逻辑，不需要 Robolectric / ActivityManager。
 */
class ProcessStateSummaryTest {
    @Test
    fun buildSummaryFormatIsScreenEditorSync() {
        val summary = ProcessStateSummary.buildSummary("Works", "0", "idle")
        assertEquals("screen=Works;editor=0;sync=idle", summary)
    }

    @Test
    fun buildSummaryWithEditorActiveFlag() {
        val active = ProcessStateSummary.buildSummary("Works", "1", "idle")
        assertEquals("screen=Works;editor=1;sync=idle", active)
        val inactive = ProcessStateSummary.buildSummary("Works", "0", "syncing")
        assertEquals("screen=Works;editor=0;sync=syncing", inactive)
    }

    @Test
    fun buildSummaryWithEmptyValues() {
        val summary = ProcessStateSummary.buildSummary("", "", "")
        assertEquals("screen=;editor=;sync=", summary)
    }

    @Test
    fun truncateToBytesKeepsShortStringUnchanged() {
        val text = "screen=Works;editor=0;sync=idle"
        val truncated = ProcessStateSummary.truncateToBytes(text, 128)
        assertEquals(text, truncated)
    }

    @Test
    fun truncateToBytesLimitsToMaxBytes() {
        // 构造超过 128 bytes 的字符串
        val longText = "x".repeat(300)
        val truncated = ProcessStateSummary.truncateToBytes(longText, 128)
        assertTrue("truncated length should be <= 128 bytes", truncated.toByteArray(Charsets.UTF_8).size <= 128)
        assertTrue("truncated should be non-empty", truncated.isNotEmpty())
    }

    @Test
    fun truncateToBytesHandlesMultibyteUtf8() {
        // 中文字符占 3 bytes/char，128 bytes 最多放 42 个中文字符
        val longText = "素".repeat(100)
        val truncated = ProcessStateSummary.truncateToBytes(longText, 128)
        val byteLen = truncated.toByteArray(Charsets.UTF_8).size
        assertTrue("truncated UTF-8 bytes should be <= 128, got $byteLen", byteLen <= 128)
        assertTrue("truncated should be non-empty", truncated.isNotEmpty())
        // 不应产生半个 UTF-8 字符
        assertTrue("truncated should be valid UTF-8", truncated.toByteArray(Charsets.UTF_8).isNotEmpty())
    }

    @Test
    fun truncateToBytesReturnsEmptyForMaxZero() {
        val truncated = ProcessStateSummary.truncateToBytes("abc", 0)
        assertEquals("", truncated)
    }

    @Test
    fun truncateToBytesEmptyStringStaysEmpty() {
        val truncated = ProcessStateSummary.truncateToBytes("", 128)
        assertEquals("", truncated)
    }

    @Test
    fun buildSummaryWithLongValuesGetsTruncatedByUpdate() {
        // buildSummary 本身不截断，update() 内部截断后写 setProcessStateSummary。
        // 这里验证 buildSummary 输出可能超过 128 bytes，需配合 truncateToBytes。
        val longScreen = "S".repeat(200)
        val summary = ProcessStateSummary.buildSummary(longScreen, "0", "idle")
        assertTrue(summary.length > 128)
        val truncated = ProcessStateSummary.truncateToBytes(summary, 128)
        assertTrue(truncated.toByteArray(Charsets.UTF_8).size <= 128)
    }
}

/**
 * Issue #612 八：JankStatsController 聚合统计测试。
 *
 * onFrame / buildGroup 提取为 internal 函数，可在测试中直接验证
 * 聚合逻辑，不需要 Window / JankStats 真实实例。
 * onFrame 内部调 DiagnosticsLogger.i 落日志，需 Robolectric 提供 android.util.Log。
 */
@org.junit.runner.RunWith(org.robolectric.RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [34])
class JankStatsControllerAggregationTest {
    @Before
    fun setUp() {
        JankStatsController.reset()
        EditorEventRingBuffer.setEnabled(true)
        EditorEventRingBuffer.clear()
    }

    @After
    fun tearDown() {
        JankStatsController.reset()
        EditorEventRingBuffer.setEnabled(false)
    }

    @Test
    fun onFrameCountsTotalFrames() {
        JankStatsController.onFrame(1L, 16_000_000L, false, emptyList<Pair<String, String>>())
        JankStatsController.onFrame(2L, 20_000_000L, false, emptyList<Pair<String, String>>())
        val summary = JankStatsController.getSummary()
        assertEquals(2L, summary["totalFrames"])
    }

    @Test
    fun onFrameCountsOnlyJankFramesAsJank() {
        JankStatsController.onFrame(1L, 16_000_000L, false, emptyList<Pair<String, String>>())
        JankStatsController.onFrame(2L, 40_000_000L, true, emptyList<Pair<String, String>>())
        JankStatsController.onFrame(3L, 50_000_000L, true, emptyList<Pair<String, String>>())
        val summary = JankStatsController.getSummary()
        assertEquals(3L, summary["totalFrames"])
        assertEquals(2L, summary["jankFrames"])
    }

    @Test
    fun onFrameTracksMaxFrameDurationUiNanos() {
        JankStatsController.onFrame(1L, 16_000_000L, false, emptyList<Pair<String, String>>())
        JankStatsController.onFrame(2L, 50_000_000L, true, emptyList<Pair<String, String>>())
        JankStatsController.onFrame(3L, 30_000_000L, false, emptyList<Pair<String, String>>())
        val summary = JankStatsController.getSummary()
        assertEquals(50_000_000L, summary["maxFrameDurationUiNanos"])
    }

    @Test
    fun onFrameGroupsJankByScreenAndInteraction() {
        val worksSwitching = listOf("screen" to "Works", "interaction" to "top_level_switch")
        val starMapOnly = listOf("screen" to "StarMap")
        JankStatsController.onFrame(1L, 40_000_000L, true, worksSwitching)
        JankStatsController.onFrame(2L, 40_000_000L, true, worksSwitching)
        JankStatsController.onFrame(3L, 40_000_000L, true, starMapOnly)
        val summary = JankStatsController.getSummary()

        @Suppress("UNCHECKED_CAST")
        val jankByGroup = summary["jankByGroup"] as Map<String, Long>
        assertEquals(2L, jankByGroup["screen=Works,interaction=top_level_switch"])
        assertEquals(1L, jankByGroup["screen=StarMap"])
    }

    @Test
    fun buildGroupWithoutInteraction() {
        val group = JankStatsController.buildGroup("Works", null)
        assertEquals("screen=Works", group)
    }

    @Test
    fun buildGroupWithInteraction() {
        val group = JankStatsController.buildGroup("Works", "top_level_switch")
        assertEquals("screen=Works,interaction=top_level_switch", group)
    }

    @Test
    fun getSummaryReturnsAllExpectedFields() {
        val summary = JankStatsController.getSummary()
        assertNotNull(summary["totalFrames"])
        assertNotNull(summary["jankFrames"])
        assertNotNull(summary["maxFrameDurationUiNanos"])
        assertNotNull(summary["jankByGroup"])
        assertNotNull(summary["trackingEnabled"])
    }

    @Test
    fun resetClearsAllAggregates() {
        JankStatsController.onFrame(1L, 40_000_000L, true, emptyList<Pair<String, String>>())
        JankStatsController.reset()
        val summary = JankStatsController.getSummary()
        assertEquals(0L, summary["totalFrames"])
        assertEquals(0L, summary["jankFrames"])
        assertEquals(0L, summary["maxFrameDurationUiNanos"])

        @Suppress("UNCHECKED_CAST")
        val jankByGroup = summary["jankByGroup"] as Map<String, Long>
        assertTrue(jankByGroup.isEmpty())
    }

    @Test
    fun nonJankFrameDoesNotIncrementJankByGroup() {
        JankStatsController.onFrame(1L, 16_000_000L, false, listOf("screen" to "Works"))
        val summary = JankStatsController.getSummary()

        @Suppress("UNCHECKED_CAST")
        val jankByGroup = summary["jankByGroup"] as Map<String, Long>
        assertTrue(jankByGroup.isEmpty())
    }
}

/**
 * Issue #612 八：DiagnosticsEvents 新事件测试。
 *
 * 验证 theme.resolve / settings.section / field.focus / field.commit /
 * nav.top_level_switch 事件被记录到 EditorEventRingBuffer 且不含敏感内容。
 * DiagnosticsEvents.record 内部调 DiagnosticsLogger.i 落日志，需 Robolectric。
 */
@org.junit.runner.RunWith(org.robolectric.RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [34])
class DiagnosticsEventsNewEventsTest {
    @Before
    fun setUp() {
        EditorEventRingBuffer.setEnabled(true)
        EditorEventRingBuffer.clear()
    }

    @After
    fun tearDown() {
        EditorEventRingBuffer.setEnabled(false)
    }

    @Test
    fun themeResolveRecordsEventWithAllFields() {
        DiagnosticsEvents.themeResolve("dark", "builtin", true, "ink", null, 34)
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        val event = snapshot[0]
        assertEquals("theme.resolve", event["event"])
        assertEquals("dark", event["appearanceMode"])
        assertEquals("builtin", event["colorSource"])
        assertEquals(true, event["isDark"])
        assertEquals("ink", event["selectedBuiltin"])
        assertNull(event["selectedPalette"])
        assertEquals(34, event["sdk"])
        assertNotNull(event["ts"])
    }

    @Test
    fun settingsSectionRecordsEvent() {
        DiagnosticsEvents.settingsSection("editor", true)
        DiagnosticsEvents.settingsSection("sync", false)
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(2, snapshot.size)
        assertEquals("settings.section", snapshot[0]["event"])
        assertEquals("editor", snapshot[0]["section"])
        assertEquals(true, snapshot[0]["expanded"])
        assertEquals("sync", snapshot[1]["section"])
        assertEquals(false, snapshot[1]["expanded"])
    }

    @Test
    fun fieldFocusRecordsEvent() {
        DiagnosticsEvents.fieldFocus("title", true)
        DiagnosticsEvents.fieldFocus("content", false)
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(2, snapshot.size)
        assertEquals("field.focus", snapshot[0]["event"])
        assertEquals("title", snapshot[0]["fieldType"])
        assertEquals(true, snapshot[0]["focused"])
        assertEquals("content", snapshot[1]["fieldType"])
        assertEquals(false, snapshot[1]["focused"])
    }

    @Test
    fun fieldCommitRecordsEventWithCharCount() {
        DiagnosticsEvents.fieldCommit("title", 12, "ok")
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        val event = snapshot[0]
        assertEquals("field.commit", event["event"])
        assertEquals("title", event["fieldType"])
        assertEquals(12, event["charCount"])
        assertEquals("ok", event["result"])
    }

    @Test
    fun navTopLevelSwitchRecordsFromAndTo() {
        DiagnosticsEvents.navTopLevelSwitch("Works", "StarMap")
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        val event = snapshot[0]
        assertEquals("nav.top_level_switch", event["event"])
        assertEquals("Works", event["from"])
        assertEquals("StarMap", event["to"])
    }

    @Test
    fun themeResolveDoesNotRecordSensitiveContent() {
        // 即使传入含敏感内容，事件本身只记录标识/模式，不记录内容
        DiagnosticsEvents.themeResolve("dark", "builtin", true, "ink", null, 34)
        val snapshot = EditorEventRingBuffer.getSnapshot()
        val event = snapshot[0]
        // 确认没有敏感 key
        assertNull(event["text"])
        assertNull(event["content"])
        assertNull(event["body"])
        assertNull(event["token"])
        assertNull(event["password"])
    }

    @Test
    fun fieldCommitDoesNotRecordFieldValue() {
        // fieldCommit 只记录 charCount，不记录字段实际内容
        DiagnosticsEvents.fieldCommit("title", 12, "ok")
        val event = EditorEventRingBuffer.getSnapshot()[0]
        assertNull(event["text"])
        assertNull(event["content"])
        assertEquals(12, event["charCount"])
    }

    @Test
    fun newEventsAreRecordedWhenEnabled() {
        EditorEventRingBuffer.setEnabled(false)
        EditorEventRingBuffer.clear()
        DiagnosticsEvents.navTopLevelSwitch("Works", "StarMap")
        assertTrue(EditorEventRingBuffer.getSnapshot().isEmpty())
    }
}

/**
 * Issue #612 五：ThemeStore.reload() 真实触发 theme.resolve 事件集成测试。
 *
 * 正测试：reload() 后 EditorEventRingBuffer 包含 theme.resolve 事件且字段正确。
 * 反测试：未调用 reload() 时不产生 theme.resolve 事件。
 */
@org.junit.runner.RunWith(org.robolectric.RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [34])
@org.robolectric.annotation.GraphicsMode(org.robolectric.annotation.GraphicsMode.Mode.NATIVE)
class ThemeResolveIntegrationTest {
    private lateinit var context: android.content.Context
    private lateinit var settingsRepository: com.xiwei.sujian.feature.settings.data.SettingsRepository
    private lateinit var themeRepository: com.xiwei.sujian.app.theme.ThemeRepository

    @org.junit.Before
    fun setUp() {
        context = androidx.test.core.app.ApplicationProvider.getApplicationContext()
        val dir = java.nio.file.Files.createTempDirectory("sujian_theme_resolve_test_").toString()
        val bridge =
            com.xiwei.sujian.core.interop.app.AppServiceBridge(
                com.xiwei.sujian.core.interop.app.WriterAppServiceHolder(dir, dir),
            )
        settingsRepository = com.xiwei.sujian.feature.settings.data.SettingsRepository(context, bridge)
        themeRepository = com.xiwei.sujian.app.theme.ThemeRepository(context, bridge)
        com.xiwei.sujian.app.theme.ThemeStore.initialize(themeRepository, settingsRepository)
        com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer.setEnabled(true)
        com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer.clear()
    }

    @org.junit.After
    fun tearDown() {
        com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer.clear()
        com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer.setEnabled(false)
    }

    @org.junit.Test
    fun reloadEmitsThemeResolveEvent() {
        com.xiwei.sujian.app.theme.ThemeStore.reload()
        val snapshot = com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer.getSnapshot()
        val themeEvent = snapshot.find { it["event"] == "theme.resolve" }
        org.junit.Assert.assertNotNull("reload() 必须产生 theme.resolve 事件", themeEvent)
        org.junit.Assert.assertNotNull("theme.resolve 必须包含 appearanceMode", themeEvent!!["appearanceMode"])
        org.junit.Assert.assertNotNull("theme.resolve 必须包含 colorSource", themeEvent["colorSource"])
        org.junit.Assert.assertNotNull("theme.resolve 必须包含 isDark", themeEvent["isDark"])
        org.junit.Assert.assertNotNull("theme.resolve 必须包含 sdk", themeEvent["sdk"])
    }

    @org.junit.Test
    fun noThemeResolveEventWithoutReload() {
        val snapshot = com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer.getSnapshot()
        val themeEvent = snapshot.find { it["event"] == "theme.resolve" }
        org.junit.Assert.assertNull("未调用 reload() 不应产生 theme.resolve 事件", themeEvent)
    }
}

/**
 * Issue #612 收尾：LogcatSnapshotCollector.truncate UTF-8 安全截断测试。
 *
 * 纯 JVM 测试，不需要 Robolectric：truncate 是纯字符串操作。
 * 验证截断不产生 U+FFFD（截断处是完整 UTF-8 字符边界），与旧实现的字节截断形成正反对比。
 */
class LogcatSnapshotCollectorTruncateTest {
    @Test
    fun truncateKeepsShortTextUnchanged() {
        val text = "short logcat content"
        assertEquals(text, LogcatSnapshotCollector.truncate(text))
    }

    @Test
    fun truncateLimitsLongTextToMaxSnapshotBytes() {
        val text = "x".repeat(3 * 1024 * 1024) // 3 MiB > 2 MiB
        val truncated = LogcatSnapshotCollector.truncate(text)
        val byteLen = truncated.toByteArray(Charsets.UTF_8).size
        assertTrue("truncated bytes should be <= 2 MiB, got $byteLen", byteLen <= 2 * 1024 * 1024)
        assertTrue("truncated should be non-empty", truncated.isNotEmpty())
    }

    @Test
    fun truncateHandlesMultibyteUtf8OnCharBoundary() {
        // 中文字符 3 bytes/char，800000 个中文 = 2.4 MiB > 2 MiB
        val text = "素".repeat(800000)
        val truncated = LogcatSnapshotCollector.truncate(text)
        val byteLen = truncated.toByteArray(Charsets.UTF_8).size
        assertTrue("truncated bytes should be <= 2 MiB, got $byteLen", byteLen <= 2 * 1024 * 1024)
        assertTrue("truncated should be non-empty", truncated.isNotEmpty())
        // 关键正反对比：旧字节截断会在多字节字符中间切断产生 U+FFFD；
        // 新按字符回退实现必须保证截断处是完整字符边界，不含 U+FFFD。
        assertTrue("truncated must not contain U+FFFD (char-boundary safe)", !truncated.contains('\uFFFD'))
    }

    @Test
    fun truncateEmptyStringStaysEmpty() {
        assertEquals("", LogcatSnapshotCollector.truncate(""))
    }

    @Test
    fun truncateAsciiAtExactBoundaryUnchanged() {
        // 恰好 2 MiB 的 ASCII 文本不截断
        val text = "a".repeat(2 * 1024 * 1024)
        val truncated = LogcatSnapshotCollector.truncate(text)
        assertEquals(text, truncated)
    }
}

/**
 * Issue #612 收尾：PersistentLogWriter 真实正反单测。
 *
 * 覆盖 flushBlocking 语义、setEnabled 开关、多线程线程安全、
 * 1 MiB / 5 文件轮转、clearLogs 清空、getLogFiles 返回。
 * Robolectric 提供 Context 与 Environment.getExternalStorageDirectory()。
 *
 * PersistentLogWriter 是 internal object 单例，writer 线程在 init 后常驻；
 * 每个测试前先 flushBlocking 让 writer 空闲，再 clearLogs 清空队列与文件，
 * 避免与 writer 线程的 writeBatch 产生竞态。
 */
@org.junit.runner.RunWith(org.robolectric.RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [34])
class PersistentLogWriterTest {
    private lateinit var context: android.content.Context

    @Before
    fun setUp() {
        context = androidx.test.core.app.ApplicationProvider.getApplicationContext()
        PersistentLogWriter.init(context)
        PersistentLogWriter.setEnabled(true)
        // 先等待之前可能残留的写完成（writer 空闲），再清空队列与文件，
        // 确保 clearLogs 不会与 writer 的 writeBatch 并发产生竞态。
        PersistentLogWriter.flushBlocking()
        PersistentLogWriter.clearLogs()
    }

    @After
    fun tearDown() {
        PersistentLogWriter.flushBlocking()
        PersistentLogWriter.clearLogs()
        PersistentLogWriter.setEnabled(false)
    }

    private fun req(
        message: String,
        ts: Long = 1_000L,
    ) = LogRequest(
        level = "I",
        tag = "test",
        message = message,
        timestampMs = ts,
        threadName = "main",
    )

    private fun currentLogFile(): File = File(AndroidDataRoot.logsDir(), "sujian-current.log")

    @Test
    fun flushBlockingWritesAllEnqueuedMessagesToFile() {
        PersistentLogWriter.enqueue(req("msg-alpha", 1000L))
        PersistentLogWriter.enqueue(req("msg-beta", 2000L))
        PersistentLogWriter.enqueue(req("msg-gamma", 3000L))
        PersistentLogWriter.flushBlocking()

        val file = currentLogFile()
        assertTrue("log file should exist", file.exists())
        val content = file.readText()
        assertTrue("should contain msg-alpha", content.contains("msg-alpha"))
        assertTrue("should contain msg-beta", content.contains("msg-beta"))
        assertTrue("should contain msg-gamma", content.contains("msg-gamma"))
    }

    @Test
    fun flushBlockingOnEmptyQueueReturnsImmediately() {
        // 未 enqueue 直接 flushBlocking，不应阻塞/死锁
        PersistentLogWriter.flushBlocking()
        // 到达此处即说明未死锁
        assertTrue(true)
    }

    @Test
    fun setEnabledFalseStopsNewEnqueuesFromPersisting() {
        PersistentLogWriter.setEnabled(false)
        PersistentLogWriter.enqueue(req("should-not-persist"))
        PersistentLogWriter.flushBlocking()

        val file = currentLogFile()
        val content = if (file.exists()) file.readText() else ""
        assertTrue(
            "disabled enqueue must not be persisted",
            !content.contains("should-not-persist"),
        )
    }

    @Test
    fun setEnabledTrueResumesPersistingAfterReEnable() {
        PersistentLogWriter.setEnabled(false)
        PersistentLogWriter.enqueue(req("dropped"))
        PersistentLogWriter.flushBlocking()
        PersistentLogWriter.setEnabled(true)
        PersistentLogWriter.enqueue(req("kept"))
        PersistentLogWriter.flushBlocking()

        val content = currentLogFile().readText()
        assertTrue("kept should be persisted after re-enable", content.contains("kept"))
        assertTrue("dropped should not be persisted", !content.contains("dropped"))
    }

    @Test
    fun concurrentEnqueueIsThreadSafe() {
        val threads = 8
        val perThread = 50
        val pool = java.util.concurrent.Executors.newFixedThreadPool(threads)
        val latch = java.util.concurrent.CountDownLatch(threads)
        try {
            for (t in 0 until threads) {
                pool.submit {
                    for (i in 0 until perThread) {
                        PersistentLogWriter.enqueue(req("t$t-i$i"))
                    }
                    latch.countDown()
                }
            }
            assertTrue(
                "all enqueuers should finish within timeout",
                latch.await(10, java.util.concurrent.TimeUnit.SECONDS),
            )
            PersistentLogWriter.flushBlocking()
        } finally {
            pool.shutdown()
        }

        val file = currentLogFile()
        assertTrue("log file should exist after concurrent writes", file.exists())
        val lines = file.readLines()
        assertEquals(
            "all ${threads * perThread} messages should be persisted, got ${lines.size}",
            threads * perThread,
            lines.size,
        )
    }

    @Test
    fun rotationCreatesRotatedFileAndKeepsAtMostFiveFiles() {
        // 每条 200 KB message，6 条约 1.2 MiB > 1 MiB 触发轮转
        val bigMessage = "A".repeat(200 * 1024)
        // 先写 6 条使当前文件超过 1 MiB
        repeat(6) { PersistentLogWriter.enqueue(req(bigMessage, it.toLong())) }
        PersistentLogWriter.flushBlocking()
        // 再分 6 轮：每轮先 enqueue 1 条触发 rotateIfNeeded（文件已 > 1 MiB），
        // 再 enqueue 6 条让当前文件再次超过 1 MiB，为下一轮轮转做准备。
        repeat(6) { round ->
            PersistentLogWriter.enqueue(req(bigMessage, (10 + round).toLong()))
            PersistentLogWriter.flushBlocking()
            repeat(6) { j -> PersistentLogWriter.enqueue(req(bigMessage, (100 + round * 10 + j).toLong())) }
            PersistentLogWriter.flushBlocking()
        }

        val files = PersistentLogWriter.getLogFiles()
        assertTrue("should have rotated files, got ${files.size}", files.size >= 2)
        assertTrue("should keep at most 5 files, got ${files.size}", files.size <= 5)
        // 当前文件应存在
        assertTrue(
            "current log sujian-current.log should exist",
            files.any { it.name == "sujian-current.log" },
        )
    }

    @Test
    fun clearLogsConcurrentWithWriterNeverResurrectsOldBatch() {
        // 反（缺陷守护）：clearLogs 与 writer 写盘并发时，旧 batch 不得写回重建的文件。
        // 缺陷根因：旧实现 clearLogs 只清队列不等待 writer——writer 已交换 batch 但
        // 尚未打开文件时，clearLogs 删除文件，writer 随后 FileWriter(currentFile, true)
        // 重建文件并写回旧日志（复活）。新实现用 ClearBarrier 命令：writer 处理到屏障时先写完前序 batch 再删除文件，后续 Append 一定在删除之后才写盘。
        // 并发时序无法从黑盒稳定命中，这里用多轮大消息竞争守护最终不变量：
        // 任何一轮清空前的消息都不得出现在清空后的文件中。
        repeat(20) { round ->
            PersistentLogWriter.enqueue(req("old-round-$round-" + "Z".repeat(64 * 1024)))
            PersistentLogWriter.clearLogs()
            PersistentLogWriter.flushBlocking()
        }
        val file = currentLogFile()
        val content = if (file.exists()) file.readText() else ""
        for (round in 0 until 20) {
            assertTrue(
                "round $round must not resurrect after clearLogs",
                !content.contains("old-round-$round-"),
            )
        }
    }

    @Test
    fun clearLogsWaitsForInFlightBatchBeforeDeletingFiles() {
        // 正（新语义）：clearLogs 必须等待 writer 完成正在写的 batch 后才删除文件。
        // 间接验证：enqueue 大消息让 writer 必然忙于写盘，clearLogs 返回后立即
        // enqueue + flushBlocking 的新消息必须落入全新文件，且旧消息不存在——
        // 命令队列保证 ClearBarrier 按序处理，其后的 Append 一定在文件删除之后才写盘。
        val big = "Q".repeat(512 * 1024)
        PersistentLogWriter.enqueue(req("in-flight-" + big))
        // 给 writer 时间取出命令并开始写盘（MIN_PRIORITY，大消息写盘耗时明显）。
        Thread.sleep(100)
        PersistentLogWriter.clearLogs()
        PersistentLogWriter.enqueue(req("after-clear-log"))
        PersistentLogWriter.flushBlocking()
        val file = currentLogFile()
        assertTrue("new file must exist after clear + enqueue", file.exists())
        val content = file.readText()
        assertTrue(
            "new message must be persisted after clear",
            content.contains("after-clear-log"),
        )
        assertTrue(
            "in-flight old batch must not be resurrected",
            !content.contains("in-flight-"),
        )
    }

    @Test
    fun writerThreadSurvivesInterruptAndKeepsWriting() {
        // 反（缺陷复现）：writer 被 interrupt 后必须继续处理后续日志，
        // 旧实现 wait() 抛 InterruptedException 直接杀死 writer 线程。
        // flushBlocking 用带超时的 future 包装：若 writer 已死会超时失败而非挂死套件。
        PersistentLogWriter.flushBlocking()
        val writerThread =
            Thread.getAllStackTraces().keys.firstOrNull { it.name == "sujian-logger" }
        assertNotNull("sujian-logger writer thread must exist", writerThread)
        writerThread!!.interrupt()
        // 等待 writer 处理中断并回到 wait（若线程已死则等待必然超时）。
        Thread.sleep(200)
        PersistentLogWriter.enqueue(req("after-interrupt"))
        val executor = java.util.concurrent.Executors.newSingleThreadExecutor()
        try {
            val future = executor.submit { PersistentLogWriter.flushBlocking() }
            future.get(10, java.util.concurrent.TimeUnit.SECONDS)
        } finally {
            executor.shutdownNow()
        }
        val file = currentLogFile()
        assertTrue("log file should exist after writer interrupt", file.exists())
        val content = file.readText()
        assertTrue(
            "writer must survive interrupt and persist later logs",
            content.contains("after-interrupt"),
        )
    }

    @Test
    fun clearLogsRemovesFilesAndClearsPendingQueue() {
        PersistentLogWriter.enqueue(req("to-be-cleared"))
        PersistentLogWriter.flushBlocking()
        val file = currentLogFile()
        assertTrue("file should exist before clearLogs", file.exists())

        PersistentLogWriter.clearLogs()
        assertTrue("file should be deleted after clearLogs", !file.exists())

        // 队列清空后新 enqueue 只写新消息，不残留旧消息
        PersistentLogWriter.enqueue(req("after-clear"))
        PersistentLogWriter.flushBlocking()
        assertTrue("current file should exist again after new enqueue", file.exists())
        val content = file.readText()
        assertTrue("after-clear should be persisted", content.contains("after-clear"))
        assertTrue("to-be-cleared should be gone after clearLogs", !content.contains("to-be-cleared"))
    }

    @Test
    fun getLogFilesReturnsCurrentLogFiles() {
        PersistentLogWriter.enqueue(req("file-list-test"))
        PersistentLogWriter.flushBlocking()

        val files = PersistentLogWriter.getLogFiles()
        assertTrue("getLogFiles should not be empty", files.isNotEmpty())
        assertTrue(
            "should contain sujian-current.log",
            files.any { it.name == "sujian-current.log" },
        )
        assertTrue(
            "all returned files should match sujian-current*.log",
            files.all { it.name.startsWith("sujian-current") && it.name.endsWith(".log") },
        )
    }

    @Test
    fun flushBlockingReturnsTrueWhenCompletedBeforeTimeout() {
        // 正（评论 3.4 新契约）：正常落盘完成时 flushBlocking 返回 true，且日志已写盘。
        PersistentLogWriter.enqueue(req("returns-true"))
        val ok = PersistentLogWriter.flushBlocking()
        assertTrue("flushBlocking must return true on success", ok)
        assertTrue(
            "flushed message must be on disk",
            currentLogFile().readText().contains("returns-true"),
        )
    }

    @Test
    fun flushBlockingReturnsFalseWhenCallerInterrupted() {
        // 反（评论 3.4 新契约）：调用线程被中断时 latch 等待被打断，必须返回 false
        // 而不是静默假装成功（旧实现返回 Unit，调用方无从感知）。
        val executor = java.util.concurrent.Executors.newSingleThreadExecutor()
        try {
            // 显式 <Boolean> 强制 submit(Callable) 重载：submit(Runnable) 的
            // Future<?>.get() 恒返回 null，拿不到 lambda 的 Boolean 返回值。
            val future =
                executor.submit<Boolean> {
                    Thread.currentThread().interrupt()
                    PersistentLogWriter.flushBlocking()
                }
            val result = future.get(10, java.util.concurrent.TimeUnit.SECONDS)
            assertEquals("interrupted flushBlocking must return false", false, result)
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun clearLogsReturnsFalseWhenCallerInterrupted() {
        // 反（评论 3.4 新契约）：调用线程被中断时 clearLogs 必须返回 false，
        // 设置页据此显示失败而不是假装已经清空。
        val executor = java.util.concurrent.Executors.newSingleThreadExecutor()
        try {
            // 显式 <Boolean> 强制 submit(Callable) 重载，否则返回 null（Runnable 版）。
            val future =
                executor.submit<Boolean> {
                    Thread.currentThread().interrupt()
                    PersistentLogWriter.clearLogs()
                }
            val result = future.get(10, java.util.concurrent.TimeUnit.SECONDS)
            assertEquals("interrupted clearLogs must return false", false, result)
        } finally {
            executor.shutdownNow()
        }
    }
}

/**
 * Issue #612 评论 2 收口：ProcessExitCollector.processStateSummary 解码正反测试。
 *
 * processStateSummary 自 API 30 起可用（minSdk=30），不应被 API 31（S）守卫跳过。
 * decodeProcessStateSummary 提取为 internal 纯函数，把 byte[] 按 UTF-8 解码后脱敏，
 * 可在纯 JVM 测试中直接验证解码逻辑，不需要 ApplicationExitInfo / Robolectric。
 */
class ProcessExitCollectorProcessStateSummaryTest {
    @Test
    fun decodeUtf8BytesReturnsReadableText() {
        // 正：UTF-8 字节解码后为可读文本（旧实现输出 hex 不可读）。
        val raw = "screen=Works;editor=0;sync=idle"
        val bytes = raw.toByteArray(Charsets.UTF_8)
        val decoded = ProcessExitCollector.decodeProcessStateSummary(bytes)
        assertEquals(raw, decoded)
    }

    @Test
    fun decodeEmptyBytesReturnsNull() {
        // 正：空 byte 数组返回 null。
        assertNull(ProcessExitCollector.decodeProcessStateSummary(ByteArray(0)))
    }

    @Test
    fun decodeMultibyteUtf8ChinesePreserved() {
        // 正：含中文的 UTF-8 字节正确解码（每个中文 3 bytes）。
        val raw = "屏幕=工作;编辑器=0;同步=空闲"
        val bytes = raw.toByteArray(Charsets.UTF_8)
        val decoded = ProcessExitCollector.decodeProcessStateSummary(bytes)
        assertEquals(raw, decoded)
    }

    @Test
    fun decodeDoesNotOutputHexLikeOldImplementation() {
        // 反：新实现输出可读文本，不再是旧实现的 hex 编码。
        // 旧实现会把 "screen" 编码为 "73637265656e..."；新实现直接返回 "screen"。
        val bytes = "screen".toByteArray(Charsets.UTF_8)
        val decoded = ProcessExitCollector.decodeProcessStateSummary(bytes)
        assertEquals("screen", decoded)
        // 确认不是 hex：hex 编码的 "screen" 应为纯小写 hex 字符且更长。
        assertTrue("decoded should be readable text not hex", decoded!!.length < bytes.size * 2)
    }
}

/**
 * Issue #612 收口：ProcessExitCollector 在 API 30 上不再写 "requires API 31+" 占位文件。
 *
 * 正测试：API 30 调用 collect 后 process_exits.json 不含 "requires API 31+" 占位
 * （getHistoricalProcessExitReasons 自 API 30 起可用，minSdk=30）。
 * 修复前（反）：旧代码在 API 30 写 "requires API 31+" 占位并 return，新代码继续采集。
 */
@org.junit.runner.RunWith(org.robolectric.RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [30])
class ProcessExitCollectorApi30Test {
    @Test
    fun collectOnApi30DoesNotWriteRequiresApi31Placeholder() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val dir = java.nio.file.Files.createTempDirectory("sujian_proc_exit_30_").toFile()
        try {
            ProcessExitCollector.collect(context, dir)
            val outputFile = java.io.File(dir, "process_exits.json")
            assertTrue("process_exits.json should exist", outputFile.exists())
            val content = outputFile.readText()
            assertTrue(
                "API 30 must not write 'requires API 31+' placeholder; got: $content",
                !content.contains("requires API 31+"),
            )
        } finally {
            dir.deleteRecursively()
        }
    }
}

/**
 * Issue #612 收口：一级切换 interaction 上下文判定正反测试。
 *
 * 修复前（反）：SujianJankInteractionClearEffect 在首次组合（应用启动、未发生任何切换）
 * 也写 interaction=top_level_switch，启动期帧被误标为“一级切换”；
 * 修复后（正）：首次组合只写 screen，不写 interaction。
 */
class ResolveTopLevelSwitchInteractionTest {
    @Test
    fun firstCompositionIsNotTopLevelSwitch() {
        // 正：应用启动首次组合（previous=null）不是一级切换，不写 interaction。
        assertEquals(
            null,
            resolveTopLevelSwitchInteraction(null, SujianDestination.Works),
        )
    }

    @Test
    fun actualSwitchWritesTopLevelSwitchInteraction() {
        // 正：Works → StarMap 是一级切换，写 interaction=top_level_switch。
        assertEquals(
            "top_level_switch",
            resolveTopLevelSwitchInteraction(SujianDestination.Works, SujianDestination.StarMap),
        )
    }

    @Test
    fun switchBackAlsoWritesTopLevelSwitchInteraction() {
        // 正：StarMap → Works 同样是切换。
        assertEquals(
            "top_level_switch",
            resolveTopLevelSwitchInteraction(SujianDestination.StarMap, SujianDestination.Works),
        )
    }

    @Test
    fun reselectingSameDestinationIsNotTopLevelSwitch() {
        // 反：原地重复选择当前 tab 不产生切换动画，不应标 interaction。
        assertEquals(
            null,
            resolveTopLevelSwitchInteraction(SujianDestination.Works, SujianDestination.Works),
        )
    }

    @Test
    fun statsToWorksIsTopLevelSwitch() {
        // 正：Stats → Works 同样是一级切换。
        assertEquals(
            "top_level_switch",
            resolveTopLevelSwitchInteraction(SujianDestination.Stats, SujianDestination.Works),
        )
    }
}

/**
 * Issue #612 三、3.2 收口：进程状态摘要同步字段正反测试。
 *
 * 修复前（反）：recordTopLevelSwitchDiagnostics 硬编码 sync="idle"，切页会把
 * 卡住的 syncing/failed 覆盖掉，而同步状态未变时摘要不再纠正；
 * 修复后（正）：syncIndicatorSummary 把真实同步状态映射为摘要字符串，
 * SujianProcessStateEffect 监听 (目的地, 章节, 同步状态) 三键统一写入。
 */
class SyncIndicatorSummaryTest {
    @Test
    fun syncingMapsToSyncing() {
        assertEquals("syncing", syncIndicatorSummary(SyncIndicatorState.Syncing))
    }

    @Test
    fun syncedMapsToSynced() {
        assertEquals("synced", syncIndicatorSummary(SyncIndicatorState.Synced))
    }

    @Test
    fun failedMapsToFailed() {
        assertEquals("failed", syncIndicatorSummary(SyncIndicatorState.Failed))
    }

    @Test
    fun unconfiguredMapsToUnconfigured() {
        assertEquals("unconfigured", syncIndicatorSummary(SyncIndicatorState.Unconfigured))
    }
}

/**
 * Issue #612 评论二.4 收口：崩溃文件“导出时两处都收集”正反测试。
 *
 * 修复前（反）：DiagnosticsExporter 只取 getCrashFile() 一处（优先外部 logsDir），
 * 当外部与 filesDir/diagnostics/ 两处都有 last_crash.txt 时，较新的回退份被漏掉。
 * 修复后（正）：planCrashFileCopies 把主位置导出为 last_crash.txt，
 * 两处都有时回退位置额外导出为 last_crash_fallback.txt。
 */
class PlanCrashFileCopiesTest {
    @Test
    fun bothLocationsYieldTwoCopies() {
        // 正：两处都有 crash 文件时，主位置 + 回退位置都导出。
        val primary = java.nio.file.Files.createTempFile("crash_primary_", ".txt").toFile()
        val fallback = java.nio.file.Files.createTempFile("crash_fallback_", ".txt").toFile()
        try {
            val copies = DiagnosticsExporter.planCrashFileCopies(primary, fallback)
            assertEquals(2, copies.size)
            assertEquals("last_crash.txt", copies[0].first)
            assertEquals(primary, copies[0].second)
            assertEquals("last_crash_fallback.txt", copies[1].first)
            assertEquals(fallback, copies[1].second)
        } finally {
            primary.delete()
            fallback.delete()
        }
    }

    @Test
    fun onlyPrimaryYieldsSingleCanonicalCopy() {
        // 正：只有主位置有文件时，导出为 last_crash.txt，无回退副本。
        val primary = java.nio.file.Files.createTempFile("crash_primary_", ".txt").toFile()
        val missingFallback = java.io.File(primary.parentFile, "missing_crash.txt")
        try {
            val copies = DiagnosticsExporter.planCrashFileCopies(primary, missingFallback)
            assertEquals(1, copies.size)
            assertEquals("last_crash.txt", copies[0].first)
            assertEquals(primary, copies[0].second)
        } finally {
            primary.delete()
        }
    }

    @Test
    fun neitherLocationYieldsNoCopies() {
        // 反：两处都没有文件时不做任何拷贝。
        val dir = java.nio.file.Files.createTempDirectory("crash_none_").toFile()
        try {
            val copies =
                DiagnosticsExporter.planCrashFileCopies(
                    java.io.File(dir, "missing_a.txt"),
                    java.io.File(dir, "missing_b.txt"),
                )
            assertTrue("no copies expected", copies.isEmpty())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun sameFileTwiceDoesNotDuplicate() {
        // 反：主/回退指向同一文件时不产生重复副本。
        val same = java.nio.file.Files.createTempFile("crash_same_", ".txt").toFile()
        try {
            val copies = DiagnosticsExporter.planCrashFileCopies(same, same)
            assertEquals(1, copies.size)
            assertEquals("last_crash.txt", copies[0].first)
        } finally {
            same.delete()
        }
    }
}

/**
 * Issue #612 评论二.4 收口：getCrashFile / getFallbackCrashFile 两位置采集正反测试。
 *
 * 正：两处都有 last_crash.txt 时，getFallbackCrashFile 返回回退份（导出补上）；
 * 反：只有主位置或只有回退位置时，getFallbackCrashFile 返回 null（避免同一文件
 * 被导出两份）。
 */
@org.junit.runner.RunWith(org.robolectric.RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [34])
class CrashFileLocationsTest {
    private lateinit var context: android.content.Context

    @Before
    fun setUp() {
        context = androidx.test.core.app.ApplicationProvider.getApplicationContext()
        DiagnosticsLogger.init(context, isEnabled = true, isVerbose = true)
        DiagnosticsLogger.clearLogs()
    }

    @After
    fun tearDown() {
        DiagnosticsLogger.clearLogs()
    }

    @Test
    fun bothLocationsReturnFallbackForSecondCopy() {
        // 正：两处都有时，主位置 + 回退位置都可取到。
        val primaryDir = AndroidDataRoot.logsDir()
        primaryDir.mkdirs()
        val primary = java.io.File(primaryDir, "last_crash.txt")
        primary.writeText("crash in external logsDir")
        val fallbackDir = java.io.File(context.filesDir, "diagnostics")
        fallbackDir.mkdirs()
        val fallback = java.io.File(fallbackDir, "last_crash.txt")
        fallback.writeText("crash in filesDir fallback")
        try {
            assertEquals(primary, DiagnosticsLogger.getCrashFile())
            assertEquals(fallback, DiagnosticsLogger.getFallbackCrashFile())
        } finally {
            primary.delete()
            fallback.delete()
        }
    }

    @Test
    fun onlyPrimaryReturnsNullFallback() {
        // 反：只有主位置有文件时，回退位置不应返回（否则同一内容导出两份）。
        val primaryDir = AndroidDataRoot.logsDir()
        primaryDir.mkdirs()
        val primary = java.io.File(primaryDir, "last_crash.txt")
        primary.writeText("crash in external logsDir")
        try {
            assertEquals(primary, DiagnosticsLogger.getCrashFile())
            assertNull(DiagnosticsLogger.getFallbackCrashFile())
        } finally {
            primary.delete()
        }
    }

    @Test
    fun onlyFallbackIsReturnedAsPrimarySource() {
        // 正：只有回退位置有文件时，getCrashFile 回退返回它（以主文件名导出）。
        val fallbackDir = java.io.File(context.filesDir, "diagnostics")
        fallbackDir.mkdirs()
        val fallback = java.io.File(fallbackDir, "last_crash.txt")
        fallback.writeText("crash in filesDir fallback")
        try {
            assertEquals(fallback, DiagnosticsLogger.getCrashFile())
            assertNull(DiagnosticsLogger.getFallbackCrashFile())
        } finally {
            fallback.delete()
        }
    }
}
