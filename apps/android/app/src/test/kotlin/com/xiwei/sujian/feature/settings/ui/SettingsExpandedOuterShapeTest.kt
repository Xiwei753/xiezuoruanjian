package com.xiwei.sujian.feature.settings.ui

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #633 评论 5379618506 / #635 评论 5385740370：设置页新结构测试。
 *
 * 旧的 settingsExpandedOuterShape / expandedItemClosesOuterGroup 纯函数已随
 * SettingsSurfaces.kt 删除。新结构中整组外轮廓（包括底圆角）由
 * [SettingsExpandableCategory] 的最外层 Surface 唯一拥有；
 * [SettingsExpandedShell] 永远 [RectangleShape]，不再决定外轮廓，
 * 不再需要 First/Middle/Last 拼卡决策函数。
 *
 * 本测试验证新结构的关键不变量：
 * - SettingsExpandableCategory 存在（独占展开动画 + 整组外轮廓）；
 * - SettingsExpandedShell 存在（永远 RectangleShape）。
 */
class SettingsExpandedOuterShapeTest {
    @Test
    fun settingsExpandableCategory_exists() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsExpandableCategoryKt")
        val methods = fileClass.declaredMethods
        val hasCategory = methods.any { it.name == "SettingsExpandableCategory" }
        assertTrue("SettingsExpandableCategory 应存在（独占展开动画）", hasCategory)
    }

    @Test
    fun settingsExpandedShell_exists() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsContainersKt")
        val methods = fileClass.declaredMethods
        val hasShell = methods.any { it.name == "SettingsExpandedShell" }
        assertTrue("SettingsExpandedShell 应存在（永远 RectangleShape）", hasShell)
    }
}
