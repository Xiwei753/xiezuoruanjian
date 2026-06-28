package com.xiwei.sujian.data

import android.util.Log
import com.xiwei.sujian.model.ActionPlacement
import com.xiwei.sujian.model.ActionRole
import com.xiwei.sujian.model.ActionSlot
import com.xiwei.sujian.model.ScreenPolicy
import com.xiwei.sujian.model.ScreenRole
import com.xiwei.sujian.model.ShellMode

/**
 * Screen Policy Bridge — 通过 Core resolve_screen_policy 获取跨端统一动作位置语义。
 *
 * Android 端传入 ScreenRole + ShellMode，调用 Core 获取 ActionSlot 列表。
 * 不允许 Android 端自行决定动作位置，所有位置必须来自 Core 的 ActionPlacement。
 */
class ScreenPolicyBridge(private val appServiceBridge: AppServiceBridge) {

    companion object {
        private const val TAG = "ScreenPolicyBridge"
    }

    fun resolveScreenPolicy(screenRole: ScreenRole, shellMode: ShellMode): ScreenPolicy? {
        return try {
            val result = appServiceBridge.resolveScreenPolicy(
                screenRole.toDto(),
                shellMode.toDto()
            )
            when (result) {
                is BridgeResult.Success -> result.data.toModel()
                else -> null
            }
        } catch (e: Exception) {
            Log.e(TAG, "resolveScreenPolicy failed: ${e.message}", e)
            null
        }
    }

    // ── DTO conversion helpers ──

    private fun ScreenRole.toDto(): uniffi.writer_core.ScreenRoleDto = when (this) {
        ScreenRole.Home -> uniffi.writer_core.ScreenRoleDto.HOME
        ScreenRole.Workspace -> uniffi.writer_core.ScreenRoleDto.WORKSPACE
        ScreenRole.Writing -> uniffi.writer_core.ScreenRoleDto.WRITING
        ScreenRole.Settings -> uniffi.writer_core.ScreenRoleDto.SETTINGS
        ScreenRole.Sync -> uniffi.writer_core.ScreenRoleDto.SYNC
    }

    private fun ShellMode.toDto(): uniffi.writer_core.ShellModeDto = when (this) {
        ShellMode.SinglePane -> uniffi.writer_core.ShellModeDto.SINGLE_PANE
        ShellMode.SupportingPane -> uniffi.writer_core.ShellModeDto.SUPPORTING_PANE
        ShellMode.TwoPane -> uniffi.writer_core.ShellModeDto.TWO_PANE
    }

    private fun uniffi.writer_core.ScreenRoleDto.toModel(): ScreenRole = when (this) {
        uniffi.writer_core.ScreenRoleDto.HOME -> ScreenRole.Home
        uniffi.writer_core.ScreenRoleDto.WORKSPACE -> ScreenRole.Workspace
        uniffi.writer_core.ScreenRoleDto.WRITING -> ScreenRole.Writing
        uniffi.writer_core.ScreenRoleDto.SETTINGS -> ScreenRole.Settings
        uniffi.writer_core.ScreenRoleDto.SYNC -> ScreenRole.Sync
    }

    private fun uniffi.writer_core.ActionRoleDto.toModel(): ActionRole = when (this) {
        uniffi.writer_core.ActionRoleDto.BACK -> ActionRole.Back
        uniffi.writer_core.ActionRoleDto.SAVE -> ActionRole.Save
        uniffi.writer_core.ActionRoleDto.CREATE_PROJECT -> ActionRole.CreateProject
        uniffi.writer_core.ActionRoleDto.CREATE_VOLUME -> ActionRole.CreateVolume
        uniffi.writer_core.ActionRoleDto.CREATE_CHAPTER -> ActionRole.CreateChapter
        uniffi.writer_core.ActionRoleDto.DELETE -> ActionRole.Delete
        uniffi.writer_core.ActionRoleDto.RENAME -> ActionRole.Rename
        uniffi.writer_core.ActionRoleDto.SETTINGS -> ActionRole.Settings
        uniffi.writer_core.ActionRoleDto.SYNC -> ActionRole.Sync
        uniffi.writer_core.ActionRoleDto.SEARCH -> ActionRole.Search
    }

    private fun uniffi.writer_core.ActionPlacementDto.toModel(): ActionPlacement = when (this) {
        uniffi.writer_core.ActionPlacementDto.TOP_LEADING -> ActionPlacement.TopLeading
        uniffi.writer_core.ActionPlacementDto.TOP_TRAILING -> ActionPlacement.TopTrailing
        uniffi.writer_core.ActionPlacementDto.FLOATING -> ActionPlacement.Floating
        uniffi.writer_core.ActionPlacementDto.BOTTOM_BAR -> ActionPlacement.BottomBar
        uniffi.writer_core.ActionPlacementDto.CONTEXT_MENU -> ActionPlacement.ContextMenu
        uniffi.writer_core.ActionPlacementDto.SIDE_PANEL -> ActionPlacement.SidePanel
    }

    private fun uniffi.writer_core.ScreenPolicyDto.toModel(): ScreenPolicy {
        return ScreenPolicy(
            screenRole = screenRole.toModel(),
            actionSlots = actionSlots.map { it.toModel() }
        )
    }

    private fun uniffi.writer_core.ActionSlotDto.toModel(): ActionSlot {
        return ActionSlot(
            actionId = actionId,
            role = role.toModel(),
            placement = placement.toModel(),
            visibleIn = visibleIn.map { it.toShellMode() },
            requiresConfirmation = requiresConfirmation
        )
    }

    private fun uniffi.writer_core.ShellModeDto.toShellMode(): ShellMode = when (this) {
        uniffi.writer_core.ShellModeDto.SINGLE_PANE -> ShellMode.SinglePane
        uniffi.writer_core.ShellModeDto.SUPPORTING_PANE -> ShellMode.SupportingPane
        uniffi.writer_core.ShellModeDto.TWO_PANE -> ShellMode.TwoPane
    }
}
