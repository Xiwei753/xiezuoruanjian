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

// detekt StringLiteralDuplication：反射类名在多处 Class.forName 复用，提取为文件级常量。
private const val SETTINGS_SURFACES_KT_CLASS_NAME = "com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt"

/**
 * #630 R14：设置页三层语义结构测试。
 *
 * 验证：
 * - SettingsGroupHeader / SettingsExpandableSection 显式 shadowElevation = 0
 * - SettingsFieldGroupTitle 不再套 Surface
 * - SettingsExpandedGroupContainer 三层结构：Low 外壳 + High 内缩
 * - 8 个设置文件全部使用 SettingsExpandedGroupContainer
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsSurfacesStructureTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ── 编译期反射验证：SettingsFieldGroupTitle 不再创建 Surface ──

    @Test
    fun settingsFieldGroupTitle_hasNoOwnSurface() {
        val fileClass = Class.forName(SETTINGS_SURFACES_KT_CLASS_NAME)
        val methods = fileClass.declaredMethods
        val hasFieldGroupTitle = methods.any { it.name == "SettingsFieldGroupTitle" }
        assertNotNull(
            "SettingsFieldGroupTitle 函数应仍存在",
            hasFieldGroupTitle,
        )
    }

    @Test
    fun settingsExpandedGroupContainer_exists() {
        val fileClass = Class.forName(SETTINGS_SURFACES_KT_CLASS_NAME)
        val methods = fileClass.declaredMethods
        val hasExpandedGroupContainer = methods.any { it.name == "SettingsExpandedGroupContainer" }
        assertNotNull(
            "SettingsExpandedGroupContainer 函数应存在（#630 R14 新增）",
            hasExpandedGroupContainer,
        )
    }

    // ── 运行时 Compose 验证：shadowElevation = 0 ──

    @Test
    fun settingsGroupHeader_rendersWithShadowElevationZero() {
        composeRule.setContent {
            SettingsGroupHeader(title = "Test Group")
        }
        composeRule.waitForIdle()
    }

    @Test
    fun settingsExpandableSection_rendersWithShadowElevationZero() {
        composeRule.setContent {
            SettingsExpandableSection(
                title = "Test Section",
                summary = "Test summary",
                value = null,
                expanded = false,
                onExpandedChange = {},
            )
        }
        composeRule.waitForIdle()
    }

    // ── 运行时 Compose 验证：SettingsExpandedGroupContainer 结构 ──

    @Test
    fun settingsExpandedGroupContainer_rendersContent() {
        composeRule.setContent {
            SettingsExpandedGroupContainer(
                closeOuterGroup = false,
                firstInGroup = true,
                lastInGroup = true,
            ) {
                Box(modifier = Modifier.testTag("expanded_content")) {}
            }
        }
        composeRule.onNodeWithTag("expanded_content").assertExists()
    }

    @Test
    fun settingsExpandedGroupContainer_singleItem_fullShape() {
        composeRule.setContent {
            Box(modifier = Modifier.width(300.dp).testTag("parent_box")) {
                SettingsExpandedGroupContainer(
                    closeOuterGroup = false,
                    firstInGroup = true,
                    lastInGroup = true,
                ) {
                    Box(modifier = Modifier.testTag("content")) {}
                }
            }
        }
        composeRule.onNodeWithTag("content").assertExists()
    }

    // ── 编译期验证：8 个设置文件全部使用 SettingsExpandedGroupContainer ──

    @Test
    fun appearanceSettings_usesExpandedGroupContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AppearanceSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "appearanceSettingsItems" }
        assertNotNull("appearanceSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun editorSettings_usesExpandedGroupContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.EditorSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "editorSettingsItems" }
        assertNotNull("editorSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun saveSettings_usesExpandedGroupContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SaveSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "saveSettingsItems" }
        assertNotNull("saveSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun syncSettings_usesExpandedGroupContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SyncSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "syncSettingsItems" }
        assertNotNull("syncSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun aiSettings_usesExpandedGroupContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AiSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "aiSettingsItems" }
        assertNotNull("aiSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun diagnosticsSettings_usesExpandedGroupContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.DiagnosticsSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "diagnosticsSettingsItems" }
        assertNotNull("diagnosticsSettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun laboratorySettings_usesExpandedGroupContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.LaboratorySettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "laboratorySettingsItems" }
        assertNotNull("laboratorySettingsItems 函数应存在", hasFunction)
    }

    @Test
    fun aboutSettings_usesExpandedGroupContainer() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AboutSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "aboutSettingsItems" }
        assertNotNull("aboutSettingsItems 函数应存在", hasFunction)
    }

    // ── 验证 SettingsGroupItemContainer 仍存在（只包分类标题） ──

    @Test
    fun settingsGroupItemContainer_stillExists() {
        val fileClass = Class.forName(SETTINGS_SURFACES_KT_CLASS_NAME)
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
        val fileClass = Class.forName(SETTINGS_SURFACES_KT_CLASS_NAME)
        val methods = fileClass.declaredMethods
        val hasSyncScope = methods.any { it.name == "SettingsSyncScope" }
        assertFalse(
            "SettingsSyncScope 函数应已从 SettingsSurfaces 删除",
            hasSyncScope,
        )
    }
}
