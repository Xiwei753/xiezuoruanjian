package com.xiwei.sujian.feature.settings.ui

import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #618 三 复审：外部同步重载（reloadFromExternalSync）的草稿保护契约。
 *
 * 外部同步把设置拉回来后，SettingsViewModel 从 Core 重读设置。重载绝不能覆盖
 * 用户尚未保存的编辑：有 pending 编辑（revision 已前进、未 flush）时保留 UI 草稿，
 * 等保存队列把用户值写回 Core（用户编辑胜出，UI 与 Core 保持一致）；没有任何
 * pending 编辑时才应用外部同步值。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SettingsExternalReloadTest {
    private fun createVm(): SettingsViewModel {
        val context = org.robolectric.RuntimeEnvironment.getApplication()
        val repo = com.xiwei.sujian.feature.settings.data.SettingsRepository(context)
        val themeRepo = com.xiwei.sujian.app.theme.ThemeRepository(context)
        val syncRepo = com.xiwei.sujian.feature.sync.data.SyncRepository(context)
        val syncStatusRepo = com.xiwei.sujian.feature.sync.data.SyncStatusRepository(syncRepo)
        val coordinator = com.xiwei.sujian.feature.sync.data.SyncCoordinator(syncRepo, syncStatusRepo)
        return SettingsViewModel(repo, themeRepo, syncRepo, coordinator)
    }

    @Test
    fun `reloadFromExternalSync keeps unsaved drafts`() {
        val vm = createVm()
        // 用户编辑已入队但尚未保存到 Core（revision 前进，保存队列未 flush）。
        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorFontSize = 18f) })
        vm.handleIntent(SettingsIntent.UpdateFontSize(20f))
        assertEquals(1L, vm.localRevision)
        assertEquals(0L, vm.localPersistedRevision)
        // 外部同步完成触发重载：不得覆盖未保存草稿。
        kotlinx.coroutines.runBlocking { vm.reloadFromExternalSync() }
        assertEquals("草稿字段不能被外部重载覆盖", 18f, vm.uiState.value.settings.editorFontSize, 0.01f)
        assertEquals("草稿字号不能被外部重载覆盖", 20f, vm.uiState.value.fontSize, 0.01f)
        // pending 状态保持不变：flush 后用户值仍会写回 Core。
        assertEquals(1L, vm.localRevision)
        assertEquals(0L, vm.localPersistedRevision)
    }

    @Test
    fun `reloadFromExternalSync applies external values when nothing pending`() {
        val vm = createVm()
        // 无 pending 编辑：重载应用外部值（单测环境原生库未加载 → 读取如实返回默认）。
        kotlinx.coroutines.runBlocking { vm.reloadFromExternalSync() }
        assertEquals(16f, vm.uiState.value.fontSize, 0.01f)
        assertEquals(0L, vm.localRevision)
        assertEquals(0L, vm.localPersistedRevision)
    }
}
