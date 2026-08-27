package com.xiwei.sujian.feature.project.ui

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #640：窄屏 compact host body geometry 决策回归测试。
 *
 * Issue 640 核心要求：预热 ready 必须在最终 Editor chrome 的真实 bounds。
 * compact host 必须在 hidden 和 visible 两种状态使用完全相同的 measured body bounds：
 * 正文 placeable 的 y/height 为 root 去掉 top-bar 实际测量高度。
 *
 * body geometry 由纯函数 [resolveCompactEditorBodyGeometry] 统一表达，
 * 该函数不接收 presentationVisible — 因此 hidden 与 visible 的 body bounds 必然相同。
 * 生产 [CompactEditorMeasureLayout] 与本测共用同一函数。
 */
class EditorPresentationCompactBodyGeometryTest {
    @Test
    fun bodyGeometry_subtractsTopBarFromRoot() {
        val geometry = resolveCompactEditorBodyGeometry(rootHeightPx = 1000, topBarHeightPx = 80)
        assertEquals("body top 必须等于 top-bar 实际测量高度", 80, geometry.bodyTopPx)
        assertEquals("body height 必须为 root 去掉 top-bar 高度", 920, geometry.bodyHeightPx)
    }

    @Test
    fun bodyGeometry_zeroTopBar_bodyFillsRoot() {
        val geometry = resolveCompactEditorBodyGeometry(rootHeightPx = 1000, topBarHeightPx = 0)
        assertEquals(0, geometry.bodyTopPx)
        assertEquals("无 top-bar 时 body 占满 root", 1000, geometry.bodyHeightPx)
    }

    @Test
    fun bodyGeometry_topBarExceedsRoot_clampsBodyToZero() {
        val geometry = resolveCompactEditorBodyGeometry(rootHeightPx = 100, topBarHeightPx = 200)
        assertEquals("top-bar 超过 root 时 body top 钳到 root", 100, geometry.bodyTopPx)
        assertEquals("top-bar 超过 root 时 body height 钳到 0，不得为负", 0, geometry.bodyHeightPx)
    }

    @Test
    fun bodyGeometry_negativeRoot_clampsToZero() {
        val geometry = resolveCompactEditorBodyGeometry(rootHeightPx = -10, topBarHeightPx = -5)
        assertEquals(0, geometry.bodyTopPx)
        assertEquals(0, geometry.bodyHeightPx)
    }

    @Test
    fun bodyGeometry_isIndependentOfPresentationVisible() {
        // 函数不接收 presentationVisible — hidden 与 visible 共用同一公式，
        // 因此预热 ready 的 bounds 与最终 visible 的 bounds 完全一致，
        // visible 切换不会触发 onSizeChanged。
        val hidden = resolveCompactEditorBodyGeometry(rootHeightPx = 2400, topBarHeightPx = 168)
        val visible = resolveCompactEditorBodyGeometry(rootHeightPx = 2400, topBarHeightPx = 168)
        assertEquals(hidden, visible)
        assertEquals(168, visible.bodyTopPx)
        assertEquals(2232, visible.bodyHeightPx)
    }
}
