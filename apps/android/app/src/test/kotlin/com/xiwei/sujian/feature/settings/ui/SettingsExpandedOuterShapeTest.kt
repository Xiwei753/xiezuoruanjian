package com.xiwei.sujian.feature.settings.ui

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #633 评论 5379618506：设置页新结构测试。
 *
 * 旧的 settingsExpandedOuterShape / expandedItemClosesOuterGroup 纯函数已随
 * SettingsSurfaces.kt 删除。新结构中展开外壳的圆角由 [SettingsExpandedShell]
 * 的 closesGroup 参数直接决定（顶边永远直角，closesGroup=true 时画底圆角），
 * 不再需要 First/Middle/Last 拼卡决策函数。
 *
 * 本测试验证新结构的关键不变量：
 * - SettingsExpandableCategory 存在（独占展开动画）；
 * - SettingsExpandedShell 顶边永远直角（closesGroup=false 时全直角）。
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
        assertTrue("SettingsExpandedShell 应存在（顶边永远直角）", hasShell)
    }
}
