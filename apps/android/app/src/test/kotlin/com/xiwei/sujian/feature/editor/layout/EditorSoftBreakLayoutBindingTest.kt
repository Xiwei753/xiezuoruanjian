@file:Suppress("StringLiteralDuplication", "TooManyFunctions") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.layout

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #717 评论 5743443030 修复1：[EditorSoftBreakLayoutBinding] 单元测试。
 *
 * 覆盖：精确内容匹配、同长度不同内容不误匹配、多次 record 取最新、
 * 环形缓冲区淘汰、空 displayText、未 record 时 find 返回 null、
 * U+200B 在 rawText 中不被误删。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class EditorSoftBreakLayoutBindingTest {
    @Test
    fun record_andFindForDisplayText_exactMatch() {
        val holder = EditorSoftBreakLayoutBinding()
        val projection = EditorSoftBreakProjection.fromRawText("abc")
        val displayText = "a\u200Bb\u200Bc"

        holder.record(rawText = "abc", projection = projection, displayText = displayText)

        val match = holder.findForDisplayText(displayText)
        assertNotNull(match)
        assertEquals("abc", match!!.rawText)
        assertEquals(projection, match.projection)
    }

    @Test
    fun findForDisplayText_sameLengthDifferentContent_returnsNull() {
        val holder = EditorSoftBreakLayoutBinding()
        val projection = EditorSoftBreakProjection.fromRawText("abcdef")

        holder.record(rawText = "abcdef", projection = projection, displayText = "abcdef")

        // "abcxef" 长度相同但内容不同，不应误匹配
        val match = holder.findForDisplayText("abcxef")
        assertNull(match)
    }

    @Test
    fun findForDisplayText_multipleRecords_returnsLatestMatch() {
        val holder = EditorSoftBreakLayoutBinding()
        val p1 = EditorSoftBreakProjection.identity()
        val p2 = EditorSoftBreakProjection.fromRawText("v2text")

        holder.record(rawText = "v1", projection = p1, displayText = "d1")
        holder.record(rawText = "v2", projection = p2, displayText = "d2")

        val matchD2 = holder.findForDisplayText("d2")
        assertNotNull(matchD2)
        assertEquals("v2", matchD2!!.rawText)
        assertEquals(p2, matchD2.projection)

        val matchD1 = holder.findForDisplayText("d1")
        assertNotNull(matchD1)
        assertEquals("v1", matchD1!!.rawText)
        assertEquals(p1, matchD1.projection)
    }

    @Test
    fun findForDisplayText_ringBufferEviction_oldestEvicted() {
        val holder = EditorSoftBreakLayoutBinding()
        val projection = EditorSoftBreakProjection.identity()

        // CAPACITY = 16，record 17 次，最早的（displayText="d0"）被淘汰
        for (i in 0..16) {
            holder.record(rawText = "raw$i", projection = projection, displayText = "d$i")
        }

        // 最早的第 0 条已被淘汰
        assertNull(holder.findForDisplayText("d0"))

        // 最近的第 17 条（index 16）仍可找到
        val matchLatest = holder.findForDisplayText("d16")
        assertNotNull(matchLatest)
        assertEquals("raw16", matchLatest!!.rawText)

        // 第 1 条（index 1）是缓冲区里最早的，仍可找到
        val matchOldestInBuffer = holder.findForDisplayText("d1")
        assertNotNull(matchOldestInBuffer)
        assertEquals("raw1", matchOldestInBuffer!!.rawText)
    }

    @Test
    fun findForDisplayText_emptyDisplayText_matchesEmptyRaw() {
        val holder = EditorSoftBreakLayoutBinding()
        val identity = EditorSoftBreakProjection.identity()

        holder.record(rawText = "", projection = identity, displayText = "")

        val match = holder.findForDisplayText("")
        assertNotNull(match)
        assertEquals("", match!!.rawText)
        assertEquals(identity, match.projection)
    }

    @Test
    fun findForDisplayText_noRecords_returnsNull() {
        val holder = EditorSoftBreakLayoutBinding()

        assertNull(holder.findForDisplayText("abc"))
    }

    @Test
    fun findForDisplayText_zeroWidthSpaceInRawText_preserved() {
        val holder = EditorSoftBreakLayoutBinding()
        // raw 本身含 U+200B，display 在其基础上再插入 U+200B
        val rawText = "a\u200Bb"
        val projection = EditorSoftBreakProjection.fromRawText(rawText)
        val displayText = "a\u200Bb\u200Bb"

        holder.record(rawText = rawText, projection = projection, displayText = displayText)

        val match = holder.findForDisplayText(displayText)
        assertNotNull(match)
        // rawText 保留了用户原文的 U+200B，没有被误删
        assertEquals("a\u200Bb", match!!.rawText)
        assertEquals(projection, match.projection)
    }
}
