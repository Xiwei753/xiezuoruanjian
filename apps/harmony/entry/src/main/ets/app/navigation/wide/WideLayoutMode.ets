// WideLayoutMode — 宽屏布局模式的唯一平台判定。
//
// Issue #972：宽屏外壳形状只由根窗口的实际布局宽度决定，页面不得各自再推导宽屏模式。
// 840vp 是产品约定断点：达到该宽度使用新的左侧一级导航 + 右上全局动作条 + 页面画布；
// 低于该宽度继续使用既有窄屏 HDS 外壳。
//
// 唯一的模式状态由 PrimaryTabShell 根容器 onAreaChange 写入；AppNavigation 的接续恢复、
// SettingsOverlayController 的浮层判定和所有页面都只读取这一份，不再从 AdaptiveContext
// 各自重新算一遍。首次读取时用平台窗口状态播种，保证根容器完成首帧布局前也有正确初值。

import { getAdaptiveState, WIDTH_WIDE_MIN_VP } from '../../../ui/adaptive/AdaptiveContext'

/** 外壳布局模式。宽屏页面只接收这一个模式，不接收第二套“是否宽屏”标志。 */
export enum WideLayoutMode {
  Compact = 'Compact',
  Wide = 'Wide'
}

/** 纯函数：根据根窗口实际宽度解析唯一布局模式。 */
export function resolveWideLayoutMode(widthVp: number): WideLayoutMode {
  return widthVp >= WIDTH_WIDE_MIN_VP ? WideLayoutMode.Wide : WideLayoutMode.Compact
}

/**
 * 进程内唯一的平台布局状态。根容器测量到真实宽度后调用 update；
 * 其他模块只调用 current，不订阅第二份窗口尺寸。
 */
export class WideLayoutState {
  private static instance: WideLayoutState | null = null
  private mode: WideLayoutMode = WideLayoutMode.Compact
  private seeded: boolean = false

  static getInstance(): WideLayoutState {
    if (WideLayoutState.instance === null) {
      WideLayoutState.instance = new WideLayoutState()
    }
    return WideLayoutState.instance
  }

  current(): WideLayoutMode {
    if (!this.seeded) {
      this.mode = resolveWideLayoutMode(getAdaptiveState().widthVp)
      this.seeded = true
    }
    return this.mode
  }

  update(widthVp: number): void {
    if (widthVp > 0) {
      this.mode = resolveWideLayoutMode(widthVp)
      this.seeded = true
    }
  }
}

export function getWideLayoutState(): WideLayoutState {
  return WideLayoutState.getInstance()
}
