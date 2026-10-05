/**
 * LayoutPolicyHelper — Harmony 平台值计算
 *
 * 本文件只含 Harmony 平台值计算：
 *   - harmonyAvailablePaneCount：窗口宽 → 可用栏数（Harmony 断点）
 *   - buildHarmonyLayoutPlan：在 Core 契约之上叠加纸面宽度/内边距/网格列数
 * 这些是 Harmony 平台决策，不出现在 Core 契约里。
 *
 * Core 布局契约（shellMode/workspaceLayoutMode/primaryNavigationPlacement/metrics）
 * 的解析由 AdaptiveContext 通过 bridge.resolveLayout() 调用 Core FFI 完成，不在本文件。
 *
 * #610：Harmony 平台值（断点、纸面宽度、内边距、网格列数）是平台决策，
 * 在 buildHarmonyLayoutPlan 中计算，不出现在 Core 契约里。
 */

import { LayoutContract, LayoutMetrics, LayoutPlan, WorkspaceLayoutMode } from '../../corebridge/dto/PlatformDtos'

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
  let contentMaxWidthVp: number = 0
  let pagePaddingVp: number = 16
  let gridColumns: number = 2
  if (contract.workspaceLayoutMode === WorkspaceLayoutMode.Workbench) {
    contentMaxWidthVp = 840
    pagePaddingVp = 32
    gridColumns = harmonyAvailablePaneCount(widthVp) >= 3 ? 4 : 3
  }
  return {
    shellMode: contract.shellMode,
    workspaceLayoutMode: contract.workspaceLayoutMode,
    primaryNavigationPlacement: contract.primaryNavigationPlacement,
    contentMaxWidthVp: contentMaxWidthVp,
    pagePaddingVp: pagePaddingVp,
    gridColumns: gridColumns,
    metrics: contract.metrics
  }
}

/**
 * Core LayoutMetricsDto 的兜底值，与 core/writer_core metrics.rs 的
 * DEFAULT_* 常量逐项一致。
 *
 * 仅用于 Core 契约尚未解析出来的时刻（例如 App 冷启动默认 state）。
 * 任何一项漂移都会让冷启动那一帧按另一套工作台尺寸画出来，
 * 修改本函数前先核对 metrics.rs。
 */
export function defaultLayoutMetrics(): LayoutMetrics {
  return {
    listPaneWidthDp: 320,
    projectCardMinWidthDp: 180,
    toolPaneWidthDp: 240,
    toolRailWidthDp: 56,
    editorMinWidthDp: 240,
    toolbarHeightDp: 64,
    toolbarLeadingWidthDp: 200,
    toolbarTrailingWidthDp: 200,
    listPaneMinWidthDp: 200,
    toolPaneMinWidthDp: 200
  }
}
