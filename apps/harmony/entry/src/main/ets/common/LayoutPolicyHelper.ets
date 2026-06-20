/**
 * LayoutPolicyHelper — 与 Core layout_policy.rs 完全一致的布局策略纯函数
 *
 * ⚠️ DEPRECATED: 本文件仅用于 Mock 模式（MockWriterCoreBridge）。
 * 正式 native 模式必须通过 bridge.resolveLayout() 调用 Core FFI。
 * 当 Mock 模式被移除后，本文件应一并删除。
 *
 * 断点阈值、映射规则、特殊规则覆盖必须与 Core layout_policy.rs 保持同步。
 * 任何布局策略变更必须先在 Core layout_policy.rs 中修改，再同步到本文件。
 */

import {
  WindowMetrics,
  LayoutPlan,
  FoldPosture,
  WidthClass,
  HeightClass,
  ShellMode,
  EditorMode,
  NavigationMode
} from '../model/CoreDtos'

// ========== 断点阈值（与 Core layout_policy.rs 完全一致） ==========

const WIDTH_COMPACT_MAX: number = 600
const WIDTH_MEDIUM_MAX: number = 840
const HEIGHT_COMPACT_MAX: number = 480
const HEIGHT_MEDIUM_MAX: number = 900

// ========== 纯函数 ==========

/**
 * 解析宽度断点
 * 逻辑与 Core resolve_width_class() 完全一致
 */
export function resolveWidthClass(widthVp: number): WidthClass {
  if (widthVp < WIDTH_COMPACT_MAX) {
    return WidthClass.Compact
  } else if (widthVp < WIDTH_MEDIUM_MAX) {
    return WidthClass.Medium
  } else {
    return WidthClass.Expanded
  }
}

/**
 * 解析高度断点
 * 逻辑与 Core resolve_height_class() 完全一致
 */
export function resolveHeightClass(heightVp: number): HeightClass {
  if (heightVp < HEIGHT_COMPACT_MAX) {
    return HeightClass.Compact
  } else if (heightVp < HEIGHT_MEDIUM_MAX) {
    return HeightClass.Medium
  } else {
    return HeightClass.Expanded
  }
}

/**
 * 根据窗口度量解析布局计划
 * 逻辑与 Core resolve_layout() 完全一致，纯函数，无副作用
 */
export function resolveLayoutFromMetrics(metrics: WindowMetrics): LayoutPlan {
  const widthClass = resolveWidthClass(metrics.widthVp)
  const heightClass = resolveHeightClass(metrics.heightVp)

  // 基础策略（按宽度断点）
  let shellMode: ShellMode
  let navigationMode: NavigationMode
  let editorMode: EditorMode
  let contentMaxWidthVp: number
  let pagePaddingVp: number
  let gridColumns: number
  let showSidePanel: boolean
  let showBottomBar: boolean

  switch (widthClass) {
    case WidthClass.Compact:
      shellMode = ShellMode.SinglePane
      navigationMode = NavigationMode.Stack
      editorMode = EditorMode.FullWidth
      contentMaxWidthVp = 0
      pagePaddingVp = 16
      gridColumns = 2
      showSidePanel = false
      showBottomBar = true
      break
    case WidthClass.Medium:
      shellMode = ShellMode.SupportingPane
      navigationMode = NavigationMode.Stack
      editorMode = EditorMode.CenteredPaper
      contentMaxWidthVp = 720
      pagePaddingVp = 24
      gridColumns = 3
      showSidePanel = false
      showBottomBar = true
      break
    case WidthClass.Expanded:
      shellMode = ShellMode.TwoPane
      navigationMode = NavigationMode.ListDetail
      editorMode = EditorMode.CenteredPaper
      contentMaxWidthVp = 840
      pagePaddingVp = 32
      gridColumns = 4
      showSidePanel = true
      showBottomBar = false
      break
  }

  // 特殊规则覆盖：折叠屏半展开 → SupportingPane
  if (metrics.foldPosture === FoldPosture.HalfOpened) {
    shellMode = ShellMode.SupportingPane
  }

  // 特殊规则覆盖：键盘可见 → 隐藏底栏
  if (metrics.keyboardVisible) {
    showBottomBar = false
  }

  const plan: LayoutPlan = {
    widthClass,
    heightClass,
    shellMode,
    editorMode,
    navigationMode,
    contentMaxWidthVp,
    pagePaddingVp,
    gridColumns,
    showSidePanel,
    showBottomBar,
    sidePanelWidthVp: showSidePanel ? 320 : 0,
    primaryPaneWeight: showSidePanel ? 0.35 : 1.0,
    detailPanelMaxWidthVp: contentMaxWidthVp
  }

  return plan
}