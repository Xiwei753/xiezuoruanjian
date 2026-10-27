// ─── StarMapVisualTokens.ets ───
// 星图活动画布的视觉共同真相。
//
// 这里只放当前画布的共享常量：节点文字卡片颜色、标题/连线几何、Embed fit 留白。
// 不再存在 preview / interactive / shell 档位：同一个 Embed 入口在任何缩放下
// 都是同一个圆壳和标题，画面只按真实坐标连续缩放，不切换画法。
//
// 边界：
// - 不含任何状态、没有任何 ArkUI 组件类型（纯常量 + 纯函数）；
// - 不含设备 / 横竖屏分支：尺寸是恒定的基准量，缩放由 .scale() 承担；
// - 不按覆盖率、屏幕面积或缩放下发任何“画多少 / 画成什么”的开关。

// ─── 子星图（Embed）外壳 ───

/** Embed 内容适配圆形可用区时的四边留白（vp）。 */
export const STARMAP_EMBED_FIT_PADDING_VP: number = 8

// ─── 节点颜色 ───

/** 未知 kind 的兜底色。 */
export const STARMAP_NODE_FALLBACK_COLOR: string = '#4A90D9'

/** 节点 kind → 文字卡片标题色。 */
const STARMAP_NODE_KIND_COLORS: Record<string, string> = {
  'Character': '#4A90D9',
  'Location': '#F39C12',
  'Event': '#9B59B6',
  'Theme': '#2ECC71',
  'Plot': '#E74C3C',
  'Item': '#1ABC9C',
  'Organization': '#3498DB',
  'Custom': '#95A5A6'
}

/**
 * 按节点 kind 取文字卡片标题色。
 *
 * @param kind 节点 kind（StarMapNode.kind）
 * @returns 颜色字符串；未知 / 空 kind 走 STARMAP_NODE_FALLBACK_COLOR
 */
export function starmapNodeColor(kind: string): string {
  const mapped: string | undefined = STARMAP_NODE_KIND_COLORS[kind]
  return mapped !== undefined ? mapped : STARMAP_NODE_FALLBACK_COLOR
}