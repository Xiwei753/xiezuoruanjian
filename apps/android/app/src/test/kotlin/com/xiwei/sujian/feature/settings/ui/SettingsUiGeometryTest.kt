package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * 设置页搜索入口几何行为契约。
 *
 * - [settingsSearchEntry_fillsMaxWidth]：SettingsSearchEntry 的 Surface fillMaxWidth，
 *   在固定宽度父容器内节点宽度等于父宽度。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsUiGeometryTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun settingsSearchEntry_fillsMaxWidth() {
        composeRule.setContent {
            Box(modifier = Modifier.width(300.dp).testTag("parent_box")) {
                SettingsSearchEntry(onClick = {})
            }
        }

        val entryNode = composeRule.onNodeWithTag("settings_search_entry").fetchSemanticsNode()
        val parentNode = composeRule.onNodeWithTag("parent_box").fetchSemanticsNode()
        val entryWidth = entryNode.boundsInRoot.width
        val parentWidth = parentNode.boundsInRoot.width

        assertEquals(
            "SettingsSearchEntry Surface 应 fillMaxWidth — 节点宽度应等于父容器宽度",
            parentWidth,
            entryWidth,
            1f,
        )
    }

    @Test
    fun settingsSearchEntry_hasTestTag() {
        composeRule.setContent {
            SettingsSearchEntry(onClick = {})
        }
        // 验证 testTag 存在（fillMaxWidth 后 testTag 仍挂在 Surface 上）
        composeRule.onNodeWithTag("settings_search_entry").assertExists()
    }
}
