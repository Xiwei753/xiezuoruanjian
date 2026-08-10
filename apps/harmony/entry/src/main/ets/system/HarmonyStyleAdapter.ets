import {
  ActionRole, ActionRegion, ActionSlot, ActionTarget, PaneRole
} from '../model/CoreDtos'

/**
 * Harmony StyleAdapter — 将 Core ActionRole/PaneRole 映射为 ArkUI 控件。
 *
 * Core 说"这里是 primaryAction"，Harmony 画成 ArkUI Button。
 * Core 说"这里是 destructiveAction"，Harmony 画成 AlertDialog 确认弹窗。
 *
 * #610：动作区域（region）与顺序（order）是产品语义，来自 Core；
 * 各端不允许自行发明动作位置。控件呈现（ArkUI 控件/布局位置）在本层决定。
 */

// ── ActionRole → ArkUI 控件类型 ──

export enum ArkControlType {
  NavigationButton,    // Back
  ProminentButton,     // CreateVolume, CreateChapter, Sync
  ProminentLarge,      // CreateProject (large)
  ContextMenuItem,     // Delete, Rename
  IconButton,          // Settings, Search
  DefaultButton        // 未知 ActionRole 降级
}

export interface ArkControl {
  type: ArkControlType
  // #610 评论二：业务目标身份透传到控件，Delete+Volume 与 Delete+Chapter 可区分绑定。
  target: ActionTarget
  isDestructive: boolean
  requiresConfirmation: boolean
}

export function mapActionSlot(slot: ActionSlot): ArkControl {
  let type: ArkControlType
  switch (slot.role) {
    case ActionRole.Back:
      type = ArkControlType.NavigationButton
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
    target: slot.target,
    isDestructive: slot.role === ActionRole.Delete,
    requiresConfirmation: slot.requiresConfirmation
  }
}

// ── ActionRegion → ArkUI 布局位置 ──

export enum ArkLayoutPosition {
  TopBarLeading,    // HeaderLeading
  TopBarTrailing,   // HeaderTrailing
  ListHeader,       // ListHeader
  ItemTrailing,     // ItemTrailing
  ContextualMenu,   // Context
  EmptyState        // EmptyState
}

export function mapRegion(region: ActionRegion): ArkLayoutPosition {
  switch (region) {
    case ActionRegion.HeaderLeading:
      return ArkLayoutPosition.TopBarLeading
    case ActionRegion.HeaderTrailing:
      return ArkLayoutPosition.TopBarTrailing
    case ActionRegion.ListHeader:
      return ArkLayoutPosition.ListHeader
    case ActionRegion.ItemTrailing:
      return ArkLayoutPosition.ItemTrailing
    case ActionRegion.Context:
      return ArkLayoutPosition.ContextualMenu
    case ActionRegion.EmptyState:
      return ArkLayoutPosition.EmptyState
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
