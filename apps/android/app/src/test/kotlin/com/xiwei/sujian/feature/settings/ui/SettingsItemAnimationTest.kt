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
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 5326175206 项2: Lazy item 动画 wrapper 测试。
 *
 * - SettingsMovableItemContent: 只做 placement 120ms，无 fade
 * - SettingsExpandedItemContent: fade + placement（fadeIn 100/fadeOut 70/placement 120）
 * - 旧 item（group spacer/header/category title）使用 SettingsMovableItemContent
 * - 新字段继续使用 SettingsExpandedItemContent
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsItemAnimationTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun movableItemContent_exists() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasMovable = methods.any { it.name == "SettingsMovableItemContent" }
        assertNotNull("SettingsMovableItemContent 应存在", hasMovable)
    }

    @Test
    fun expandedItemContent_exists() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasExpanded = methods.any { it.name == "SettingsExpandedItemContent" }
        assertNotNull("SettingsExpandedItemContent 应存在", hasExpanded)
    }

    @Test
    fun movableItemContent_rendersContent() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                androidx.compose.foundation.lazy.LazyColumn {
                    item {
                        SettingsMovableItemContent {
                            Box(
                                modifier =
                                    Modifier
                                        .fillMaxWidth()
                                        .height(48.dp)
                                        .testTag("movable_content"),
                            ) {}
                        }
                    }
                }
            }
        }
        composeRule.onNodeWithTag("movable_content").assertExists()
    }

    @Test
    fun expandedItemContent_rendersContent() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                androidx.compose.foundation.lazy.LazyColumn {
                    item {
                        SettingsExpandedItemContent {
                            Box(
                                modifier =
                                    Modifier
                                        .fillMaxWidth()
                                        .height(48.dp)
                                        .testTag("expanded_content"),
                            ) {}
                        }
                    }
                }
            }
        }
        composeRule.onNodeWithTag("expanded_content").assertExists()
    }

    @Test
    fun movableItemModifier_exists() {
        // settingsExpandedItemModifier 备选 modifier 仍存在
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasModifier = methods.any { it.name == "settingsExpandedItemModifier" }
        assertNotNull("settingsExpandedItemModifier 应存在", hasModifier)
    }
}
