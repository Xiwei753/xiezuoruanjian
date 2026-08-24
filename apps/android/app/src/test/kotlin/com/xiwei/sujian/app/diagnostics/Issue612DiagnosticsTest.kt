@file:Suppress("StringLiteralDuplication") // 测试固件中 "screen=" / "editor=" 等前缀天然重复

package com.xiwei.sujian.app.diagnostics

import com.xiwei.sujian.app.navigation.SujianDestination
import com.xiwei.sujian.app.navigation.resolveTopLevelSwitchInteraction
import com.xiwei.sujian.app.navigation.syncIndicatorSummary
import com.xiwei.sujian.core.diagnostics.DiagnosticsBuildIdentity
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
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.ByteArrayInputStream
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

    // ── #614: sanitizeSummaryValue 字符白名单 ──────────────────────

    /** 应用内部枚举值与安全符号原样保留。 */
    @Test
    fun sanitizeSummaryValueKeepsAlphanumericAndSafeSymbols() {
        assertEquals("Works", ProcessStateSummary.sanitizeSummaryValue("Works"))
        assertEquals("0", ProcessStateSummary.sanitizeSummaryValue("0"))
        assertEquals("idle", ProcessStateSummary.sanitizeSummaryValue("idle"))
        assertEquals("syncing", ProcessStateSummary.sanitizeSummaryValue("syncing"))
        assertEquals("a-b_c_d_e", ProcessStateSummary.sanitizeSummaryValue("a-b_c=d;e"))
    }

    /** 含空格的值替换为 _，破坏 shell/SQL 注入语义。 */
    @Test
    fun sanitizeSummaryValueReplacesSpacesToBreakInjection() {
        val sanitized = ProcessStateSummary.sanitizeSummaryValue("Works; rm -rf /")
        assertFalse("空格应被替换", sanitized.contains(" "))
        assertTrue(sanitized.contains("Works"))
        assertTrue(sanitized.contains("rm"))
        assertTrue(sanitized.contains("-rf"))
    }

    /** 引号、尖括号、反斜杠替换为 _，防止 HTML/转义注入。 */
    @Test
    fun sanitizeSummaryValueReplacesQuotesAndAngleBrackets() {
        val sanitized = ProcessStateSummary.sanitizeSummaryValue("""<script>"x"</script>""")
        assertFalse(sanitized.contains("<"))
        assertFalse(sanitized.contains(">"))
        assertFalse(sanitized.contains("\""))
    }

    /** #614: 结构分隔符 = 和 ; 在 value 中替换为 _，防止注入破坏 screen=…;editor=…;sync=… 解析语义。 */
    @Test
    fun sanitizeSummaryValueReplacesStructureDelimitersToPreventInjection() {
        // '=' 是 key=value 分隔符，value 内出现会制造歧义键值对
        assertEquals("a_b", ProcessStateSummary.sanitizeSummaryValue("a=b"))
        // ';' 是字段分隔符，value 内出现会注入额外字段段
        assertEquals("a_b", ProcessStateSummary.sanitizeSummaryValue("a;b"))
        // 组合：screen 值含 "=;" 时全部替换，buildSummary 结构分隔符不受影响
        val summary = ProcessStateSummary.buildSummary("x=y;z", "0", "idle")
        assertEquals("screen=x_y_z;editor=0;sync=idle", summary)
    }

    /** buildSummary 对每个 value 单独 sanitize，含特殊字符的 value 不破坏整体格式。 */
    @Test
    fun buildSummarySanitizesEachValue() {
        val summary = ProcessStateSummary.buildSummary("Works; rm -rf", "0", "idle")
        assertFalse(summary.contains(" rm"))
        assertTrue(summary.startsWith("screen=Works"))
        assertTrue(summary.contains(";editor=0;sync=idle"))
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
        JankStatsController.onFrame(1L, 16_000_000L, false, "unknown", null)
        JankStatsController.onFrame(2L, 20_000_000L, false, "unknown", null)
        val summary = JankStatsController.getSummary()
        assertEquals(2L, summary["totalFrames"])
    }

    @Test
    fun onFrameCountsOnlyJankFramesAsJank() {
        JankStatsController.onFrame(1L, 16_000_000L, false, "unknown", null)
        JankStatsController.onFrame(2L, 40_000_000L, true, "unknown", null)
        JankStatsController.onFrame(3L, 50_000_000L, true, "unknown", null)
        val summary = JankStatsController.getSummary()
        assertEquals(3L, summary["totalFrames"])
        assertEquals(2L, summary["jankFrames"])
    }

    @Test
    fun onFrameTracksMaxFrameDurationUiNanos() {
        JankStatsController.onFrame(1L, 16_000_000L, false, "unknown", null)
        JankStatsController.onFrame(2L, 50_000_000L, true, "unknown", null)
        JankStatsController.onFrame(3L, 30_000_000L, false, "unknown", null)
        val summary = JankStatsController.getSummary()
        assertEquals(50_000_000L, summary["maxFrameDurationUiNanos"])
    }

    @Test
    fun onFrameGroupsJankByScreenAndInteraction() {
        JankStatsController.onFrame(1L, 40_000_000L, true, "Works", "top_level_switch")
        JankStatsController.onFrame(2L, 40_000_000L, true, "Works", "top_level_switch")
        JankStatsController.onFrame(3L, 40_000_000L, true, "StarMap", null)
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
        assertNotNull(summary["recentJankFrames"])
    }

    @Test
    fun resetClearsAllAggregates() {
        JankStatsController.onFrame(1L, 40_000_000L, true, "unknown", null)
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
        JankStatsController.onFrame(1L, 16_000_000L, false, "Works", null)
        val summary = JankStatsController.getSummary()

        @Suppress("UNCHECKED_CAST")
        val jankByGroup = summary["jankByGroup"] as Map<String, Long>
        assertTrue(jankByGroup.isEmpty())
    }

    @Test
    fun recentJankFramesCollectedAndCappedAt128() {
        // 正（评论 3.3）：jank 帧进入 recentJankFrames 环形缓冲，超过 128 条时丢弃最旧的
        repeat(150) { i ->
            JankStatsController.onFrame(i.toLong(), 40_000_000L, true, "Works", null)
        }
        val summary = JankStatsController.getSummary()

        @Suppress("UNCHECKED_CAST")
        val recent = summary["recentJankFrames"] as List<Map<String, Any?>>
        assertEquals(128, recent.size)
        // 最旧的 22 条被丢弃，保留 [22..149]
        assertEquals(22L, recent[0]["frameStartNanos"])
        assertEquals(149L, recent[127]["frameStartNanos"])
    }

    @Test
    fun nonJankFramesNotAddedToRecentJankFrames() {
        // 反（评论 3.3）：非 jank 帧不进入 recentJankFrames
        JankStatsController.onFrame(1L, 16_000_000L, false, "Works", null)
        val summary = JankStatsController.getSummary()

        @Suppress("UNCHECKED_CAST")
        val recent = summary["recentJankFrames"] as List<Map<String, Any?>>
        assertTrue(recent.isEmpty())
    }

    @Test
    fun recentJankFramesPreserveScreenAndInteraction() {
        // 正（评论 3.3）：recentJankFrames 保留 screen/interaction 上下文
        JankStatsController.onFrame(1L, 40_000_000L, true, "StarMap", "top_level_switch")
        val summary = JankStatsController.getSummary()

        @Suppress("UNCHECKED_CAST")
        val recent = summary["recentJankFrames"] as List<Map<String, Any?>>
        assertEquals(1, recent.size)
        assertEquals("StarMap", recent[0]["screen"])
        assertEquals("top_level_switch", recent[0]["interaction"])
    }

    @Test
    fun resetClearsRecentJankFrames() {
        // 正（评论 3.3）：reset 清空 recentJankFrames 环形缓冲
        JankStatsController.onFrame(1L, 40_000_000L, true, "Works", null)
        JankStatsController.reset()
        val summary = JankStatsController.getSummary()

        @Suppress("UNCHECKED_CAST")
        val recent = summary["recentJankFrames"] as List<Map<String, Any?>>
        assertTrue(recent.isEmpty())
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

    // ── #638 结构化持久编辑器诊断 ──────────────────────────────

    @Test
    fun editorMotionPolicyRecordsAllFields() {
        DiagnosticsEvents.editorMotionPolicy(
            textEnabled = true,
            textMs = 100L,
            cursorEnabled = true,
            cursorMs = 80L,
            coordinated = true,
            reduceMotion = false,
        )
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        val event = snapshot[0]
        assertEquals("editor.motion_policy", event["event"])
        assertEquals(true, event["textEnabled"])
        assertEquals(100L, event["textMs"])
        assertEquals(true, event["cursorEnabled"])
        assertEquals(80L, event["cursorMs"])
        assertEquals(true, event["coordinated"])
        assertEquals(false, event["reduceMotion"])
        assertNotNull(event["ts"])
    }

    @Test
    fun editorMotionPolicyDoesNotRecordSensitiveContent() {
        DiagnosticsEvents.editorMotionPolicy(
            textEnabled = true,
            textMs = 100L,
            cursorEnabled = true,
            cursorMs = 80L,
            coordinated = true,
            reduceMotion = false,
        )
        val event = EditorEventRingBuffer.getSnapshot()[0]
        // 确认不含敏感内容字段（正文/密码/token 等）
        assertNull(event["content"])
        assertNull(event["body"])
        assertNull(event["token"])
        assertNull(event["password"])
    }

    @Test
    fun editorTypographyRecordsAllFields() {
        DiagnosticsEvents.editorTypography(
            fontSizeSp = 18.5f,
            lineSpacing = 1.6f,
            firstLineIndent = true,
            indentChars = 2.0f,
        )
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        val event = snapshot[0]
        assertEquals("editor.typography", event["event"])
        assertEquals(18.5f, event["fontSizeSp"])
        assertEquals(1.6f, event["lineSpacing"])
        assertEquals(true, event["firstLineIndent"])
        assertEquals(2.0f, event["indentChars"])
        assertNotNull(event["ts"])
    }

    @Test
    fun editorTypographyRedactSensitiveFields() {
        DiagnosticsEvents.editorTypography(
            fontSizeSp = 16f,
            lineSpacing = 1.5f,
            firstLineIndent = false,
            indentChars = 0f,
        )
        val event = EditorEventRingBuffer.getSnapshot()[0]
        assertNull(event["text"])
        assertNull(event["content"])
        assertNull(event["body"])
    }

    @Test
    fun viewportRetargetRecordsAllFields() {
        DiagnosticsEvents.viewportRetarget(
            transactionId = 12345L,
            fromY = 100.5f,
            toY = 500.0f,
            maxY = 2000.0f,
            reason = "scroll",
        )
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        val event = snapshot[0]
        assertEquals("editor.viewport_retarget", event["event"])
        assertEquals(12345L, event["transaction"])
        assertEquals(100.5f, event["fromY"])
        assertEquals(500.0f, event["toY"])
        assertEquals(2000.0f, event["maxY"])
        assertEquals("scroll", event["reason"])
        assertNotNull(event["ts"])
    }

    @Test
    fun viewportRetargetRedactSensitiveFields() {
        DiagnosticsEvents.viewportRetarget(
            transactionId = 1L,
            fromY = 0f,
            toY = 100f,
            maxY = 500f,
            reason = "scroll",
        )
        val event = EditorEventRingBuffer.getSnapshot()[0]
        assertNull(event["text"])
        assertNull(event["content"])
        assertNull(event["body"])
    }

    @Test
    fun animationRebaseStateRecordsAllFields() {
        DiagnosticsEvents.animationRebaseState(
            oldTransactionId = 100L,
            newTransactionId = 101L,
            deleteSlices = 5,
            cursorRemaining = 0.12f,
            minSliceRemaining = 0f,
            maxSliceRemaining = 0.5f,
        )
        val snapshot = EditorEventRingBuffer.getSnapshot()
        assertEquals(1, snapshot.size)
        val event = snapshot[0]
        assertEquals("editor.animation_rebase_state", event["event"])
        assertEquals(100L, event["oldTransaction"])
        assertEquals(101L, event["newTransaction"])
        assertEquals(5, event["deleteSlices"])
        assertEquals(0.12f, event["cursorRemaining"])
        assertEquals(0f, event["minSliceRemaining"])
        assertEquals(0.5f, event["maxSliceRemaining"])
        assertNotNull(event["ts"])
    }

    @Test
    fun animationRebaseStateRedactSensitiveFields() {
        DiagnosticsEvents.animationRebaseState(
            oldTransactionId = 1L,
            newTransactionId = 2L,
            deleteSlices = 0,
            cursorRemaining = 0.1f,
            minSliceRemaining = 0f,
            maxSliceRemaining = 0.1f,
        )
        val event = EditorEventRingBuffer.getSnapshot()[0]
        assertNull(event["text"])
        assertNull(event["content"])
        assertNull(event["body"])
    }
}

/**
 * Issue #638：真实 DiagnosticsLogger → PersistentLogWriter 持久链路最小测试。
 *
 * 验证 viewportRetarget 事件经 DiagnosticsEvents.record → DiagnosticsLogger.i
 * → PersistentLogWriter.enqueue 完整链路落盘，且日志文件不含正文内容。
 * 不记正文、glyph、preedit；只记录事件类型、transaction/起止 Y/最大 Y/原因。
 */
@org.junit.runner.RunWith(org.robolectric.RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [34])
class DiagnosticsPersistenceChainTest {
    private lateinit var context: android.content.Context

    @org.junit.Before
    fun setUp() {
        context = androidx.test.core.app.ApplicationProvider.getApplicationContext()
        com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer.setEnabled(true)
        com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer.clear()
        com.xiwei.sujian.core.diagnostics.DiagnosticsLogger.init(context, isEnabled = true, isVerbose = false)
        com.xiwei.sujian.core.diagnostics.PersistentLogWriter.flushBlocking()
        com.xiwei.sujian.core.diagnostics.PersistentLogWriter.clearLogs()
    }

    @org.junit.After
    fun tearDown() {
        com.xiwei.sujian.core.diagnostics.PersistentLogWriter.flushBlocking()
        com.xiwei.sujian.core.diagnostics.PersistentLogWriter.clearLogs()
        com.xiwei.sujian.core.diagnostics.PersistentLogWriter.setEnabled(false)
        com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer.setEnabled(false)
    }

    @Test
    fun viewportRetargetEventPersistsToLogFile() {
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.viewportRetarget(
            transactionId = 12345L,
            fromY = 100.5f,
            toY = 500.0f,
            maxY = 2000.0f,
            reason = "ensureRectVisible",
        )
        val flushed = com.xiwei.sujian.core.diagnostics.PersistentLogWriter.flushBlocking()
        org.junit.Assert.assertTrue("flushBlocking must return true", flushed)

        val logFiles = com.xiwei.sujian.core.diagnostics.PersistentLogWriter.getLogFiles()
        org.junit.Assert.assertTrue("must have at least one log file", logFiles.isNotEmpty())

        val content = logFiles[0].readText(Charsets.UTF_8)
        org.junit.Assert.assertTrue(
            "log file must contain viewport_retarget event",
            content.contains("editor.viewport_retarget"),
        )
        org.junit.Assert.assertTrue(
            "log file must contain transaction=12345",
            content.contains("transaction=12345"),
        )
        org.junit.Assert.assertTrue(
            "log file must contain fromY=100.5",
            content.contains("fromY=100.5"),
        )
        org.junit.Assert.assertFalse(
            "log file must NOT contain any content field",
            content.contains("content="),
        )
        org.junit.Assert.assertFalse(
            "log file must NOT contain any text field",
            content.contains("text="),
        )
        org.junit.Assert.assertFalse(
            "log file must NOT contain body field",
            content.contains("body="),
        )
    }

    @Test
    fun viewportRetargetWithNullTransactionIdPersistsWithoutTransactionField() {
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.viewportRetarget(
            transactionId = null,
            fromY = 0f,
            toY = 100f,
            maxY = 500f,
            reason = "scroll",
        )
        val flushed = com.xiwei.sujian.core.diagnostics.PersistentLogWriter.flushBlocking()
        org.junit.Assert.assertTrue(flushed)

        val logFiles = com.xiwei.sujian.core.diagnostics.PersistentLogWriter.getLogFiles()
        val content = logFiles[0].readText(Charsets.UTF_8)
        org.junit.Assert.assertTrue(content.contains("editor.viewport_retarget"))
        org.junit.Assert.assertTrue(content.contains("transaction=null"))
    }

    @Test
    fun animationRebaseStatePersistsRemainingFractionAsFloat() {
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.animationRebaseState(
            oldTransactionId = 100L,
            newTransactionId = 101L,
            deleteSlices = 3,
            cursorRemaining = 0.75f,
            minSliceRemaining = 0.25f,
            maxSliceRemaining = 0.75f,
        )
        val flushed = com.xiwei.sujian.core.diagnostics.PersistentLogWriter.flushBlocking()
        org.junit.Assert.assertTrue(flushed)

        val logFiles = com.xiwei.sujian.core.diagnostics.PersistentLogWriter.getLogFiles()
        val content = logFiles[0].readText(Charsets.UTF_8)
        org.junit.Assert.assertTrue(content.contains("editor.animation_rebase_state"))
        org.junit.Assert.assertTrue(content.contains("cursorRemaining=0.75"))
        org.junit.Assert.assertTrue(content.contains("minSliceRemaining=0.25"))
        org.junit.Assert.assertTrue(content.contains("maxSliceRemaining=0.75"))
        org.junit.Assert.assertFalse(content.contains("content="))
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
 * Issue #612 三、3.1 收口：LogcatSnapshotCollector 读取体积上限与 UTF-8 安全截断测试。
 *
 * 纯 JVM 测试，不需要 Robolectric：readBounded / truncateBytes 是纯字节操作。
 * 验证：读取即受限（不会把整个 logcat 缓冲区读进内存再截断）、截断不产生 U+FFFD
 * （截断处是完整 UTF-8 字符边界），与旧实现的“先全量读入再截断”形成正反对比。
 */
class LogcatSnapshotCollectorBoundedReadTest {
    @Test
    fun truncateBytesKeepsShortTextUnchanged() {
        val text = "short logcat content"
        val result = LogcatSnapshotCollector.truncateBytes(text.toByteArray(Charsets.UTF_8))
        assertEquals(text, String(result, Charsets.UTF_8))
    }

    @Test
    fun truncateBytesLimitsLongTextToMaxSnapshotBytes() {
        val text = "x".repeat(3 * 1024 * 1024) // 3 MiB > 2 MiB
        val truncated = LogcatSnapshotCollector.truncateBytes(text.toByteArray(Charsets.UTF_8))
        assertTrue(
            "truncated bytes should be <= 2 MiB, got ${truncated.size}",
            truncated.size <= LogcatSnapshotCollector.MAX_SNAPSHOT_BYTES,
        )
        assertTrue("truncated should be non-empty", truncated.isNotEmpty())
    }

    @Test
    fun truncateBytesHandlesMultibyteUtf8OnCharBoundary() {
        // 中文字符 3 bytes/char，800000 个中文 = 2.4 MiB > 2 MiB
        val text = "素".repeat(800000)
        val truncated = LogcatSnapshotCollector.truncateBytes(text.toByteArray(Charsets.UTF_8))
        assertTrue(
            "truncated bytes should be <= 2 MiB, got ${truncated.size}",
            truncated.size <= LogcatSnapshotCollector.MAX_SNAPSHOT_BYTES,
        )
        assertTrue("truncated should be non-empty", truncated.isNotEmpty())
        // 关键正反对比：旧字节截断会在多字节字符中间切断产生 U+FFFD；
        // 新按字符回退实现必须保证截断处是完整字符边界，不含 U+FFFD。
        val decoded = String(truncated, Charsets.UTF_8)
        assertTrue("truncated must not contain U+FFFD (char-boundary safe)", !decoded.contains('\uFFFD'))
    }

    @Test
    fun truncateBytesEmptyStaysEmpty() {
        assertTrue(LogcatSnapshotCollector.truncateBytes(ByteArray(0)).isEmpty())
    }

    @Test
    fun truncateBytesAsciiAtExactBoundaryUnchanged() {
        // 恰好 2 MiB 的 ASCII 文本不截断
        val text = "a".repeat(LogcatSnapshotCollector.MAX_SNAPSHOT_BYTES.toInt())
        val truncated = LogcatSnapshotCollector.truncateBytes(text.toByteArray(Charsets.UTF_8))
        assertEquals(text, String(truncated, Charsets.UTF_8))
    }

    @Test
    fun readBoundedSmallInputReadsAllAndNotCapped() {
        val text = "normal logcat line\nsecond line\n"
        val (bytes, capped) =
            LogcatSnapshotCollector.readBounded(ByteArrayInputStream(text.toByteArray(Charsets.UTF_8)))
        assertFalse("small input must not be capped", capped)
        assertEquals(text, String(bytes, Charsets.UTF_8))
    }

    @Test
    fun readBoundedEmptyStreamReturnsEmptyAndNotCapped() {
        val (bytes, capped) = LogcatSnapshotCollector.readBounded(ByteArrayInputStream(ByteArray(0)))
        assertFalse("empty stream must not be capped", capped)
        assertTrue(bytes.isEmpty())
    }

    @Test
    fun readBoundedHugeInputStopsAtCapAndFlagsCapped() {
        // 反（缺陷守护）：超过 2 MiB 的 logcat 输出不得全部读进内存——
        // 旧实现 readText() 读完整条输出后才截断，大缓冲区设备会撑爆导出内存。
        val huge = ByteArray(64 * 1024 * 1024) { 'x'.code.toByte() } // 64 MiB
        val (bytes, capped) =
            LogcatSnapshotCollector.readBounded(ByteArrayInputStream(huge))
        assertTrue("huge input must be capped", capped)
        assertTrue(
            "read bytes must stay near cap (<= max + chunk), got ${bytes.size}",
            bytes.size <= LogcatSnapshotCollector.MAX_SNAPSHOT_BYTES + LogcatSnapshotCollector.READ_CHUNK_BYTES,
        )
        assertTrue(
            "read bytes must cover at least the max bound, got ${bytes.size}",
            bytes.size >= LogcatSnapshotCollector.MAX_SNAPSHOT_BYTES,
        )
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

    private fun currentLogFile(): File {
        val identity = DiagnosticsBuildIdentity.fromBuildConfig()
        return File(AndroidDataRoot.logsDir(), "sujian-current-${identity.buildKey}.log")
    }

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
        // #623 评论 10：同一构建 4 轮转 + 1 当前 = 最多 5 个文件。旧断言 <=6
        // 编码的正是评论 10 指出的“总数变成 6”错误（prune 从 MAX_LOG_FILES=5
        // 开始删，保留 5 个 rotated + 1 当前 = 6）。新契约 pruneOldLogs 只保留
        // MAX_ROTATED_FILES_PER_BUILD=4 个轮转 + 1 当前 = 5。
        assertTrue(
            "should keep at most 4 rotated + 1 current = 5 files, got ${files.size}",
            files.size <= 5,
        )
        // 当前文件应存在
        assertTrue(
            "current log sujian-current.log should exist",
            files.any { it.name == currentLogFile().name },
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
            files.any { it.name == currentLogFile().name },
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

    @Test
    fun flushBlockingReturnsFalseWhenWriteFails() {
        // 反（评论 3.1 新契约）：文件系统不可写时 writeBatch 返回 false，
        // persistenceHealthy 变 false，flushBlocking 返回 false 而非假装成功。
        // 把 logsDir 路径占为普通文件（目录状态异常）使 writeBatch 无法创建文件。
        PersistentLogWriter.flushBlocking()
        val logsDir = AndroidDataRoot.logsDir()
        if (logsDir.exists()) logsDir.deleteRecursively()
        val parent = logsDir.parentFile
        parent.mkdirs()
        logsDir.writeText("occupied by a file")
        try {
            PersistentLogWriter.enqueue(req("should-fail-to-persist"))
            val ok = PersistentLogWriter.flushBlocking()
            assertFalse("flushBlocking must return false when write fails", ok)
        } finally {
            logsDir.delete()
        }
        // 恢复后 clearLogs 重置 persistenceHealthy，后续 flushBlocking 恢复 true
        PersistentLogWriter.clearLogs()
        PersistentLogWriter.enqueue(req("after-recover"))
        assertTrue("flushBlocking must recover after clearLogs", PersistentLogWriter.flushBlocking())
    }
}

/**
 * #623 评论 9：轮转/裁剪语义回归测试。
 *
 * - prune 下标修正：6 个日志文件裁剪后精确剩 5 个（旧实现从 MAX_LOG_FILES-1 开始删，
 *   实际只保留 4 个）；
 * - 裁剪删除失败返回 false，不得把“保留数量正常”伪装成成功；
 * - 轮转失败（Files.move 抛 IOException）必须进入与写盘失败相同的错误语义：
 *   writeBatch=false → persistenceHealthy=false → flushBlocking=false，
 *   导出不得把日志系统不完整伪装成完整落盘。
 */
@org.junit.runner.RunWith(org.robolectric.RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [34])
class PersistentLogWriterRotationPruneTest {
    @Before
    fun setUp() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        PersistentLogWriter.init(context)
        PersistentLogWriter.setEnabled(true)
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

    private fun currentLogFile(): File {
        val identity = DiagnosticsBuildIdentity.fromBuildConfig()
        return File(AndroidDataRoot.logsDir(), "sujian-current-${identity.buildKey}.log")
    }

    @Test
    fun pruneOldLogsKeepsExactlyFourRotatedPerBuild() {
        // #623 评论 10：同一 buildKey 最多 4 个轮转 + 1 当前 = 5 总数。
        // 创建 6 个 rotated + 1 当前，裁剪后保留 4 个最新 rotated（probe-2..probe-5），
        // 删除 2 个最旧 rotated（probe-0, probe-1）；当前文件不参与裁剪必须保留。
        val logsDir = AndroidDataRoot.logsDir()
        logsDir.mkdirs()
        PersistentLogWriter.getLogFiles().forEach { it.delete() }
        val rotated =
            (0 until 6).map { i ->
                val f = File(logsDir, "sujian-current-probe-$i.log")
                f.writeText("probe-$i")
                f.setLastModified(1_000_000_000L + i * 60_000L)
                f
            }
        val current = File(logsDir, "sujian-current-probe.log")
        current.writeText("current")
        current.setLastModified(2_000_000_000L)

        assertTrue(
            "prune must report success when all deletes succeed",
            PersistentLogWriter.pruneOldLogs(logsDir, "probe"),
        )

        val remaining = PersistentLogWriter.getLogFiles().map { it.name }.toSet()
        // 4 rotated + 1 current = 5
        assertEquals("must keep exactly 4 rotated + 1 current = 5 files, got ${remaining.size}", 5, remaining.size)
        for (i in 2 until 6) {
            assertTrue("newest rotated ${rotated[i].name} must be kept", remaining.contains(rotated[i].name))
        }
        for (i in 0 until 2) {
            assertTrue("oldest rotated ${rotated[i].name} must be pruned", !remaining.contains(rotated[i].name))
        }
        assertTrue("current file must be kept (not参与 rotated 裁剪)", remaining.contains(current.name))
    }

    @Test
    fun pruneOldLogsReportsDeleteFailure() {
        // #623 评论 9/10：裁剪删除失败必须返回 false，不得把“保留数量正常”伪装成成功。
        // buildKey=probe：4 个 rotated（probe-0..3）+ 1 current + 1 blocker 目录（占用文件名）。
        // current 被排除，参与裁剪 5 项（4 rotated + blocker），保留前 4 个 rotated，
        // blocker 位于索引 4 裁剪位，delete() 非空目录返回 false → prune 返回 false。
        // 5 个 probe 文件（4 rotated + 1 current）必须全部保留。
        val logsDir = AndroidDataRoot.logsDir()
        logsDir.mkdirs()
        PersistentLogWriter.getLogFiles().forEach { it.delete() }
        val blocker = File(logsDir, "sujian-current-probe-blocker.log")
        blocker.mkdirs()
        File(blocker, "child").writeText("occupies the name")
        blocker.setLastModified(999_000_000L) // 最旧 → 位于裁剪位
        val rotated =
            (0 until 4).map { i ->
                val f = File(logsDir, "sujian-current-probe-$i.log")
                f.writeText("probe-$i")
                f.setLastModified(1_000_000_000L + i * 60_000L)
                f
            }
        val current = File(logsDir, "sujian-current-probe.log")
        current.writeText("current")
        current.setLastModified(2_000_000_000L)
        try {
            assertFalse(
                "delete failure must be reported as prune failure",
                PersistentLogWriter.pruneOldLogs(logsDir, "probe"),
            )
            assertTrue("all rotated files must remain when prune fails", rotated.all { it.exists() })
            assertTrue("current file must remain when prune fails", current.exists())
        } finally {
            blocker.deleteRecursively()
        }
    }

    @Test
    fun flushBlockingReturnsFalseWhenRotationFails() {
        // #623 评论 9：轮转失败必须进入与写盘失败相同的错误语义。旧实现
        // File.renameTo 失败只返回 false 被无视，writeBatch 继续 append 旧文件并
        // 返回 true，persistenceHealthy 仍认为日志正常落盘。新实现 Files.move
        // 失败抛 IOException → writeBatch=false → persistenceHealthy=false →
        // flushBlocking=false，导出据此不得把日志系统不完整伪装成完整落盘。
        PersistentLogWriter.flushBlocking()
        val logsDir = AndroidDataRoot.logsDir()
        logsDir.mkdirs()
        PersistentLogWriter.getLogFiles().forEach { it.delete() }
        // 预置超过 1 MiB 的当前日志文件 → 下一次 writeBatch 必然触发轮转。
        val currentFile = File(logsDir, currentLogFile().name)
        currentFile.writeText("A".repeat(1024 * 1024 + 100))
        // 目录只读 → 同目录 Files.move 抛 AccessDeniedException（非 root 宿主）。
        logsDir.setWritable(false)
        try {
            PersistentLogWriter.enqueue(req("must-not-be-claimed-persisted"))
            val ok = PersistentLogWriter.flushBlocking()
            assertFalse("rotation failure must surface through flushBlocking", ok)
            assertTrue("current file must still exist after failed rotation", currentFile.exists())
        } finally {
            logsDir.setWritable(true)
        }
    }

    @Test
    fun pruneOldLogsDoesNotDeleteOtherBuildFiles() {
        // #623 评论 10：裁剪必须按 buildKey 隔离，不允许 B 构建的轮转删除 A 构建的日志。
        // A buildKey=v1-aaa-noAi-debug：1 current + 5 rotated
        // B buildKey=v2-bbb-ai-debug：1 current + 6 rotated（超过 4，触发裁剪）
        // 裁剪 B 后：A 的所有文件必须原样保留（存在 + 内容不变 + 数量不变），
        // B current 保留，B rotated 保留恰好 4 个最新。
        val logsDir = AndroidDataRoot.logsDir()
        logsDir.mkdirs()
        PersistentLogWriter.getLogFiles().forEach { it.delete() }

        val buildKeyA = "v1-aaa-noAi-debug"
        val buildKeyB = "v2-bbb-ai-debug"
        val baseA = "sujian-current-$buildKeyA"
        val baseB = "sujian-current-$buildKeyB"

        // A：1 current + 5 rotated，内容唯一可辨。
        val aCurrent = File(logsDir, "$baseA.log")
        aCurrent.writeText("A-current-content")
        aCurrent.setLastModified(1_000_000_000L)
        val aRotated =
            (0 until 5).map { i ->
                val f = File(logsDir, "$baseA-$i.log")
                f.writeText("A-rotated-$i-content")
                f.setLastModified(1_000_000_000L + i * 60_000L)
                f
            }
        val aAll = listOf(aCurrent) + aRotated
        val aSnapshots = aAll.associateWith { it.readText() }

        // B：1 current + 6 rotated，mtime 整体比 A 新以确保裁剪位落在 B 内。
        val bCurrent = File(logsDir, "$baseB.log")
        bCurrent.writeText("B-current-content")
        bCurrent.setLastModified(2_000_000_000L)
        val bRotated =
            (0 until 6).map { i ->
                val f = File(logsDir, "$baseB-$i.log")
                f.writeText("B-rotated-$i-content")
                f.setLastModified(2_000_000_000L + i * 60_000L)
                f
            }

        assertTrue(
            "prune B must report success when all deletes succeed",
            PersistentLogWriter.pruneOldLogs(logsDir, buildKeyB),
        )

        // 1. A 的所有文件仍然存在，内容完全不变。
        for (f in aAll) {
            assertTrue("A file ${f.name} must still exist", f.exists())
            assertEquals("A file ${f.name} content must be unchanged", aSnapshots[f], f.readText())
        }
        // 2. A 的文件数量不变（1 current + 5 rotated = 6）。
        val aRemaining = logsDir.listFiles { _, name -> name.startsWith(baseA) && name.endsWith(".log") }!!.toList()
        assertEquals("A file count must be unchanged", 6, aRemaining.size)
        // 3. B 的 current 文件仍存在。
        assertTrue("B current file must still exist", bCurrent.exists())
        // 4. B 的 rotated 保留恰好 4 个最新的（bRotated[2..5]，删除 bRotated[0], bRotated[1]）。
        val bRotatedRemaining =
            logsDir.listFiles { _, name ->
                name.startsWith(baseB) && name.endsWith(".log") && name != "$baseB.log"
            }!!.toList()
        assertEquals("B must keep exactly 4 rotated files, got ${bRotatedRemaining.size}", 4, bRotatedRemaining.size)
        for (i in 2 until 6) {
            assertTrue("B newest rotated ${bRotated[i].name} must be kept", bRotated[i].exists())
        }
        for (i in 0 until 2) {
            assertTrue("B oldest rotated ${bRotated[i].name} must be pruned", !bRotated[i].exists())
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

/**
 * Issue #612 评论 3.4 收口：deleteLogFiles 删除语义正反测试（纯 JVM，临时目录）。
 *
 * - 只删除文件；子目录及其内容属于未知数据，绝不触碰（仓库安全边界）。
 * - 目录不存在视为无可删内容（true）；路径存在但不是目录返回 false（目录状态异常，
 *   调用方不得把“无法确认已清空”伪装成成功）。
 */
class PersistentLogWriterDeleteLogFilesTest {
    @Test
    fun deleteLogFilesDeletesFilesAndLeavesSubdirectoriesUntouched() {
        val dir = createTempDir()
        try {
            File(dir, "sujian-current.log").writeText("log1")
            File(dir, "sujian-current-1.log").writeText("log2")
            val subDir = File(dir, "stray-dir")
            subDir.mkdirs()
            val unknownFile = File(subDir, "unknown-user-data.txt")
            unknownFile.writeText("must survive")

            val ok = PersistentLogWriter.deleteLogFiles(dir)
            assertTrue("all log files must be deleted", ok)
            assertTrue("sujian-current.log must be gone", !File(dir, "sujian-current.log").exists())
            assertTrue("sujian-current-1.log must be gone", !File(dir, "sujian-current-1.log").exists())
            // 正：未知子目录及其内容必须原样保留，不得触碰。
            assertTrue("subdir must survive", subDir.exists())
            assertTrue("unknown user data must survive", unknownFile.exists())
            assertEquals("unknown data content intact", "must survive", unknownFile.readText())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun deleteLogFilesMissingDirReturnsTrue() {
        val dir = File(createTempDir(), "nested/not/exists")
        assertTrue("missing dir is vacuously clear", PersistentLogWriter.deleteLogFiles(dir))
    }

    @Test
    fun deleteLogFilesReturnsFalseWhenPathIsNotDirectory() {
        // 反：路径是普通文件（目录状态异常）时不得返回 true 假装已清空。
        val file = File.createTempFile("logs-as-file", ".tmp")
        try {
            assertFalse(
                "file at dir path must report clear failure",
                PersistentLogWriter.deleteLogFiles(file),
            )
        } finally {
            file.delete()
        }
    }

    @Test
    fun deleteLogFilesEmptyDirReturnsTrue() {
        val dir = createTempDir()
        try {
            assertTrue(PersistentLogWriter.deleteLogFiles(dir))
        } finally {
            dir.deleteRecursively()
        }
    }
}

/**
 * Issue #612 评论 3.4 收口：clearLogs 删除失败必须返回 false（不假装已清空）。
 * Robolectric 集成：把 logsDir 路径占为普通文件（目录状态异常）时，
 * ClearBarrier 删除阶段必须返回失败，调用方拿到 false。
 */
@org.junit.runner.RunWith(org.robolectric.RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [34])
class ClearLogsFailurePropagationTest {
    @Test
    fun clearLogsReturnsFalseWhenLogsDirIsNotDirectory() {
        PersistentLogWriter.flushBlocking()
        val logsDir = AndroidDataRoot.logsDir()
        // 把目录路径占为普通文件（不存在的父目录先建好；沙箱内残留目录先清掉）。
        if (logsDir.exists()) logsDir.deleteRecursively()
        val parent = logsDir.parentFile
        parent.mkdirs()
        logsDir.writeText("occupied by a file")
        try {
            val ok = PersistentLogWriter.clearLogs()
            assertFalse("clearLogs must report failure when deletion cannot complete", ok)
            assertEquals("blocking file must survive", "occupied by a file", logsDir.readText())
        } finally {
            logsDir.delete()
        }
        // 恢复后清空恢复正常。
        assertTrue(PersistentLogWriter.clearLogs())
    }
}

/**
 * Issue #612 三、3.2 收口：exit_traces 文件名冲突不覆盖。
 * 同一毫秒内同一 reason 的多条退出记录（多进程包）必须各自落盘，不得互相覆盖。
 */
class ProcessExitCollectorUniqueTraceFileTest {
    @Test
    fun firstTraceGetsCanonicalName() {
        val dir = createTempDir()
        try {
            val file = ProcessExitCollector.uniqueTraceFile(dir, "1720000000000-CRASH")
            assertEquals("1720000000000-CRASH.trace", file.name)
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun collidingTraceGetsIndexedSuffix() {
        // 反（缺陷守护）：同 ts+reason 的旧实现直接覆盖前者，丢失一条 trace。
        val dir = createTempDir()
        try {
            File(dir, "1720000000000-CRASH.trace").writeText("first process trace")
            val second = ProcessExitCollector.uniqueTraceFile(dir, "1720000000000-CRASH")
            assertEquals("1720000000000-CRASH-1.trace", second.name)
            second.writeText("second process trace")

            val third = ProcessExitCollector.uniqueTraceFile(dir, "1720000000000-CRASH")
            assertEquals("1720000000000-CRASH-2.trace", third.name)

            // 两条 trace 内容都保留，无覆盖。
            assertEquals("first process trace", File(dir, "1720000000000-CRASH.trace").readText())
            assertEquals("second process trace", File(dir, "1720000000000-CRASH-1.trace").readText())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun differentReasonsDoNotCollide() {
        val dir = createTempDir()
        try {
            File(dir, "1720000000000-ANR.trace").writeText("anr")
            val file = ProcessExitCollector.uniqueTraceFile(dir, "1720000000000-CRASH")
            assertEquals("1720000000000-CRASH.trace", file.name)
        } finally {
            dir.deleteRecursively()
        }
    }
}

/**
 * Issue #612 评论 5 收口真实正反测试。
 *
 * 5.1：PersistentLogWriter.writeBatch 改用 BufferedWriter（PrintWriter 会吞 IOException，
 *     使 flushBlocking 在写盘失败时仍返回 true）。反：源码不得再 import PrintWriter/FileWriter
 *     （回归守卫），且必须使用 FileOutputStream+bufferedWriter 真实抛 IOException。
 * 5.2：LogcatSnapshotCollector 单一 deadline + finally 回收。正：echo 成功路径写出脱敏内容；
 *     反：sleep 30 必须在 ~5s deadline 内返回占位文件而非等 30s；命令不存在时写占位。
 */
class Issue612Comment5DiagnosticsTest {
    private fun logcatDir(): File =
        File(System.getProperty("java.io.tmpdir"), "issue612-c5-logcat-${System.nanoTime()}").apply { mkdirs() }

    @After
    fun tearDown() {
        // collectCommand 不依赖 PersistentLogWriter 单例，无需清理。
    }

    @Test
    fun logcatCollectCommandEchoSuccessWritesRedactedContent() {
        // 正（5.2 成功路径）：echo 立即输出并退出，collectCommand 写出脱敏后的内容。
        val dir = logcatDir()
        try {
            LogcatSnapshotCollector.collectCommand(dir, listOf("echo", "hello-logcat-line"))
            val out = File(dir, "logcat.txt")
            assertTrue("logcat.txt should be written on success", out.exists())
            val content = out.readText()
            assertTrue(
                "success path should contain echo output, got: $content",
                content.contains("hello-logcat-line"),
            )
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun logcatCollectCommandSleepHangsReturnsWithinDeadline() {
        // 反（5.2 单一 deadline）：sleep 30 既不输出也不退出，reader 阻塞在 read()。
        // 旧实现 future.get(5s) + waitFor(5s) 最坏 ~10s，且异常路径不回收子进程；
        // 新实现单一 5s deadline → TimeoutException → finally destroyForcibly 回收。
        // 断言总耗时明显小于 30s（应在 ~5s），且写占位文件。
        val dir = logcatDir()
        val start = System.nanoTime()
        try {
            LogcatSnapshotCollector.collectCommand(dir, listOf("sleep", "30"))
            val elapsed = (System.nanoTime() - start) / 1_000_000_000.0
            assertTrue(
                "collectCommand must respect single ~5s deadline, elapsed=${elapsed}s",
                elapsed < 20.0,
            )
            val out = File(dir, "logcat.txt")
            assertTrue("placeholder should be written on timeout", out.exists())
            val content = out.readText()
            assertTrue(
                "placeholder should indicate failure, got: $content",
                content.startsWith("logcat capture failed"),
            )
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun logcatCollectCommandMissingBinaryWritesPlaceholder() {
        // 反（5.2 命令不存在）：ProcessBuilder.start() 抛 IOException → 外层 catch 写占位，
        // finally 处理 null process，不抛异常。
        val dir = logcatDir()
        try {
            LogcatSnapshotCollector.collectCommand(dir, listOf("this-binary-does-not-exist-612"))
            val out = File(dir, "logcat.txt")
            assertTrue("placeholder should be written when binary missing", out.exists())
            assertTrue(
                "placeholder should indicate failure",
                out.readText().startsWith("logcat capture failed"),
            )
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun persistentLogWriterSourceDoesNotImportPrintWriter() {
        // 反（5.1 回归守卫）：writeBatch 不得再用 PrintWriter/FileWriter——
        // PrintWriter 吞 IOException 会使 flushBlocking 在写盘失败时仍返回 true。
        val candidates =
            listOf(
                "src/main/kotlin/com/xiwei/sujian/core/diagnostics/PersistentLogWriter.kt",
                "app/src/main/kotlin/com/xiwei/sujian/core/diagnostics/PersistentLogWriter.kt",
                "apps/android/app/src/main/kotlin/com/xiwei/sujian/core/diagnostics/PersistentLogWriter.kt",
            )
        val sourceFile =
            candidates.map { File(it) }.firstOrNull { it.exists() }
                ?: error("PersistentLogWriter.kt not found from any candidate path")
        val source = sourceFile.readText()
        assertTrue(
            "PersistentLogWriter must not import PrintWriter (swallows IOException, Issue #612 评论 5.1)",
            !source.contains("import java.io.PrintWriter"),
        )
        assertTrue(
            "PersistentLogWriter must not import FileWriter (replaced by FileOutputStream+bufferedWriter)",
            !source.contains("import java.io.FileWriter"),
        )
        assertTrue(
            "PersistentLogWriter must use BufferedWriter/FileOutputStream to surface IOException",
            source.contains("FileOutputStream") && source.contains("bufferedWriter"),
        )
    }
}
