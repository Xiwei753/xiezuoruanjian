// =============================================================================
// DesktopStyleAdapter.js — ActionRole/PaneRole → AppButton/SplitView/RightDrawer 映射
// =============================================================================
//
// Core 说"这里是 primaryAction"，Desktop 画成 AppButton primary。
// Core 说"这里是 destructiveAction"，Desktop 画成 AppButton danger + 确认弹窗。
//
// 各端不允许自行发明动作位置，所有位置必须来自 Core 的 ActionPlacement。

// ── ActionRole → Desktop 控件类型 ──

const AppControlType = {
    GhostIcon: "GhostIcon",           // Back, Settings, Search
    PrimaryButton: "PrimaryButton",   // Save, CreateProject, CreateChapter, Sync
    DangerButton: "DangerButton",     // Delete
    GhostButton: "GhostButton",       // Rename
    DefaultButton: "DefaultButton"    // 未知 ActionRole 降级
}

function mapActionSlot(slot) {
    let type
    switch (slot.role) {
        case "Back":
            type = AppControlType.GhostIcon
            break
        case "Save":
            type = AppControlType.PrimaryButton
            break
        case "CreateProject":
        case "CreateChapter":
            type = AppControlType.PrimaryButton
            break
        case "CreateVolume":
            type = AppControlType.PrimaryButton
            break
        case "Delete":
            type = AppControlType.DangerButton
            break
        case "Rename":
            type = AppControlType.GhostButton
            break
        case "Settings":
        case "Search":
            type = AppControlType.GhostIcon
            break
        case "Sync":
            type = AppControlType.PrimaryButton
            break
        default:
            type = AppControlType.DefaultButton
            break
    }

    return {
        type: type,
        isDestructive: slot.role === "Delete",
        requiresConfirmation: slot.requiresConfirmation
    }
}

// ── ActionPlacement → 布局位置 ──

const LayoutPosition = {
    TopBarLeading: "TopBarLeading",     // TopBar 左侧
    TopBarTrailing: "TopBarTrailing",   // TopBar 右侧
    FloatingBottom: "FloatingBottom",   // 右下角 FAB
    BottomTabBar: "BottomTabBar",       // 底部 TabBar
    ContextualMenu: "ContextualMenu",   // 右键菜单
    RightDrawer: "RightDrawer"          // 右侧抽屉
}

function mapPlacement(placement) {
    switch (placement) {
        case "TopLeading":
            return LayoutPosition.TopBarLeading
        case "TopTrailing":
            return LayoutPosition.TopBarTrailing
        case "Floating":
            return LayoutPosition.FloatingBottom
        case "BottomBar":
            return LayoutPosition.BottomTabBar
        case "ContextMenu":
            return LayoutPosition.ContextualMenu
        case "SidePanel":
            return LayoutPosition.RightDrawer
        default:
            return LayoutPosition.TopBarTrailing
    }
}

// ── PaneRole → 布局容器 ──

const PaneContainer = {
    SplitViewLeft: "SplitViewLeft",     // PrimaryList
    SplitViewRight: "SplitViewRight",   // Detail
    EditorArea: "EditorArea",           // Editor
    RightDrawer: "RightDrawer",         // Inspector
    SideDrawer: "SideDrawer"            // Drawer
}

function mapPaneRole(role) {
    switch (role) {
        case "PrimaryList":
            return PaneContainer.SplitViewLeft
        case "Detail":
            return PaneContainer.SplitViewRight
        case "Editor":
            return PaneContainer.EditorArea
        case "Inspector":
            return PaneContainer.RightDrawer
        case "Drawer":
            return PaneContainer.SideDrawer
        default:
            return PaneContainer.SplitViewRight
    }
}