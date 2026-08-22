package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
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
 * #633 评论 5379618506：SettingsSurfaces.kt 已删除，反射验证改为确认类不存在。
 *
 * - [settingsSurfacesKt_isRemoved]：SettingsSurfacesKt 已删除。
 * - [settingsSearchEntry_fillsMaxWidth]：SettingsSearchEntry 的 Surface fillMaxWidth，
 *   在固定宽度父容器内节点宽度等于父宽度。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsUiGeometryTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun settingsSurfacesKt_isRemoved() {
        // #633 评论 5379618506：SettingsSurfaces.kt 已删除，
        // 旧的 SettingsSyncScope / SettingsExpandedGroupContainer 等符号全部退出。
        val exists =
            runCatching { Class.forName("com.xiwei.sujian.feature.settings.ui.SettingsSurfacesKt") }
                .isSuccess
        assertFalse(
            "SettingsSurfacesKt 应已删除（#633 评论 5379618506）",
            exists,
        )
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

    @Suppress("unused")
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
