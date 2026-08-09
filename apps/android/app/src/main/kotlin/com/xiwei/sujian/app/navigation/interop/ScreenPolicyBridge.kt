package com.xiwei.sujian.app.navigation.interop
import com.xiwei.sujian.app.layout.model.ShellMode
import com.xiwei.sujian.app.navigation.model.ActionPlacement
import com.xiwei.sujian.app.navigation.model.ActionRole
import com.xiwei.sujian.app.navigation.model.ActionSlot
import com.xiwei.sujian.app.navigation.model.ScreenPolicy
import com.xiwei.sujian.app.navigation.model.ScreenRole
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult

class ScreenPolicyBridge(private val appServiceBridge: AppServiceBridge) {
    companion object {
        private const val TAG = "ScreenPolicyBridge"
    }

    fun resolveScreenPolicy(
        screenRole: ScreenRole,
        shellMode: ShellMode,
    ): ScreenPolicy? {
        return try {
            val result =
                appServiceBridge.resolveScreenPolicy(
                    screenRole.toDto(),
                    shellMode.toDto(),
                )
            when (result) {
                is BridgeResult.Success -> result.data.toModel()
                else -> null
            }
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "resolveScreenPolicy failed: ${e.message}", e)
            null
        }
    }

    private fun ScreenRole.toDto(): uniffi.writer_core.ScreenRoleDto =
        when (this) {
            ScreenRole.Home -> uniffi.writer_core.ScreenRoleDto.HOME
            ScreenRole.ProjectList -> uniffi.writer_core.ScreenRoleDto.PROJECT_LIST
            ScreenRole.ProjectWorkspace -> uniffi.writer_core.ScreenRoleDto.PROJECT_WORKSPACE
            ScreenRole.Writing -> uniffi.writer_core.ScreenRoleDto.WRITING
            ScreenRole.StarMap -> uniffi.writer_core.ScreenRoleDto.STAR_MAP
            ScreenRole.Stats -> uniffi.writer_core.ScreenRoleDto.STATS
            ScreenRole.Settings -> uniffi.writer_core.ScreenRoleDto.SETTINGS
            ScreenRole.Sync -> uniffi.writer_core.ScreenRoleDto.SYNC
        }

    private fun ShellMode.toDto(): uniffi.writer_core.ShellModeDto =
        when (this) {
            ShellMode.SinglePane -> uniffi.writer_core.ShellModeDto.SINGLE_PANE
            ShellMode.SupportingPane -> uniffi.writer_core.ShellModeDto.SUPPORTING_PANE
            ShellMode.TwoPane -> uniffi.writer_core.ShellModeDto.TWO_PANE
            ShellMode.ThreePane -> uniffi.writer_core.ShellModeDto.THREE_PANE
        }

    private fun uniffi.writer_core.ScreenRoleDto.toModel(): ScreenRole =
        when (this) {
            uniffi.writer_core.ScreenRoleDto.HOME -> ScreenRole.Home
            uniffi.writer_core.ScreenRoleDto.PROJECT_LIST -> ScreenRole.ProjectList
            uniffi.writer_core.ScreenRoleDto.PROJECT_WORKSPACE -> ScreenRole.ProjectWorkspace
            uniffi.writer_core.ScreenRoleDto.WRITING -> ScreenRole.Writing
            uniffi.writer_core.ScreenRoleDto.STAR_MAP -> ScreenRole.StarMap
            uniffi.writer_core.ScreenRoleDto.STATS -> ScreenRole.Stats
            uniffi.writer_core.ScreenRoleDto.SETTINGS -> ScreenRole.Settings
            uniffi.writer_core.ScreenRoleDto.SYNC -> ScreenRole.Sync
        }

    private fun uniffi.writer_core.ActionRoleDto.toModel(): ActionRole =
        when (this) {
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
            uniffi.writer_core.ActionRoleDto.SORT -> ActionRole.Sort
        }

    private fun uniffi.writer_core.ActionPlacementDto.toModel(): ActionPlacement =
        when (this) {
            uniffi.writer_core.ActionPlacementDto.TOP_LEADING -> ActionPlacement.TopLeading
            uniffi.writer_core.ActionPlacementDto.TOP_TRAILING -> ActionPlacement.TopTrailing
            uniffi.writer_core.ActionPlacementDto.FLOATING -> ActionPlacement.Floating
            uniffi.writer_core.ActionPlacementDto.BOTTOM_BAR -> ActionPlacement.BottomBar
            uniffi.writer_core.ActionPlacementDto.CONTEXT_MENU -> ActionPlacement.ContextMenu
            uniffi.writer_core.ActionPlacementDto.SIDE_PANEL -> ActionPlacement.SidePanel
            uniffi.writer_core.ActionPlacementDto.NAVIGATION -> ActionPlacement.Navigation
            uniffi.writer_core.ActionPlacementDto.LIST_HEADER -> ActionPlacement.ListHeader
            uniffi.writer_core.ActionPlacementDto.ITEM_TRAILING -> ActionPlacement.ItemTrailing
            uniffi.writer_core.ActionPlacementDto.EMPTY_STATE -> ActionPlacement.EmptyState
        }

    private fun uniffi.writer_core.ScreenPolicyDto.toModel(): ScreenPolicy {
        return ScreenPolicy(
            screenRole = screenRole.toModel(),
            actionSlots = actionSlots.map { it.toModel() },
        )
    }

    private fun uniffi.writer_core.ActionSlotDto.toModel(): ActionSlot {
        return ActionSlot(
            actionId = actionId,
            role = role.toModel(),
            placement = placement.toModel(),
            visibleIn = visibleIn.map { it.toShellMode() },
            requiresConfirmation = requiresConfirmation,
        )
    }

    private fun uniffi.writer_core.ShellModeDto.toShellMode(): ShellMode =
        when (this) {
            uniffi.writer_core.ShellModeDto.SINGLE_PANE -> ShellMode.SinglePane
            uniffi.writer_core.ShellModeDto.SUPPORTING_PANE -> ShellMode.SupportingPane
            uniffi.writer_core.ShellModeDto.TWO_PANE -> ShellMode.TwoPane
            uniffi.writer_core.ShellModeDto.THREE_PANE -> ShellMode.ThreePane
        }
}
