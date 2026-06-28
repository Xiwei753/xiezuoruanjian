package com.xiwei.sujian.ui

import com.xiwei.sujian.model.ActionPlacement
import com.xiwei.sujian.model.ActionRole
import com.xiwei.sujian.model.ActionSlot
import com.xiwei.sujian.model.PaneRole

/**
 * Android StyleAdapter — 将 Core ActionRole/PaneRole 映射为 Material3 控件。
 *
 * Core 说"这里是 primaryAction"，Android 画成 MaterialButton/FAB。
 * Core 说"这里是 destructiveAction"，Android 画成 AlertDialog 确认弹窗。
 *
 * 各端不允许自行发明动作位置，所有位置必须来自 Core 的 ActionPlacement。
 */
object AndroidStyleAdapter {

    // ── ActionRole → Material3 控件类型 ──

    enum class MaterialControlType {
        NavigationIcon,       // Back
        TopAppBarAction,      // Save, Settings, Search
        FloatingActionButton, // CreateProject, CreateChapter, Sync
        SecondaryFAB,         // CreateVolume
        ContextMenuItem,      // Delete, Rename
        DefaultButton         // 未知 ActionRole 降级
    }

    data class MaterialControl(
        val type: MaterialControlType,
        val isDestructive: Boolean = false,
        val requiresConfirmation: Boolean = false
    )

    fun mapActionSlot(slot: ActionSlot): MaterialControl {
        val type = when (slot.role) {
            ActionRole.Back -> MaterialControlType.NavigationIcon
            ActionRole.Save -> MaterialControlType.TopAppBarAction
            ActionRole.CreateProject -> MaterialControlType.FloatingActionButton
            ActionRole.CreateVolume -> MaterialControlType.SecondaryFAB
            ActionRole.CreateChapter -> MaterialControlType.FloatingActionButton
            ActionRole.Delete -> MaterialControlType.ContextMenuItem
            ActionRole.Rename -> MaterialControlType.ContextMenuItem
            ActionRole.Settings -> MaterialControlType.TopAppBarAction
            ActionRole.Sync -> MaterialControlType.FloatingActionButton
            ActionRole.Search -> MaterialControlType.TopAppBarAction
        }
        return MaterialControl(
            type = type,
            isDestructive = slot.role == ActionRole.Delete,
            requiresConfirmation = slot.requiresConfirmation
        )
    }

    // ── ActionPlacement → 布局位置 ──

    enum class LayoutPosition {
        TopBarLeading,    // TopAppBar navigationIcon
        TopBarTrailing,   // TopAppBar action 区域
        FloatingBottom,   // FAB
        BottomNavigation, // BottomNavigationView
        ContextualMenu,   // PopupMenu / ContextualActionBar
        SideDrawer        // 侧边 Drawer
    }

    fun mapPlacement(placement: ActionPlacement): LayoutPosition {
        return when (placement) {
            ActionPlacement.TopLeading -> LayoutPosition.TopBarLeading
            ActionPlacement.TopTrailing -> LayoutPosition.TopBarTrailing
            ActionPlacement.Floating -> LayoutPosition.FloatingBottom
            ActionPlacement.BottomBar -> LayoutPosition.BottomNavigation
            ActionPlacement.ContextMenu -> LayoutPosition.ContextualMenu
            ActionPlacement.SidePanel -> LayoutPosition.SideDrawer
        }
    }

    // ── PaneRole → 布局容器 ──

    enum class PaneContainer {
        LeftRecyclerView,   // PrimaryList
        RightFrameLayout,   // Detail
        EditorFragment,     // Editor
        BottomSheet,        // Inspector
        NavigationDrawer    // Drawer
    }

    fun mapPaneRole(role: PaneRole): PaneContainer {
        return when (role) {
            PaneRole.PrimaryList -> PaneContainer.LeftRecyclerView
            PaneRole.Detail -> PaneContainer.RightFrameLayout
            PaneRole.Editor -> PaneContainer.EditorFragment
            PaneRole.Inspector -> PaneContainer.BottomSheet
            PaneRole.Drawer -> PaneContainer.NavigationDrawer
        }
    }
}