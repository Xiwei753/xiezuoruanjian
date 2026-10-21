// WideLayoutMode — 宽屏布局模式的唯一平台判定。
//
// Issue #972：宽屏外壳形状只由根窗口的实际宽度决定，页面不得各自再推导宽屏模式。
// 840vp 是产品约定断点：达到该宽度使用新的左侧一级导航 + 右上全局动作条 + 页面画布；
// 低于该宽度继续使用既有窄屏 HDS 外壳。

import { WIDTH_WIDE_MIN_VP } from '../../../ui/adaptive/AdaptiveContext'

/** 外壳布局模式。宽屏页面只接收这一个模式，不接收第二套“是否宽屏”标志。 */
export enum WideLayoutMode {
  Compact = 'Compact',
  Wide = 'Wide'
}

/** 纯函数：根据根窗口实际宽度解析唯一布局模式。 */
export function resolveWideLayoutMode(widthVp: number): WideLayoutMode {
  return widthVp >= WIDTH_WIDE_MIN_VP ? WideLayoutMode.Wide : WideLayoutMode.Compact
}
