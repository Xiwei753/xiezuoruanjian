// ─── StarMapVisualTokens.ets ───
// 星图**视觉真相的唯一出处**（#821 复审第二轮）。
//
// 为什么要有这个文件：Deep Zoom 的 preview 档和 interactive 档画的是**同一张图**，
// 只是前者是 Canvas 静态投影、后者是真组件。一旦两边的样式数字各写一份
// （节点画成圆还是画成圆角矩形、颜色表各一份、fit 留白 6 还是 8），
// 跨过 70% 门槛时就会"换皮"——这正是 #821 要消灭的那种跳变。
//
// 所以这里只放**平台视觉共同真相**：颜色、preview 圆形降级的尺寸、fit 留白。
// #984 之后 interactive 的普通节点是文字卡片，宽高由 StarMapLayout 按内容度量；
// preview 仍是低倍率下的圆点，按同一份布局矩形居中，不再依赖固定圆心。
//
// 边界：
// - 不含任何状态、没有任何 ArkUI 组件类型（纯常量 + 纯函数）；
// - 不含设备 / 横竖屏分支：尺寸是恒定的基准量，缩放由 .scale() 承担；
// - 与 Deep Zoom 档位无关：档位只决定"画多少"，不决定"画成什么样"。

// ─── 普通节点 ───

/** 低倍率 preview 里节点圆形色块的直径（vp，局部基准单位）。 */
export const STARMAP_NODE_GLYPH_DIAMETER: number = 56

/** 低倍率 preview 里节点圆形色块的半径。 */
export const STARMAP_NODE_GLYPH_RADIUS: number = STARMAP_NODE_GLYPH_DIAMETER / 2

/** preview 里节点标题字号（vp）。 */
export const STARMAP_NODE_LABEL_FONT_SIZE: number = 13

// ─── 子星图（Embed）外壳 ───

/**
 * Embed 局部 fit 的留白（vp）。
 *
 * preview 和 interactive 必须是同一个数：留白不同 → 同一张子图在两档里
 * 内容整体大小不同，跨 70% 门槛时会小小跳一下。
 */
export const STARMAP_EMBED_FIT_PADDING_VP: number = 8

// ─── preview 档的绘制阈值 ───

/**
 * 画节点标题所需的最小投影直径（vp）。
 *
 * 只决定"写不写文字"，不决定节点本体的形状和颜色：字看不清最多不写，
 * 圆还是那个圆、颜色还是那个颜色（#821：只做视觉缩小，不换对象身份）。
 */
export const STARMAP_NODE_LABEL_MIN_PROJECTED_DIAMETER_VP: number = 44

// ─── 节点颜色 ───

/** 未知 kind 的兜底色。 */
export const STARMAP_NODE_FALLBACK_COLOR: string = '#4A90D9'

/** 节点 kind → 圆形色块颜色。preview 与 interactive 共用同一张表。 */
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
 * 按节点 kind 取圆形色块颜色。
 *
 * @param kind 节点 kind（StarMapNode.kind）
 * @returns 颜色字符串；未知 / 空 kind 走 STARMAP_NODE_FALLBACK_COLOR
 */
export function starmapNodeColor(kind: string): string {
  const mapped: string | undefined = STARMAP_NODE_KIND_COLORS[kind]
  return mapped !== undefined ? mapped : STARMAP_NODE_FALLBACK_COLOR
}