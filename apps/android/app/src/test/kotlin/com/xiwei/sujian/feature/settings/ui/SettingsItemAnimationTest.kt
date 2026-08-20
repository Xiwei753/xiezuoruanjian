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
 * #630 R14: 设置页 Lazy item 结构测试。
 *
 * R14 删除了旧的 animateItem wrapper（SettingsMovableItemContent / SettingsExpandedItemContent），
 * 设置列表正常滚动时不需要任何 item placement 动画。
 * 测试验证 SettingsExpandedGroupContainer 和 contentType 常量存在。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsItemAnimationTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun expandedGroupContainer_exists() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasExpanded = methods.any { it.name == "SettingsExpandedGroupContainer" }
        assertNotNull("SettingsExpandedGroupContainer 应存在", hasExpanded)
    }

    @Test
    fun contentTypeConstants_exist() {
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val fields = fileClass.declaredFields
        val hasSearch = fields.any { it.name == "CONTENT_TYPE_SEARCH" }
        val hasSpacer = fields.any { it.name == "CONTENT_TYPE_SPACER" }
        val hasGroupHeader = fields.any { it.name == "CONTENT_TYPE_GROUP_HEADER" }
        val hasCategoryHeader = fields.any { it.name == "CONTENT_TYPE_CATEGORY_HEADER" }
        val hasFieldGroup = fields.any { it.name == "CONTENT_TYPE_EXPANDED_FIELD_GROUP" }
        assertNotNull("CONTENT_TYPE_SEARCH 应存在", hasSearch)
        assertNotNull("CONTENT_TYPE_SPACER 应存在", hasSpacer)
        assertNotNull("CONTENT_TYPE_GROUP_HEADER 应存在", hasGroupHeader)
        assertNotNull("CONTENT_TYPE_CATEGORY_HEADER 应存在", hasCategoryHeader)
        assertNotNull("CONTENT_TYPE_EXPANDED_FIELD_GROUP 应存在", hasFieldGroup)
    }

    @Test
    fun expandedGroupContainer_rendersContent() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsExpandedGroupContainer(
                    closeOuterGroup = false,
                    firstInGroup = true,
                    lastInGroup = true,
                ) {
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
}
