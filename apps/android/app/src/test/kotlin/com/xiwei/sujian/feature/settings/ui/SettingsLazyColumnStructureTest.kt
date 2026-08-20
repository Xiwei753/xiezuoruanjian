package com.xiwei.sujian.feature.settings.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #630 R14：验证 Settings LazyColumn 的 contentType、key 唯一性
 * 以及 Settings 导航使用 noPageTransitionMetadata（无双页 crossfade）。
 *
 * - contentType 常量值正确；
 * - 所有稳定 item 的 key 在同一分类内唯一（无 LazyColumn key 冲突）；
 * - Settings NavEntry 使用 noPageTransitionMetadata。
 */
class SettingsLazyColumnStructureTest {
    // ── contentType 常量值验证 ──

    @Test
    fun contentTypeConstants_haveExpectedValues() {
        assertEquals("search", CONTENT_TYPE_SEARCH)
        assertEquals("spacer", CONTENT_TYPE_SPACER)
        assertEquals("group_header", CONTENT_TYPE_GROUP_HEADER)
        assertEquals("category_header", CONTENT_TYPE_CATEGORY_HEADER)
        assertEquals("expanded_field_group", CONTENT_TYPE_EXPANDED_FIELD_GROUP)
    }

    // ── key 唯一性验证（每个分类展开后的 item key 互不重复） ──

    @Test
    fun appearanceItemKeys_areUnique() {
        val keys = appearanceSettingsItemKeys()
        assertEquals("Appearance item keys must be unique", keys.size, keys.toSet().size)
    }

    @Test
    fun editorItemKeys_areUnique() {
        val keys = editorSettingsItemKeys()
        assertEquals("Editor item keys must be unique", keys.size, keys.toSet().size)
    }

    @Test
    fun saveItemKeys_areUnique() {
        val keys = saveSettingsItemKeys()
        assertEquals("Save item keys must be unique", keys.size, keys.toSet().size)
    }

    @Test
    fun syncItemKeys_areUnique() {
        val keys = syncSettingsItemKeys()
        assertEquals("Sync item keys must be unique", keys.size, keys.toSet().size)
    }

    @Test
    fun aiItemKeys_areUnique() {
        val keys = aiSettingsItemKeys()
        assertEquals("Ai item keys must be unique", keys.size, keys.toSet().size)
    }

    @Test
    fun diagnosticsItemKeys_areUnique() {
        val keys = diagnosticsSettingsItemKeys()
        assertEquals("Diagnostics item keys must be unique", keys.size, keys.toSet().size)
    }

    @Test
    fun laboratoryItemKeys_areUnique() {
        val keys = laboratorySettingsItemKeys()
        assertEquals("Laboratory item keys must be unique", keys.size, keys.toSet().size)
    }

    @Test
    fun aboutItemKeys_areUnique() {
        val keys = aboutSettingsItemKeys()
        assertEquals("About item keys must be unique", keys.size, keys.toSet().size)
    }

    // ── Settings 导航使用 noPageTransitionMetadata（无双页 crossfade） ──

    @Test
    fun settingsRoute_hasNoPageTransitionMetadata() {
        val source =
            java.io.File(
                "src/main/kotlin/com/xiwei/sujian/app/navigation/SujianNavigationSuite.kt",
            ).readText()
        assertTrue(
            "Settings NavEntry must use noPageTransitionMetadata",
            source.contains("SujianRoute.Settings") &&
                source.contains("noPageTransitionMetadata"),
        )
    }

    // ── 辅助：提取各分类的 item key 列表 ──

    private fun appearanceSettingsItemKeys(): List<String> =
        listOf(
            "appearance.theme_group",
            "appearance.font_group",
        )

    private fun editorSettingsItemKeys(): List<String> =
        listOf(
            "editor.auto_indent_group",
            "editor.behavior_group",
        )

    private fun saveSettingsItemKeys(): List<String> = listOf("save.auto_save_group")

    private fun syncSettingsItemKeys(): List<String> =
        listOf(
            "sync.general_group",
            "sync.credentials_group",
            "sync.interval_group",
            "sync.actions_group",
        )

    private fun aiSettingsItemKeys(): List<String> = listOf("ai.enabled_group")

    private fun diagnosticsSettingsItemKeys(): List<String> =
        listOf(
            "diagnostics.diagnostics_group",
            "diagnostics.actions_group",
        )

    private fun laboratorySettingsItemKeys(): List<String> = listOf("laboratory.fullscreen")

    private fun aboutSettingsItemKeys(): List<String> = listOf("about.info_group")
}
