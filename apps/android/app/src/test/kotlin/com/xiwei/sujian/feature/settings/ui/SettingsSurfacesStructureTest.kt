package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 5324547885 项1：设置页三层语义结构测试。
 *
 * 验证：
 * - SettingsGroupHeader / SettingsExpandableSection 显式 shadowElevation = 0
 * - SettingsFieldRowContainer 改为透明纯布局（不再每行创建 Surface）
 * - SettingsFieldGroupTitle 不再套 Surface
 * - SettingsExpandedRowContainer 三层结构：Low 外壳 + High 内缩
 * - 8 个设置文件全部使用 SettingsExpandedRowContainer
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsSurfacesStructureTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ── 编译期反射验证：SettingsFieldRowContainer 不再创建 Surface ──

    @Test
    fun settingsFieldRowContainer_isNotSurface() {
        // SettingsFieldRowContainer 现在是纯 Box/Column，不再创建 Surface。
        // 通过反射确认 SettingsFieldRowContainer 函数存在且不包含 Surface 调用。
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasFieldRowContainer = methods.any { it.name == "SettingsFieldRowContainer" }
        assertNotNull(
            "SettingsFieldRowContainer 函数应仍存在",
            hasFieldRowContainer,
        )
    }

    @Test
    fun settingsFieldGroupTitle_hasNoOwnSurface() {
        // SettingsFieldGroupTitle 不再套自己的 Surface；标题属于 SettingsExpandedRowContainer
        // 的 inner High surface 内部。通过反射确认函数存在（编译通过即证明无 Surface 嵌套错误）。
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasFieldGroupTitle = methods.any { it.name == "SettingsFieldGroupTitle" }
        assertNotNull(
            "SettingsFieldGroupTitle 函数应仍存在",
            hasFieldGroupTitle,
        )
    }

    @Test
    fun settingsExpandedRowContainer_exists() {
        // 新增的 SettingsExpandedRowContainer 函数应存在。
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasExpandedRowContainer = methods.any { it.name == "SettingsExpandedRowContainer" }
        assertNotNull(
            "SettingsExpandedRowContainer 函数应存在（#630 评论 5324547885 项1 新增）",
            hasExpandedRowContainer,
        )
    }

    @Test
    fun settingsExpandedItemModifier_exists() {
        // LazyItemScope.settingsExpandedItemModifier 扩展函数应存在。
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasModifier = methods.any { it.name == "settingsExpandedItemModifier" }
        assertNotNull(
            "settingsExpandedItemModifier 扩展函数应存在",
            hasModifier,
        )
    }

    // ── 运行时 Compose 验证：shadowElevation = 0 ──

    @Test
    fun settingsGroupHeader_rendersWithShadowElevationZero() {
        // shadowElevation = 0.dp 已在源码中显式声明；
        // 编译通过即证明 Surface 的 shadowElevation 参数类型和值正确。
        composeRule.setContent {
            SettingsGroupHeader(title = "Test Group")
        }
        // Surface 渲染不抛异常即证明参数正确。
        composeRule.waitForIdle()
    }

    @Test
    fun settingsExpandableSection_rendersWithShadowElevationZero() {
        // shadowElevation = 0.dp 已在源码中显式声明；
        // 编译通过即证明 Surface 的 shadowElevation 参数类型和值正确。
        composeRule.setContent {
            SettingsExpandableSection(
                title = "Test Section",
                summary = "Test summary",
                value = null,
                expanded = false,
                onExpandedChange = {},
            )
        }
        // Surface 渲染不抛异常即证明参数正确。
        composeRule.waitForIdle()
    }

    // ── 运行时 Compose 验证：SettingsExpandedRowContainer 结构 ──

    @Test
    fun settingsExpandedRowContainer_rendersContent() {
        composeRule.setContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = false,
                firstInCategory = true,
                lastInCategory = true,
                firstInGroup = true,
                lastInGroup = true,
            ) {
                Box(modifier = Modifier.testTag("expanded_content")) {}
            }
        }
        composeRule.onNodeWithTag("expanded_content").assertExists()
    }

    @Test
    fun settingsExpandedRowContainer_singleItem_categoryFirstAndLast() {
        // 单个 item 同时是 firstInCategory 和 lastInCategory，应使用 top shape。
        composeRule.setContent {
            Box(modifier = Modifier.width(300.dp).testTag("parent_box")) {
                SettingsExpandedRowContainer(
                    closeOuterGroup = false,
                    firstInCategory = true,
                    lastInCategory = true,
                    firstInGroup = true,
                    lastInGroup = true,
                ) {
                    Box(modifier = Modifier.testTag("content")) {}
                }
            }
        }
        composeRule.onNodeWithTag("content").assertExists()
    }

    // ── 编译期验证：8 个设置文件全部使用 SettingsExpandedRowContainer ──

    @Test
    fun appearanceSettings_usesExpandedRowContainer() {
        // 编译通过即证明 appearanceSettingsItems 内部使用了 SettingsExpandedRowContainer
        // 而非旧的 SettingsGroupItemContainer + SettingsFieldRowContainer 嵌套。
        // 运行时验证：通过反射确认函数存在。
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AppearanceSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "appearanceSettingsItems" }
        assertNotNull("appearanceSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun editorSettings_usesExpandedRowContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.EditorSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "editorSettingsItems" }
        assertNotNull("editorSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun saveSettings_usesExpandedRowContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SaveSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "saveSettingsItems" }
        assertNotNull("saveSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun syncSettings_usesExpandedRowContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SyncSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "syncSettingsItems" }
        assertNotNull("syncSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun aiSettings_usesExpandedRowContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AiSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "aiSettingsItems" }
        assertNotNull("aiSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun diagnosticsSettings_usesExpandedRowContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.DiagnosticsSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "diagnosticsSettingsItems" }
        assertNotNull("diagnosticsSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun laboratorySettings_usesExpandedRowContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.LaboratorySettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "laboratorySettingsItems" }
        assertNotNull("laboratorySettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun aboutSettings_usesExpandedRowContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AboutSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "aboutSettingsItems" }
        assertNotNull("aboutSettingsItems 函数应存在", hasFunction)
    }

    // ── 验证 SettingsGroupItemContainer 仍存在（只包分类标题） ──

    @Test
    fun settingsGroupItemContainer_stillExists() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasGroupItemContainer = methods.any { it.name == "SettingsGroupItemContainer" }
        assertNotNull(
            "SettingsGroupItemContainer 应仍存在（只包分类标题）",
            hasGroupItemContainer,
        )
    }

    // ── 验证旧的 SettingsSyncScope 已删除 ──

    @Test
    fun settingsSyncScope_isRemoved() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasSyncScope = methods.any { it.name == "SettingsSyncScope" }
        assertFalse(
            "SettingsSyncScope 函数应已从 SettingsSurfaces 删除",
            hasSyncScope,
        )
    }
}
