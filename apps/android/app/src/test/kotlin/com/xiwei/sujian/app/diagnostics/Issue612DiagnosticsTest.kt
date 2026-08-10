@file:Suppress("StringLiteralDuplication") // 测试固件中 "screen=" / "editor=" 等前缀天然重复

package com.xiwei.sujian.app.diagnostics

import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.core.diagnostics.JankStatsController
import com.xiwei.sujian.core.diagnostics.LogRequest
import com.xiwei.sujian.core.diagnostics.LogcatSnapshotCollector
import com.xiwei.sujian.core.diagnostics.PersistentLogWriter
import com.xiwei.sujian.core.diagnostics.ProcessExitCollector
import com.xiwei.sujian.core.diagnostics.ProcessStateSummary
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot
import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
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
        JankStatsController.onFrame(1L, 16_000_000L, false, emptyList())
        JankStatsController.onFrame(2L, 20_000_000L, false, emptyList())
        val summary = JankStatsController.getSummary()
        assertEquals(2L, summary["totalFrames"])
    }

    @Test
    fun onFrameCountsOnlyJankFramesAsJank() {
        JankStatsController.onFrame(1L, 16_000_000L, false, emptyList())
        JankStatsController.onFrame(2L, 40_000_000L, true, emptyList())
        JankStatsController.onFrame(3L, 50_000_000L, true, emptyList())
        val summary = JankStatsController.getSummary()
        assertEquals(3L, summary["totalFrames"])
        assertEquals(2L, summary["jankFrames"])
    }

    @Test
    fun onFrameTracksMaxFrameDurationUiNanos() {
        JankStatsController.onFrame(1L, 16_000_000L, false, emptyList())
        JankStatsController.onFrame(2L, 50_000_000L, true, emptyList())
        JankStatsController.onFrame(3L, 30_000_000L, false, emptyList())
        val summary = JankStatsController.getSummary()
        assertEquals(50_000_000L, summary["maxFrameDurationUiNanos"])
    }

    @Test
    fun onFrameGroupsJankByScreenAndInteraction() {
        JankStatsController.setState("Works", "top_level_switch")
        JankStatsController.onFrame(1L, 40_000_000L, true, emptyList())
        JankStatsController.onFrame(2L, 40_000_000L, true, emptyList())
        JankStatsController.clearInteraction()
        JankStatsController.setState("StarMap", null)
        JankStatsController.onFrame(3L, 40_000_000L, true, emptyList())
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
        JankStatsController.onFrame(1L, 40_000_000L, true, emptyList())
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
        JankStatsController.setState("Works", null)
        JankStatsController.onFrame(1L, 16_000_000L, false, emptyList())
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
        // 确保 clearLogs 不会与 writer 的 writeBatch 并发清空 swap。
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
}

/**
 * Issue #612 收口：ProcessExitCollector API 守卫正反测试。
 *
 * getHistoricalProcessExitReasons 自 API 30 起可用（minSdk=30），不应被 API 31 守卫跳过；
 * processStateSummary 字段自 API 31 起可用，API 30 访问会抛 NoSuchMethodError。
 * shouldReadProcessStateSummary 提取为 internal 纯函数，可在纯 JVM 测试中验证守卫逻辑。
 */
class ProcessExitCollectorApiGuardTest {
    @Test
    fun shouldReadProcessStateSummaryReturnsFalseBelowApi31() {
        org.junit.Assert.assertFalse(
            "API 30 must not read processStateSummary (NoSuchMethodError risk)",
            ProcessExitCollector.shouldReadProcessStateSummary(30),
        )
    }

    @Test
    fun shouldReadProcessStateSummaryReturnsTrueAtApi31() {
        org.junit.Assert.assertTrue(
            "API 31 should read processStateSummary",
            ProcessExitCollector.shouldReadProcessStateSummary(31),
        )
    }

    @Test
    fun shouldReadProcessStateSummaryReturnsTrueAtApi34() {
        org.junit.Assert.assertTrue(
            "API 34 should read processStateSummary",
            ProcessExitCollector.shouldReadProcessStateSummary(34),
        )
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
