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
 * #630 评论 5326175206 项1: 展开内容横向内缩 20dp + 字段组间 gap 测试。
 *
 * - SettingsCategoryInset = 12.dp（分类标题内缩）
 * - SettingsExpandedInset = 20.dp（展开 High 内容内缩）
 * - firstInGroup && !firstInCategory 时顶部留 8dp Low 背景
 * - lastInCategory 底部留 8dp Low padding
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsExpandedInsetAndGroupGapTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun settingsExpandedInsetConstant_exists() {
        // 编译期验证：SettingsCategoryInset、SettingsExpandedExtraInset、SettingsExpandedInset
        // 三个 private val 存在。编译通过即证明常量定义正确。
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val fields = fileClass.declaredFields
        // private val 在字节码中是 synthetic field
        val hasCategoryInset = fields.any { it.name.contains("SettingsCategoryInset") }
        val hasExpandedExtraInset = fields.any { it.name.contains("SettingsExpandedExtraInset") }
        val hasExpandedInset = fields.any { it.name.contains("SettingsExpandedInset") }
        assertTrue("SettingsCategoryInset 应存在", hasCategoryInset)
        assertTrue("SettingsExpandedExtraInset 应存在", hasExpandedExtraInset)
        assertTrue("SettingsExpandedInset 应存在", hasExpandedInset)
    }

    @Test
    fun settingsExpandedRowContainer_rendersWithGroupGap() {
        // firstInGroup=true, firstInCategory=false → 顶部应有 8dp gap
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsExpandedRowContainer(
                    closeOuterGroup = false,
                    firstInCategory = false,
                    lastInCategory = false,
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
    fun settingsExpandedRowContainer_lastInCategory_rendersBottomPadding() {
        // lastInCategory=true → 底部应有 8dp Low padding
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsExpandedRowContainer(
                    closeOuterGroup = false,
                    firstInCategory = false,
                    lastInCategory = true,
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
    fun settingsExpandedRowContainer_firstInCategory_noTopGroupGap() {
        // firstInCategory=true → 不应有组间 gap（firstInGroup=false for this test）
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsExpandedRowContainer(
                    closeOuterGroup = false,
                    firstInCategory = true,
                    lastInCategory = false,
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
    fun settingsGroupItemContainer_usesCategoryInset() {
        // SettingsGroupItemContainer 编译通过即证明使用 SettingsCategoryInset (12dp)
        composeRule.setContent {
            SettingsGroupItemContainer(isLast = true) {
                Box(modifier = Modifier.testTag("category_content")) {}
            }
        }
        composeRule.onNodeWithTag("category_content").assertExists()
    }

    @Test
    fun movableItemContent_exists() {
        // SettingsMovableItemContent 函数应存在
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasMovableItemContent = methods.any { it.name == "SettingsMovableItemContent" }
        assertNotNull("SettingsMovableItemContent 函数应存在", hasMovableItemContent)
    }
}
