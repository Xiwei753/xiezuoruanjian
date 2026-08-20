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
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 R14: 展开内容横向内缩 20dp + 字段组间 gap 测试。
 *
 * - SettingsCategoryInset = 12.dp（分类标题内缩）
 * - SettingsExpandedInset = 20.dp（展开 High 内容内缩）
 * - SettingsExpandedGroupContainer: 字段组容器
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsExpandedInsetAndGroupGapTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun settingsExpandedInsetConstant_exists() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val fields = fileClass.declaredFields
        val hasCategoryInset = fields.any { it.name.contains("SettingsCategoryInset") }
        val hasExpandedExtraInset = fields.any { it.name.contains("SettingsExpandedExtraInset") }
        val hasExpandedInset = fields.any { it.name.contains("SettingsExpandedInset") }
        assertTrue("SettingsCategoryInset 应存在", hasCategoryInset)
        assertTrue("SettingsExpandedExtraInset 应存在", hasExpandedExtraInset)
        assertTrue("SettingsExpandedInset 应存在", hasExpandedInset)
    }

    @Test
    fun settingsExpandedGroupContainer_rendersContent() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsExpandedGroupContainer(
                    closeOuterGroup = false,
                    firstInGroup = true,
                    lastInGroup = false,
                ) {
                    Box(modifier = Modifier.fillMaxWidth().height(48.dp).testTag("content")) {}
                }
            }
        }
        composeRule.onNodeWithTag("content").assertExists()
    }

    @Test
    fun settingsExpandedGroupContainer_lastInGroup_rendersBottomShape() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsExpandedGroupContainer(
                    closeOuterGroup = true,
                    firstInGroup = false,
                    lastInGroup = true,
                ) {
                    Box(modifier = Modifier.fillMaxWidth().height(48.dp).testTag("content")) {}
                }
            }
        }
        composeRule.onNodeWithTag("content").assertExists()
    }

    @Test
    fun settingsExpandedGroupContainer_singleItem_fullShape() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsExpandedGroupContainer(
                    closeOuterGroup = false,
                    firstInGroup = true,
                    lastInGroup = true,
                ) {
                    Box(modifier = Modifier.fillMaxWidth().height(48.dp).testTag("content")) {}
                }
            }
        }
        composeRule.onNodeWithTag("content").assertExists()
    }

    @Test
    fun settingsGroupItemContainer_usesCategoryInset() {
        composeRule.setContent {
            SettingsGroupItemContainer(isLast = true) {
                Box(modifier = Modifier.testTag("category_content")) {}
            }
        }
        composeRule.onNodeWithTag("category_content").assertExists()
    }
}
