import {
  ActionRole, ActionPlacement, ActionSlot, PaneRole
} from '../model/CoreDtos'

/**
 * Harmony StyleAdapter — 将 Core ActionRole/PaneRole 映射为 ArkUI 控件。
 *
 * Core 说"这里是 primaryAction"，Harmony 画成 ArkUI Button。
 * Core 说"这里是 destructiveAction"，Harmony 画成 AlertDialog 确认弹窗。
 *
 * 各端不允许自行发明动作位置，所有位置必须来自 Core 的 ActionPlacement。
 */

// ── ActionRole → ArkUI 控件类型 ──

export enum ArkControlType {
  NavigationButton,    // Back
  ProminentButton,     // Save, CreateProject, CreateChapter, Sync
  ProminentLarge,      // CreateProject (large)
  ContextMenuItem,     // Delete, Rename
  IconButton,          // Settings, Search
  DefaultButton        // 未知 ActionRole 降级
}

export interface ArkControl {
  type: ArkControlType
  isDestructive: boolean
  requiresConfirmation: boolean
}

export function mapActionSlot(slot: ActionSlot): ArkControl {
  let type: ArkControlType
  switch (slot.role) {
    case ActionRole.Back:
      type = ArkControlType.NavigationButton
      break
    case ActionRole.Save:
      type = ArkControlType.ProminentButton
      break
    case ActionRole.CreateProject:
      type = ArkControlType.ProminentLarge
      break
    case ActionRole.CreateVolume:
    case ActionRole.CreateChapter:
      type = ArkControlType.ProminentButton
      break
    case ActionRole.Delete:
    case ActionRole.Rename:
      type = ArkControlType.ContextMenuItem
      break
    case ActionRole.Settings:
    case ActionRole.Search:
      type = ArkControlType.IconButton
      break
    case ActionRole.Sync:
      type = ArkControlType.ProminentButton
      break
    default:
      type = ArkControlType.DefaultButton
      break
  }

  return {
    type: type,
    isDestructive: slot.role === ActionRole.Delete,
    requiresConfirmation: slot.requiresConfirmation
  }
}

// ── ActionPlacement → 布局位置 ──

export enum ArkLayoutPosition {
  TopBarLeading,    // Row 首位
  TopBarTrailing,   // Row 末位
  FloatingBottom,   // 悬浮 Button
  BottomToolbar,    // 底部 Row toolbar
  ContextualMenu,   // BindContextMenu
  SidePanel         // 侧边 Panel
}

export function mapPlacement(placement: ActionPlacement): ArkLayoutPosition {
  switch (placement) {
    case ActionPlacement.TopLeading:
      return ArkLayoutPosition.TopBarLeading
    case ActionPlacement.TopTrailing:
      return ArkLayoutPosition.TopBarTrailing
    case ActionPlacement.Floating:
      return ArkLayoutPosition.FloatingBottom
    case ActionPlacement.BottomBar:
      return ArkLayoutPosition.BottomToolbar
    case ActionPlacement.ContextMenu:
      return ArkLayoutPosition.ContextualMenu
    case ActionPlacement.SidePanel:
      return ArkLayoutPosition.SidePanel
    default:
      return ArkLayoutPosition.TopBarTrailing
  }
}

// ── PaneRole → ArkUI 容器 ──

export enum ArkPaneContainer {
  LeftList,          // PrimaryList
  RightColumn,       // Detail
  WritingArea,       // Editor
  RightPanel,        // Inspector
  SideDrawer         // Drawer
}

export function mapPaneRole(role: PaneRole): ArkPaneContainer {
  switch (role) {
    case PaneRole.PrimaryList:
      return ArkPaneContainer.LeftList
    case PaneRole.Detail:
      return ArkPaneContainer.RightColumn
    case PaneRole.Editor:
      return ArkPaneContainer.WritingArea
    case PaneRole.Inspector:
      return ArkPaneContainer.RightPanel
    case PaneRole.Drawer:
      return ArkPaneContainer.SideDrawer
    default:
      return ArkPaneContainer.RightColumn
  }
}