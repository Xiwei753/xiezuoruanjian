package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 5306659312 问题 A+B：设置页同步标题唯一 + 搜索/页面几何 行为契约。
 *
 * - [settingsSyncScope_functionIsRemovedFromSettingsSurfaces]：SettingsSyncScope 函数已删除
 *   （编译期 + 运行时反射双保证）。
 * - [syncSettings_doesNotRenderSecondSyncTitle]：SyncSettings 展开后不渲染第二个"同步"分类标题
 *   （标题由 SettingsRoute 的 SettingsExpandableSection 唯一提供）。
 * - [settingsSearchEntry_fillsMaxWidth]：SettingsSearchEntry 的 Surface fillMaxWidth，
 *   在固定宽度父容器内节点宽度等于父宽度。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsUiGeometryTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun settingsSyncScope_functionIsRemovedFromSettingsSurfaces() {
        // 编译期保证：SyncSettings.kt 不再调用 SettingsSyncScope（编译通过即证明）。
        // 运行时反射保证：SettingsSurfaces 类不得再声明 SettingsSyncScope 方法。
        val fileClass = Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt")
        val methods = fileClass.declaredMethods
        val hasSyncScope = methods.any { it.name == "SettingsSyncScope" }
        assertFalse(
            "SettingsSyncScope 函数应已从 SettingsSurfaces 删除（#630 评论 5306659312 问题 A）",
            hasSyncScope,
        )
    }

    @Test
    fun syncSettings_doesNotRenderSecondSyncTitle() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val syncCategoryTitle = context.getString(R.string.pref_category_sync)
        val state = minimalSyncSectionState()

        composeRule.setContent {
            SyncSettings(state = state, onIntent = {})
        }

        // SyncSettings 展开后不应再渲染"同步"分类标题（pref_category_sync）—
        // 它已由 SettingsRoute 的 SettingsExpandableSection 唯一显示一次。
        composeRule.onNodeWithText(syncCategoryTitle).assertDoesNotExist()
    }

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

    private fun minimalSyncSectionState(): SyncSectionState =
        SyncSectionState(
            syncConfig = SyncConfig(),
            syncSecrets = SyncSecrets(),
            syncCapability = SyncCapabilityData(),
            syncProfileLoadState = SyncProfileLoadState.Loading,
            dryRunState = SyncCommandState.IDLE,
            testConnectionState = SyncCommandState.IDLE,
            performSyncState = SyncCommandState.IDLE,
            syncResult = null,
            secureStorageWarning = null,
        )
}
