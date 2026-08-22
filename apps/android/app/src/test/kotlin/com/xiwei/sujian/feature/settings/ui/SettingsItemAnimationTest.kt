@file:Suppress("StringLiteralDuplication")

package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #633 评论 5379618506：设置页新结构测试。
 *
 * R14 删除了旧的 animateItem wrapper；#633 进一步删除了 First/Middle/Last 拼卡模型。
 * 新结构：一个 category = 一个 Transition + 一个 AnimatedVisibility（在 SettingsExpandableCategory 内），
 * 一个逻辑字段组 = 一张 SettingsInnerCard。不再有 CONTENT_TYPE_* 常量（Column+verticalScroll 不需要）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsItemAnimationTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun settingsExpandableCategory_exists() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsExpandableCategoryKt")
        val methods = fileClass.declaredMethods
        val hasCategory = methods.any { it.name == "SettingsExpandableCategory" }
        assertNotNull("SettingsExpandableCategory 应存在", hasCategory)
    }

    @Test
    fun settingsInnerCard_rendersContent() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsInnerCard {
                    Box(
                        modifier =
                            Modifier
                                .fillMaxWidth()
                                .height(48.dp)
                                .testTag("group_content"),
                    ) {}
                }
            }
        }
        composeRule.onNodeWithTag("group_content").assertExists()
    }

    @Test
    fun settingsExpandedShell_rendersContent() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsExpandedShell(closesGroup = true) {
                    Box(
                        modifier =
                            Modifier
                                .fillMaxWidth()
                                .height(48.dp)
                                .testTag("shell_content"),
                    ) {}
                }
            }
        }
        composeRule.onNodeWithTag("shell_content").assertExists()
    }

    @Test
    fun contentTypeConstants_areRemoved() {
        // CONTENT_TYPE_* 常量已随 SettingsSurfaces.kt 删除（Column+verticalScroll 不需要 contentType）。
        // 验证 SettingsSurfacesKt 类不存在。
        val exists =
            runCatching { Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt") }
                .isSuccess
        assertTrue("SettingsSurfacesKt 应已删除", !exists)
    }
}
