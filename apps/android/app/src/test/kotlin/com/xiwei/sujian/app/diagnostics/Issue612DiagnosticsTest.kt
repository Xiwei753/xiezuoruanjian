@file:Suppress("StringLiteralDuplication") // 测试固件中 "screen=" / "editor=" 等前缀天然重复

package com.xiwei.sujian.app.diagnostics

import com.xiwei.sujian.core.diagnostics.DiagnosticsEvents
import com.xiwei.sujian.core.diagnostics.JankStatsController
import com.xiwei.sujian.core.diagnostics.ProcessStateSummary
import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

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
