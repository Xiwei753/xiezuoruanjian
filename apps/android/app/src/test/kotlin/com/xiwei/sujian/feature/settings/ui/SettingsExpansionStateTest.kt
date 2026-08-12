package com.xiwei.sujian.feature.settings.ui

import com.xiwei.sujian.app.navigation.SettingsSection
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #618 四：SettingsExpansionState 每个分类独立展开状态 + Saver 跨配置保留。
 *
 * - 切换一个分类不触碰其它分类的状态；
 * - Saver 保存/恢复展开的 section name 列表；
 * - 未知 section name 在恢复时被忽略（与旧 listSaver 语义一致）。
 */
class SettingsExpansionStateTest {
    @Test
    fun sections_haveIndependentState() {
        val state = SettingsExpansionState()
        state.setExpanded(SettingsSection.Appearance, true)
        assertTrue(state.isExpanded(SettingsSection.Appearance))
        assertFalse(state.isExpanded(SettingsSection.Editor))
        state.setExpanded(SettingsSection.Editor, true)
        state.setExpanded(SettingsSection.Appearance, false)
        assertFalse(state.isExpanded(SettingsSection.Appearance))
        assertTrue(state.isExpanded(SettingsSection.Editor))
    }

    @Test
    fun saver_roundTripsExpandedSections() {
        val state = SettingsExpansionState()
        state.setExpanded(SettingsSection.Appearance, true)
        state.setExpanded(SettingsSection.Editor, true)
        state.setExpanded(SettingsSection.Sync, false)

        val saved = state.expandedSectionNames()
        // mutableStateMapOf 迭代顺序无保证（与旧 Set 语义一致），保存内容按名字集合断言。
        assertEquals(setOf("Appearance", "Editor"), saved.toSet())

        val restored = requireNotNull(SettingsExpansionState.Saver.restore(saved))
        assertTrue(restored.isExpanded(SettingsSection.Appearance))
        assertTrue(restored.isExpanded(SettingsSection.Editor))
        assertFalse(restored.isExpanded(SettingsSection.Sync))
    }

    @Test
    fun saver_restore_ignoresUnknownNames() {
        val restored = requireNotNull(SettingsExpansionState.Saver.restore(listOf("Appearance", "NotASection")))
        assertTrue(restored.isExpanded(SettingsSection.Appearance))
        assertFalse(restored.isExpanded(SettingsSection.About))
    }
}
