import hilog from '@ohos.hilog'
import { getBridge } from '../../app/AppContext'
import {
  LayoutRect,
  ResolvedWorkspaceMode,
  WindowViewport,
  WorkbenchLayoutPlan,
  WorkbenchPlacement,
  WorkbenchRole,
  WorkbenchVisibility
} from '../../corebridge/dto/PlatformDtos'

const TAG = 'WorkbenchLayoutBridge'
const DOMAIN = 0x0000

/**
 * Workbench Layout Bridge — Issue #825：接 Core 已有的 `resolve_workbench_layout`。
 *
 * Core `presentation::layout::workbench` 已经决定七个产品角色
 * （ToolbarLeading / ToolbarCenter / ToolbarTrailing / ChapterNavigation /
 * Editor / ToolPane / ToolRail）的最终 bounds。平台端只按 role 取 bounds 摆控件，
 * 不再自己写 320 / 300 这类栏宽常量，也不再在平台侧重算 workbench 布局。
 *
 * `WorkbenchVisibility` 是端侧局部 UI 状态（目录栏 / 工具 pane 是否展开），
 * 只作为输入重算 bounds，不进 Core 持久化，也不影响编辑会话。
 */
export class WorkbenchLayoutBridge {
  private static instance: WorkbenchLayoutBridge

  private constructor() {}

  static getInstance(): WorkbenchLayoutBridge {
    if (!WorkbenchLayoutBridge.instance) {
      WorkbenchLayoutBridge.instance = new WorkbenchLayoutBridge()
    }
    return WorkbenchLayoutBridge.instance
  }

  /**
   * 用当前窗口尺寸 + 端侧 pane 展开状态向 Core 要一份七角色 bounds 计划。
   * 失败返回 null，调用方保留上一份 plan（不自己兜底算宽度）。
   */
  async resolveWorkbenchLayout(widthVp: number, heightVp: number,
    visibility: WorkbenchVisibility): Promise<WorkbenchLayoutPlan | null> {
    if (widthVp <= 0 || heightVp <= 0) {
      return null
    }
    try {
      const viewport: WindowViewport = {
        widthDp: Math.round(widthVp * 100) / 100,
        heightDp: Math.round(heightVp * 100) / 100,
        // Harmony 目前不上报遮挡几何（无折叠屏分割遮挡语义），与 AdaptiveContext 保持一致。
        occlusions: []
      }
      const bridge = getBridge()
      const result = await bridge.resolveWorkbenchLayout(viewport, visibility)
      if (result.success && result.data) {
        return result.data
      }
      return null
    } catch (err) {
      hilog.error(DOMAIN, TAG, `resolveWorkbenchLayout failed: ${JSON.stringify(err)}`)
      return null
    }
  }
}

/** 从计划里取某个角色的 bounds；Core 没给这个角色时返回 null（不猜默认值）。 */
export function workbenchBounds(plan: WorkbenchLayoutPlan | null, role: WorkbenchRole): LayoutRect | null {
  if (plan === null) {
    return null
  }
  for (const placement of plan.placements) {
    if (placement.role === role) {
      return placement.bounds
    }
  }
  return null
}

/** Core 决定的最终产品模式是否为 Workbench（而不是 SinglePane）。 */
export function isWorkbenchMode(plan: WorkbenchLayoutPlan | null): boolean {
  return plan !== null && plan.mode === ResolvedWorkspaceMode.Workbench
}

/** 计划里是否真的摆了某个角色（SinglePane 模式下只返回 Editor）。 */
export function hasWorkbenchRole(plan: WorkbenchLayoutPlan | null, role: WorkbenchRole): boolean {
  if (plan === null) {
    return false
  }
  for (const placement of plan.placements as WorkbenchPlacement[]) {
    if (placement.role === role) {
      return true
    }
  }
  return false
}

export function getWorkbenchLayoutBridge(): WorkbenchLayoutBridge {
  return WorkbenchLayoutBridge.getInstance()
}