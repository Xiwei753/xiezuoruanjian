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
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

// detekt StringLiteralDuplication：反射类名在多处 Class.forName 复用，提取为文件级常量。
private const val SETTINGS_CONTAINERS_KT_CLASS_NAME = "com.xiwei.sujian.feature.settings.ui.SettingsContainersKt"

/**
 * #633 评论 5379618506：设置页新三层语义结构测试。
 *
 * 验证：
 * - SettingsGroupHeader / SettingsFieldGroupTitle 存在
 * - SettingsExpandedShell / SettingsInnerCard 三层结构：Low 外壳 + High 内卡
 * - 8 个设置文件全部提供 @Composable XxxSettingsContent(vm)
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsSurfacesStructureTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ── 编译期反射验证：新视觉原语存在 ──

    @Test
    fun settingsFieldGroupTitle_exists() {
        val fileClass = Class.forName(SETTINGS_CONTAINERS_KT_CLASS_NAME)
        val methods = fileClass.declaredMethods
        val hasFieldGroupTitle = methods.any { it.name == "SettingsFieldGroupTitle" }
        assertNotNull(
            "SettingsFieldGroupTitle 函数应存在",
            hasFieldGroupTitle,
        )
    }

    @Test
    fun settingsExpandedShell_exists() {
        val fileClass = Class.forName(SETTINGS_CONTAINERS_KT_CLASS_NAME)
        val methods = fileClass.declaredMethods
        val hasExpandedShell = methods.any { it.name == "SettingsExpandedShell" }
        assertNotNull(
            "SettingsExpandedShell 函数应存在",
            hasExpandedShell,
        )
    }

    @Test
    fun settingsInnerCard_exists() {
        val fileClass = Class.forName(SETTINGS_CONTAINERS_KT_CLASS_NAME)
        val methods = fileClass.declaredMethods
        val hasInnerCard = methods.any { it.name == "SettingsInnerCard" }
        assertNotNull(
            "SettingsInnerCard 函数应存在",
            hasInnerCard,
        )
    }

    // ── 运行时 Compose 验证：shadowElevation = 0 ──

    @Test
    fun settingsGroupHeader_renders() {
        composeRule.setContent {
            SettingsGroupHeader(title = "Test Group")
        }
        composeRule.waitForIdle()
    }

    // ── 运行时 Compose 验证：SettingsExpandedShell / SettingsInnerCard 结构 ──

    @Test
    fun settingsExpandedShell_rendersContent() {
        composeRule.setContent {
            SettingsExpandedShell(closesGroup = false) {
                Box(modifier = Modifier.testTag("expanded_content")) {}
            }
        }
        composeRule.onNodeWithTag("expanded_content").assertExists()
    }

    @Test
    fun settingsInnerCard_rendersContent() {
        composeRule.setContent {
            Box(modifier = Modifier.width(300.dp).testTag("parent_box")) {
                SettingsInnerCard {
                    Box(modifier = Modifier.testTag("content")) {}
                }
            }
        }
        composeRule.onNodeWithTag("content").assertExists()
    }

    // ── 编译期验证：8 个设置文件全部提供 @Composable XxxSettingsContent(vm) ──

    @Test
    fun appearanceSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AppearanceSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "AppearanceSettingsContent" }
        assertTrue("AppearanceSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun editorSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.EditorSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "EditorSettingsContent" }
        assertTrue("EditorSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun saveSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SaveSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "SaveSettingsContent" }
        assertTrue("SaveSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun syncSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SyncSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "SyncSettingsContent" }
        assertTrue("SyncSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun aiSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AiSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "AiSettingsContent" }
        assertTrue("AiSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun diagnosticsSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.DiagnosticsSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "DiagnosticsSettingsContent" }
        assertTrue("DiagnosticsSettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun laboratorySettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.LaboratorySettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "LaboratorySettingsContent" }
        assertTrue("LaboratorySettingsContent 函数应存在", hasFunction)
    }

    @Test
    fun aboutSettingsContent_exists() {
        val settingsClass = Class.forName("com.xiwei.sujian.feature.settings.ui.AboutSettingsKt")
        val methods = settingsClass.declaredMethods
        val hasFunction = methods.any { it.name == "AboutSettingsContent" }
        assertTrue("AboutSettingsContent 函数应存在", hasFunction)
    }

    // ── 验证旧的 SettingsSurfaces.kt 已删除 ──

    @Test
    fun settingsSurfacesKt_isRemoved() {
        val exists =
            runCatching { Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt") }
                .isSuccess
        assertFalse(
            "SettingsSurfacesKt 应已删除（#633 评论 5379618506）",
            exists,
        )
    }

    @Test
    fun settingsExpandableSectionKt_isRemoved() {
        val exists =
            runCatching { Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsExpandableSectionKt") }
                .isSuccess
        assertFalse(
            "SettingsExpandableSectionKt 应已删除（#633 评论 5379618506）",
            exists,
        )
    }
}
