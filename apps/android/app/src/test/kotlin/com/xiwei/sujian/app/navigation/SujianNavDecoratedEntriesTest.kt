package com.xiwei.sujian.app.navigation

import androidx.compose.foundation.layout.Column
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.ui.NavDisplay
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #614 评论三回归：验证 decoratedEntries 为每个 top-level 绑定独立 decorator。
 *
 * - Works 内放一个 rememberSaveable 计数；Works → StarMap → Works 后计数保留；
 * - 真正从 Works 栈 pop 对应 entry 后重新进入才重新创建。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SujianNavDecoratedEntriesTest {
    private companion object {
        // 两个测试共用同一组占位内容文本，提取常量避免 StringLiteralDuplication。
        const val STAR_MAP_CONTENT = "starmap-content"
        const val STATS_CONTENT = "stats-content"
        const val SETTINGS_CONTENT = "settings-content"
        const val WORKS_CONTENT = "works-content"
        const val OPEN_SETTINGS = "open-settings"
    }

    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun switchTopLevel_preservesSaveableState_acrossTabSwitch() {
        lateinit var topLevelBackStack: SujianTopLevelBackStack

        composeRule.setContent {
            topLevelBackStack = rememberSujianTopLevelBackStack()
            val entryProvider: (NavKey) -> NavEntry<NavKey> = { key ->
                when (key) {
                    is SujianRoute.Works ->
                        NavEntry(key) {
                            var count by rememberSaveable { mutableIntStateOf(0) }
                            Column {
                                Text("works-count:$count")
                                Button(onClick = { count++ }) { Text("works-inc") }
                            }
                        }
                    is SujianRoute.StarMap -> NavEntry(key) { Text(STAR_MAP_CONTENT) }
                    is SujianRoute.Stats -> NavEntry(key) { Text(STATS_CONTENT) }
                    is SujianRoute.Settings -> NavEntry(key) { Text(SETTINGS_CONTENT) }
                    else -> NavEntry(key) {}
                }
            }
            NavDisplay(
                entries = topLevelBackStack.decoratedEntries(entryProvider),
                onBack = { topLevelBackStack.removeLastOrNull() },
            )
        }

        // 初始: works-count:0
        composeRule.onNodeWithText("works-count:0").assertExists()
        // 增加计数
        composeRule.onNodeWithText("works-inc").performClick()
        composeRule.onNodeWithText("works-count:1").assertExists()
        // 切到 StarMap
        composeRule.runOnIdle { topLevelBackStack.addTopLevel(SujianDestination.StarMap) }
        composeRule.onNodeWithText("starmap-content").assertExists()
        // 切回 Works — saveable 状态应保留
        composeRule.runOnIdle { topLevelBackStack.addTopLevel(SujianDestination.Works) }
        composeRule.onNodeWithText("works-count:1").assertExists()
    }

    @Test
    fun popEntry_recreatesSaveableState() {
        lateinit var topLevelBackStack: SujianTopLevelBackStack

        composeRule.setContent {
            topLevelBackStack = rememberSujianTopLevelBackStack()
            val entryProvider: (NavKey) -> NavEntry<NavKey> = { key ->
                when (key) {
                    is SujianRoute.Works ->
                        NavEntry(key) {
                            Column {
                                Text(WORKS_CONTENT)
                                Button(onClick = { topLevelBackStack.add(SujianRoute.Settings) }) {
                                    Text(OPEN_SETTINGS)
                                }
                            }
                        }
                    is SujianRoute.StarMap -> NavEntry(key) { Text(STAR_MAP_CONTENT) }
                    is SujianRoute.Stats -> NavEntry(key) { Text(STATS_CONTENT) }
                    is SujianRoute.Settings ->
                        NavEntry(key) {
                            var count by rememberSaveable { mutableIntStateOf(0) }
                            Column {
                                Text("settings-count:$count")
                                Button(onClick = { count++ }) { Text("settings-inc") }
                            }
                        }
                    else -> NavEntry(key) {}
                }
            }
            NavDisplay(
                entries = topLevelBackStack.decoratedEntries(entryProvider),
                onBack = { topLevelBackStack.removeLastOrNull() },
            )
        }

        // 初始: works-content
        composeRule.onNodeWithText(WORKS_CONTENT).assertExists()
        // 进入 Settings
        composeRule.onNodeWithText(OPEN_SETTINGS).performClick()
        composeRule.onNodeWithText("settings-count:0").assertExists()
        // 增加计数
        composeRule.onNodeWithText("settings-inc").performClick()
        composeRule.onNodeWithText("settings-count:1").assertExists()
        // pop Settings 回到 Works
        composeRule.runOnIdle { topLevelBackStack.removeLastOrNull() }
        composeRule.onNodeWithText(WORKS_CONTENT).assertExists()
        // 重新进入 Settings — saveable 状态应重置
        composeRule.onNodeWithText(OPEN_SETTINGS).performClick()
        composeRule.onNodeWithText("settings-count:0").assertExists()
    }
}
