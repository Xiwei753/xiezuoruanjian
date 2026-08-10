/**
 * LayoutPolicyHelper — 与 Core presentation/layout_contract.rs 完全一致的纯函数
 *
 * ⚠️ DEPRECATED: 本文件仅用于 Mock 模式（MockWriterCoreBridge）。
 * 正式 native 模式必须通过 bridge.resolveLayout() 调用 Core FFI。
 * 当 Mock 模式被移除后，本文件应一并删除。
 *
 * 契约映射规则必须与 Core layout_contract.rs 保持同步。
 * 任何契约变更必须先在 Core 中修改，再同步到本文件。
 *
 * #610：Harmony 平台值（断点、纸面宽度、内边距、网格列数）是平台决策，
 * 在 buildHarmonyLayoutPlan 中计算，不出现在 Core 契约里。
 */

import {
  WindowCapabilities, LayoutContract, LayoutPlan, PointerClass,
  ShellMode, WorkspacePaneMode
} from '../model/CoreDtos'

// ========== 与 Core resolve_layout() 完全一致的纯函数 ==========

/**
 * 根据窗口能力解析布局契约
 * 逻辑与 Core resolve_layout() 完全一致，纯函数，无副作用
 */
export function resolveLayoutFromCapabilities(capabilities: WindowCapabilities): LayoutContract {
  let shellMode: ShellMode
  let workspacePaneMode: WorkspacePaneMode
  let showProjectList: boolean
  let showChapterTree: boolean
  let showEditor: boolean
  let showSupporting: boolean

  const paneCount = capabilities.availablePaneCount
  if (paneCount <= 1) {
    shellMode = ShellMode.SinglePane
    workspacePaneMode = WorkspacePaneMode.SinglePane
    showProjectList = true
    showChapterTree = true
    showEditor = true
    showSupporting = false
  } else if (paneCount === 2) {
    shellMode = ShellMode.TwoPane
    workspacePaneMode = WorkspacePaneMode.ListDetail
    showProjectList = false
    showChapterTree = true
    showEditor = true
    showSupporting = false
  } else {
    shellMode = ShellMode.ThreePane
    workspacePaneMode = WorkspacePaneMode.ThreePane
    showProjectList = true
    showChapterTree = true
    showEditor = true
    showSupporting = paneCount >= 4
  }

  if (capabilities.hasSeparatingFold) {
    shellMode = ShellMode.SupportingPane
  }

  const touchInput =
    capabilities.pointerClass === PointerClass.Touch ||
    capabilities.pointerClass === PointerClass.Stylus
  const showPrimaryNavigation = !(capabilities.keyboardVisible && touchInput &&
    (shellMode === ShellMode.SinglePane || shellMode === ShellMode.SupportingPane))

  return {
    shellMode: shellMode,
    workspacePaneMode: workspacePaneMode,
    visiblePaneRoles: {
      showProjectList: showProjectList,
      showChapterTree: showChapterTree,
      showEditor: showEditor,
      showSupporting: showSupporting
    },
    showPrimaryNavigation: showPrimaryNavigation
  }
}

// ========== Harmony 平台值（不在 Core） ==========

/**
 * Harmony 侧断点：窗口宽 → 可用栏数（Harmony 平台自己的决策，不在 Core）。
 */
export function harmonyAvailablePaneCount(widthVp: number): number {
  if (widthVp < 600) {
    return 1
  } else if (widthVp < 840) {
    return 2
  }
  return 3
}

/**
 * 在 Core 契约之上叠加 Harmony 平台值，合成页面消费的 LayoutPlan。
 */
export function buildHarmonyLayoutPlan(contract: LayoutContract, widthVp: number): LayoutPlan {
  let contentMaxWidthVp: number
  let pagePaddingVp: number
  let gridColumns: number
  switch (contract.workspacePaneMode) {
    case WorkspacePaneMode.SinglePane:
      contentMaxWidthVp = 0
      pagePaddingVp = 16
      gridColumns = 2
      break
    case WorkspacePaneMode.ListDetail:
      contentMaxWidthVp = 840
      pagePaddingVp = 24
      gridColumns = 3
      break
    default:
      contentMaxWidthVp = 840
      pagePaddingVp = 32
      gridColumns = 4
      break
  }
  return {
    shellMode: contract.shellMode,
    workspacePaneMode: contract.workspacePaneMode,
    visiblePaneRoles: contract.visiblePaneRoles,
    showPrimaryNavigation: contract.showPrimaryNavigation,
    contentMaxWidthVp: contentMaxWidthVp,
    pagePaddingVp: pagePaddingVp,
    gridColumns: gridColumns
  }
}
