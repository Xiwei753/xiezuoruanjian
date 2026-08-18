package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.RectangleShape
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #630 评论 5327560790: 外层 Low 圆角按整个 SettingsGroup 收口 — 纯函数测试。
 *
 * 测试1: Writing 组 Editor→Save 外层 Low 不在中间断圆角 — Editor 展开后最后一个 item
 *        不画 BottomShape（因为 Save 还在后面），只有 Save 的最后展开 item 画 BottomShape。
 * 测试2: 最后行收口 — 组内最后一个 category 的最后一个展开 item 画 BottomShape。
 */
class SettingsExpandedOuterShapeTest {
    @Test
    fun settingsExpandedOuterShape_false_returnsRectangleShape() {
        val shape = settingsExpandedOuterShape(closeOuterGroup = false)
        assertEquals("非收口行应为 RectangleShape", RectangleShape, shape)
    }

    @Test
    fun settingsExpandedOuterShape_true_returnsRoundedCornerShape() {
        val shape = settingsExpandedOuterShape(closeOuterGroup = true)
        assertTrue("收口行应为 RoundedCornerShape", shape is RoundedCornerShape)
    }

    @Test
    fun settingsExpandedOuterShape_neverReturnsTopShape() {
        // 展开内容永远不画 SettingsGroupTopShape — 无论 closeOuterGroup 取何值，
        // 返回的 shape 都不应是顶部圆角（topStart/topEnd = 28dp 且 bottom 为直角）。
        val falseShape = settingsExpandedOuterShape(false)
        val trueShape = settingsExpandedOuterShape(true)
        // RectangleShape 不是 RoundedCornerShape；true 返回的是 BottomShape（bottom 圆角）
        assertFalse(falseShape is RoundedCornerShape)
        assertTrue(trueShape is RoundedCornerShape)
    }

    // ── 测试1: Writing 组 Editor→Save 外层 Low 不在中间断圆角 ──

    @Test
    fun writingGroup_editorLastItem_doesNotCloseOuterGroup() {
        // Writing 组顺序: Editor -> Save。Editor 不是最后一个 category（isLastCategory=false）。
        // Editor 的最后一个展开 item（isLastItemOfCategory=true）不应收口外层组。
        val editorIsLastCategory = false
        val editorLastItemIsLastInCategory = true
        val result = expandedItemClosesOuterGroup(editorIsLastCategory, editorLastItemIsLastInCategory)
        assertFalse("Editor 最后 item 不应画 BottomShape（Save 还在后面）", result)
    }

    @Test
    fun writingGroup_editorMiddleItem_doesNotCloseOuterGroup() {
        val editorIsLastCategory = false
        val editorMiddleItem = false
        val result = expandedItemClosesOuterGroup(editorIsLastCategory, editorMiddleItem)
        assertFalse("Editor 中间 item 不应画 BottomShape", result)
    }

    @Test
    fun writingGroup_saveNonLastItem_doesNotCloseOuterGroup() {
        // Save 是最后一个 category，但非最后 item 不应收口。
        val saveIsLastCategory = true
        val saveNonLastItem = false
        val result = expandedItemClosesOuterGroup(saveIsLastCategory, saveNonLastItem)
        assertFalse("Save 非最后 item 不应画 BottomShape", result)
    }

    // ── 测试2: 最后行收口 ──

    @Test
    fun writingGroup_saveLastItem_closesOuterGroup() {
        // Save 是最后一个 category（isLastCategory=true），
        // Save 的最后一个展开 item（isLastItemOfCategory=true）应收口外层组。
        val saveIsLastCategory = true
        val saveLastItemIsLastInCategory = true
        val result = expandedItemClosesOuterGroup(saveIsLastCategory, saveLastItemIsLastInCategory)
        assertTrue("Save 最后 item 应画 BottomShape（整个组收口）", result)
    }

    @Test
    fun expandedItemClosesOuterGroup_truthTable() {
        // 完整真值表
        assertFalse(expandedItemClosesOuterGroup(false, false))
        assertFalse(expandedItemClosesOuterGroup(false, true))
        assertFalse(expandedItemClosesOuterGroup(true, false))
        assertTrue(expandedItemClosesOuterGroup(true, true))
    }
}
