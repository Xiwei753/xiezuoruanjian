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
 * #633 评论 5379618506：设置页新视觉原语结构测试。
 *
 * 验证 [SettingsExpandedShell] / [SettingsInnerCard] 渲染内容，
 * 替代旧的 SettingsExpandedGroupContainer / SettingsGroupItemContainer 测试。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsExpandedInsetAndGroupGapTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun settingsExpandedShell_rendersContent() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsExpandedShell {
                    Box(modifier = Modifier.fillMaxWidth().height(48.dp).testTag("content")) {}
                }
            }
        }
        composeRule.onNodeWithTag("content").assertExists()
    }

    @Test
    fun settingsInnerCard_rendersContent() {
        composeRule.setContent {
            Box(modifier = Modifier.testTag("root")) {
                SettingsInnerCard {
                    Box(modifier = Modifier.fillMaxWidth().height(48.dp).testTag("content")) {}
                }
            }
        }
        composeRule.onNodeWithTag("content").assertExists()
    }

    @Test
    fun settingsContainersKt_exists() {
        // 验证新的 SettingsContainersKt 文件已加载
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsContainersKt")
        val methods = fileClass.declaredMethods
        val hasShell = methods.any { it.name == "SettingsExpandedShell" }
        val hasInnerCard = methods.any { it.name == "SettingsInnerCard" }
        val hasHeader = methods.any { it.name == "SettingsGroupHeader" }
        assertTrue("SettingsExpandedShell 应存在", hasShell)
        assertTrue("SettingsInnerCard 应存在", hasInnerCard)
        assertTrue("SettingsGroupHeader 应存在", hasHeader)
    }
}
