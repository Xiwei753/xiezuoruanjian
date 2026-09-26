// FAB 局部锚点 — Home / Workspace 窄屏布局共用同一套算法，避免两个页面以后再次漂移。
//
// Issue #773 评论 5846511131 第5项：position() 用的是父容器（承载 FAB 的 Stack）局部坐标，
// 不是窗口坐标。Workspace 外面还有 HdsNavDestination 原生标题栏，窗口高度与内容 Stack
// 高度不相等，所以不能拿 adaptiveState.heightVp 当 Stack 的 y。
//
// 这里只认 FAB 承载 Stack 自己 onAreaChange 上报的宽高，再让开页面边距和底部安全区。
// 不读 HdsTabs / 底栏几何：FAB 与底栏保持完全独立。

import { AdaptiveState } from './AdaptiveContext'

/** FAB 尺寸（vp） */
export const FAB_SIZE: number = 56

/** FAB 距内容区底部的间距（vp） */
export const FAB_BOTTOM_GAP: number = 28

/** FAB 在承载 Stack 里的局部坐标（vp） */
export interface FabLocalAnchor {
  x: number
  y: number
}

// 右下角锚点：x 贴右侧页面边距，y 贴底部（让开系统安全区后再留设计间距）。
// hostWidth/hostHeight 为 0 表示承载 Stack 还没测量（首帧 onAreaChange 之前），
// 此时算出的坐标落在可视区外，不会先把 FAB 画到左上角再跳一次。
export function resolveFabLocalAnchor(
  hostWidth: number,
  hostHeight: number,
  state: AdaptiveState
): FabLocalAnchor {
  const anchor: FabLocalAnchor = {
    x: hostWidth - state.pagePadding - FAB_SIZE,
    y: hostHeight - state.safeBottomVp - FAB_SIZE - FAB_BOTTOM_GAP
  }
  return anchor
}
