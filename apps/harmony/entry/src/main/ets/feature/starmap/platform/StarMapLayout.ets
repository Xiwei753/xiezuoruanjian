// ─── StarMapLayout.ets ───
// 平台层显示布局模块：freeform 直接用 node.position、自动网格布局、节点卡片宽高（#984）、
// 圆角层级、保存时生成 position patch。
// Embed 的 world 几何**恒定**（#821）：位置直接读 embed.position，尺寸恒为
// DEFAULT_EMBED_DIAMETER × DEFAULT_EMBED_DIAMETER，圆角 = 半径，永远是正圆。
// Deep Zoom 档位只决定"圆里渲染多少细节"，绝不参与这里的任何尺寸或位置计算，
// 所以缩放、掉档、视口 resize 都不会让星图里的 Embed 跳一下或换形状。
// 不依赖 Core bridge，不依赖 ArkUI 组件。所有函数都是纯函数，无副作用。

import { StarMapNode, StarMapEmbed, StarMapNodeContent } from '../../../corebridge/dto/StarMapDtos'

// ─── 接口定义 ───

/** 平台层平面坐标（画布坐标系）。纯显示坐标类型，位置真相仍在 Core 的 node.position / embed.position。 */
export interface PlatformPoint {
  x: number
  y: number
}

/** 平台层布局节点：包含位置、尺寸、圆角、层级和折叠状态。这是平台层自己的布局节点，不是 Core 的 StarMapLayoutNode。 */
export interface PlatformLayoutNode {
  nodeId: string
  x: number
  y: number
  width: number
  height: number
  radius: number
  zIndex: number
  collapsed: boolean
}

/** 节点位置补丁：保存时把平台层的布局坐标转成 position patch，用于通过 Core 的节点更新 API 写回。 */
export interface NodePositionPatch {
  nodeId: string
  x: number
  y: number
}

// ─── 常量 ───

/**
 * 普通节点卡片宽度（vp，#984）。
 *
 * 普通节点不再是"圆形图标 + 名称"的 160×80 固定块，而是固定宽度的文字卡片：
 * 标题在上、正文预览在下。宽度恒定，高度由 `measureStarMapNodeCard` 按标题 / 正文
 * 行数算出来，布局命中与 UI 渲染共用同一份度量结果。
 */
export const NODE_CARD_WIDTH: number = 208
/** 卡片内边距（vp）：四边同宽。 */
export const NODE_CARD_PADDING: number = 12
/** 标题字号（vp）。 */
export const NODE_CARD_TITLE_FONT_SIZE: number = 16
/** 标题固定行高（vp）：测量与 Text.lineHeight 必须一致。 */
export const NODE_CARD_TITLE_LINE_HEIGHT: number = 22
/** 标题最多展示 2 行（视觉折行，不改写 title 字段）。 */
export const NODE_CARD_TITLE_MAX_LINES: number = 2
/** 正文预览字号（vp）。 */
export const NODE_CARD_BODY_FONT_SIZE: number = 12
/** 正文预览固定行高（vp）。 */
export const NODE_CARD_BODY_LINE_HEIGHT: number = 17
/** 正文预览最多 5 行，超出的正文仍完整保存在 Core。 */
export const NODE_CARD_BODY_PREVIEW_MAX_LINES: number = 5
/** 标题与正文之间的间距（vp）。 */
export const NODE_CARD_TITLE_BODY_GAP: number = 6
/** 左上角 kind 颜色点缀的直径 / 与标题的间距（vp）。 */
export const NODE_CARD_ACCENT_SIZE: number = 8
export const NODE_CARD_ACCENT_GAP: number = 6
/** 卡片最小高度（vp）：只有标题时也保留一块可点击的卡片。 */
export const NODE_CARD_MIN_HEIGHT: number = 64

/**
 * 节点在画布坐标系里的默认显示尺寸（vp）。
 * 和 DEFAULT_EMBED_DIAMETER 一样导出：子星图里新建 / 移动节点要拿真实尺寸去过安全区
 * clamp（#818 复审），UI 侧禁止再手写 160 / 80。
 *
 * #984：普通节点实际几何以每个节点自己的 `measureStarMapNodeCard` 结果为准；
 * 这两个常量只保留给"还没有节点内容可测"的兜底 / 其它引用方，不能再当作所有节点的固定尺寸。
 */
export const DEFAULT_NODE_WIDTH: number = NODE_CARD_WIDTH
export const DEFAULT_NODE_HEIGHT: number = NODE_CARD_MIN_HEIGHT
/**
 * 节点圆角（vp）。UI 侧禁止再手写 16。
 *
 * #821 起 Embed 任何档位都是正圆，不再有这个圆角；普通节点卡片仍用它。
 */
export const DEFAULT_NODE_RADIUS: number = 16
const DEFAULT_NODE_ZINDEX: number = 0
/**
 * 子星图 Embed 的**稳定** world 直径（vp）。Embed 的 width / height / borderRadius
 * 全部从这一个值推导，保证 UI 与几何计算看到的永远是同一个圆。
 *
 * #821：这是恒定值，不再随视觉档位或视口 resize 变化。放大靠显示变换
 * （`.scale()`）而不是把布局矩形撑大，所以组件宽高永远是 DEFAULT_EMBED_DIAMETER。
 */
export const DEFAULT_EMBED_DIAMETER: number = 200
/** 自动网格的额外间距（vp）：在节点自身实测宽高之外留出的空档。 */
const GRID_HORIZONTAL_GAP: number = 40
const GRID_VERTICAL_GAP: number = 40

// ─── 普通节点卡片度量（#984）───

/** 节点卡片度量结果。宽高是画布坐标下的真实布局矩形，UI 与命中都读这一份。 */
export interface StarMapNodeCardMetrics {
  width: number
  height: number
  titleLines: number
  bodyLines: number
  hasBody: boolean
}

/**
 * 真实文本高度测量回调（#984）。
 *
 * 平台层不自己按字符宽度猜行数：调用方必须提供与 Text 相同的字号、行高、
 * 可用宽度、最大行数与字重，由 ArkUI 的文本排版能力返回实际高度（vp）。
 * 返回 0 表示文本为空。
 */
export type NodeTextHeightMeasure = (
  text: string,
  fontSize: number,
  lineHeight: number,
  maxWidth: number,
  maxLines: number,
  fontWeight: number
) => number

/**
 * 按标题 + 正文内容计算普通节点卡片度量（#984）。
 *
 * 高度只来自 `measureTextHeight` 的真实排版结果；没有传入测量回调时
 * 不做字符宽度估算，仅退回最小高度（生产调用点必须传回调）。
 *
 * @param title 节点名称（原始字段，不做 trim）
 * @param content 节点内容；只有 inline / empty 的 body 会进入预览
 * @param measureTextHeight 平台真实文本高度测量回调
 * @returns 卡片宽高与标题 / 正文实际占用的预览行数
 */
export function measureStarMapNodeCardByContent(
  title: string,
  content: StarMapNodeContent | null,
  measureTextHeight?: NodeTextHeightMeasure
): StarMapNodeCardMetrics {
  const titleMaxWidth: number =
    NODE_CARD_WIDTH - NODE_CARD_PADDING * 2 - NODE_CARD_ACCENT_SIZE - NODE_CARD_ACCENT_GAP
  const bodyMaxWidth: number = NODE_CARD_WIDTH - NODE_CARD_PADDING * 2
  let titleHeight: number = NODE_CARD_TITLE_LINE_HEIGHT
  if (title.length > 0 && measureTextHeight !== undefined) {
    titleHeight = Math.max(0, measureTextHeight(
      title,
      NODE_CARD_TITLE_FONT_SIZE,
      NODE_CARD_TITLE_LINE_HEIGHT,
      titleMaxWidth,
      NODE_CARD_TITLE_MAX_LINES,
      500
    ))
  }
  const titleLines: number = titleHeight > 0
    ? Math.max(1, Math.min(
      NODE_CARD_TITLE_MAX_LINES,
      Math.round(titleHeight / NODE_CARD_TITLE_LINE_HEIGHT)
    ))
    : 1

  let body: string = ''
  if (content !== null && (content.type === 'inline' || content.type === 'empty')) {
    body = content.body ?? ''
  }
  let bodyHeight: number = 0
  if (body.length > 0) {
    if (measureTextHeight !== undefined) {
      bodyHeight = Math.max(0, measureTextHeight(
        body,
        NODE_CARD_BODY_FONT_SIZE,
        NODE_CARD_BODY_LINE_HEIGHT,
        bodyMaxWidth,
        NODE_CARD_BODY_PREVIEW_MAX_LINES,
        400
      ))
    } else {
      // 无测量回调时只保留一行可点击区域，绝不按字符宽度推算更多行。
      bodyHeight = NODE_CARD_BODY_LINE_HEIGHT
    }
  }
  const bodyLines: number = bodyHeight > 0
    ? Math.max(1, Math.min(
      NODE_CARD_BODY_PREVIEW_MAX_LINES,
      Math.round(bodyHeight / NODE_CARD_BODY_LINE_HEIGHT)
    ))
    : 0
  const hasBody: boolean = body.length > 0
  let height: number = NODE_CARD_PADDING * 2 + titleHeight
  if (hasBody) {
    height += NODE_CARD_TITLE_BODY_GAP + bodyHeight
  }
  return {
    width: NODE_CARD_WIDTH,
    height: Math.max(NODE_CARD_MIN_HEIGHT, height),
    titleLines: titleLines,
    bodyLines: bodyLines,
    hasBody: hasBody
  }
}

/**
 * 计算一个普通节点的卡片度量。布局、UI 渲染、创建时的安全区 clamp 都走这里，
 * 禁止再各自按固定 160×80 或另一套字符宽度公式计算。
 */
export function measureStarMapNodeCard(
  node: StarMapNode,
  measureTextHeight?: NodeTextHeightMeasure
): StarMapNodeCardMetrics {
  return measureStarMapNodeCardByContent(node.title, node.content, measureTextHeight)
}

// ─── Freeform 布局 ───

/**
 * Freeform 布局：直接从 graph.nodes 的 authored position 构建布局节点。
 * position 是唯一真相来源 — Core 已保存的 authored position 直接映射到 PlatformLayoutNode.x/y。
 * 不重新计算、不排网格。
 *
 * #984：宽高来自每个节点自己的标题 / 正文度量，不再是统一的 160×80。
 * position 仍然是卡片左上角，语义不变。
 *
 * @param nodes 星图节点列表（每个节点有 position.x/y）
 * @returns 平台层布局节点列表
 */
export function buildFreeformLayout(
  nodes: StarMapNode[],
  measureTextHeight?: NodeTextHeightMeasure
): PlatformLayoutNode[] {
  const result: PlatformLayoutNode[] = []
  for (const node of nodes) {
    const metrics: StarMapNodeCardMetrics = measureStarMapNodeCard(node, measureTextHeight)
    result.push({
      nodeId: node.id,
      x: node.position.x,
      y: node.position.y,
      width: metrics.width,
      height: metrics.height,
      radius: DEFAULT_NODE_RADIUS,
      zIndex: DEFAULT_NODE_ZINDEX,
      collapsed: false
    })
  }
  return result
}

/**
 * 从 graph.embeds 构建 Embed 布局节点。
 *
 * **#821：位置和尺寸都是恒定的。**
 * - 位置直接等于 `embed.position`（Core 的 authored position），
 *   不做任何中心锚点派生、不按尺寸反算 —— 因为显示尺寸只有一个常量，
 *   派生和反算在数学上是同一件事，多写一层只会多一个能算错的地方。
 * - 尺寸恒为 `DEFAULT_EMBED_DIAMETER × DEFAULT_EMBED_DIAMETER`，圆角 = 半径，永远是正圆。
 *
 * 代价是这一层不再接收任何"LOD 显示边界"入参：Deep Zoom 掉档不会重排布局矩形，
 * 用户缩放也不会改这里任何一个数字。缩放是显示变换的事，归 ui/StarMapScene 的 `.scale()` 管。
 *
 * @param embeds 星图内嵌列表
 * @returns 平台层布局节点列表（用于 Embed 渲染）
 */
export function buildEmbedLayoutNodes(embeds: StarMapEmbed[]): PlatformLayoutNode[] {
  const result: PlatformLayoutNode[] = []
  for (const embed of embeds) {
    result.push({
      nodeId: embed.instanceId,
      x: embed.position.x,
      y: embed.position.y,
      width: DEFAULT_EMBED_DIAMETER,
      height: DEFAULT_EMBED_DIAMETER,
      radius: DEFAULT_EMBED_DIAMETER / 2,
      zIndex: DEFAULT_NODE_ZINDEX + 1,
      collapsed: false
    })
  }
  return result
}

/**
 * Freeform 模式下直接使用传入的节点位置，不重新计算。
 * 将 positions 中的坐标应用到 nodes 上，返回更新后的节点列表。
 * 对于 positions 中不存在的节点，保留原坐标不变。
 *
 * @param nodes 当前布局节点列表
 * @param positions nodeId → {x, y} 的位置映射
 * @returns 更新位置后的布局节点列表
 */
export function applyFreeformLayout(
  nodes: PlatformLayoutNode[],
  positions: Map<string, PlatformPoint>
): PlatformLayoutNode[] {
  const result: PlatformLayoutNode[] = []
  for (const node of nodes) {
    const pos: PlatformPoint | undefined = positions.get(node.nodeId)
    if (pos !== undefined) {
      result.push({
        nodeId: node.nodeId,
        x: pos.x,
        y: pos.y,
        width: node.width,
        height: node.height,
        radius: node.radius,
        zIndex: node.zIndex,
        collapsed: node.collapsed
      })
    } else {
      result.push(node)
    }
  }
  return result
}

// ─── 自动网格布局 ───

/**
 * 自动网格布局：按行列排布节点，保留每个节点的 nodeId 与尺寸。
 * 默认列数：Math.ceil(Math.sqrt(nodeCount))，行数：Math.ceil(nodeCount / cols)。
 * 网格步长按本批节点的**最大实测宽高** + 固定空档计算（#984）：
 * 卡片高度随正文行数变化，不能再拿一个全局固定高度当步长，否则高卡片会互相压住。
 * 起始偏移：让网格在画布中居中。
 *
 * 这是"自动布局"按钮的唯一入口；加载页面不许再排网格，freeform 直接读 position。
 *
 * @param nodes 当前布局节点列表
 * @param canvasWidth 画布宽度
 * @param canvasHeight 画布高度
 * @returns 自动网格布局后的节点列表
 */
export function autoGridLayoutNodes(
  nodes: PlatformLayoutNode[],
  canvasWidth: number,
  canvasHeight: number
): PlatformLayoutNode[] {
  const nodeCount: number = nodes.length
  if (nodeCount <= 0) {
    return []
  }

  const cols: number = Math.ceil(Math.sqrt(nodeCount))
  const rows: number = Math.ceil(nodeCount / cols)

  let maxWidth: number = DEFAULT_NODE_WIDTH
  let maxHeight: number = DEFAULT_NODE_HEIGHT
  for (const node of nodes) {
    if (node.width > maxWidth) {
      maxWidth = node.width
    }
    if (node.height > maxHeight) {
      maxHeight = node.height
    }
  }
  const horizontalStep: number = maxWidth + GRID_HORIZONTAL_GAP
  const verticalStep: number = maxHeight + GRID_VERTICAL_GAP

  // 网格总宽高（以最大卡片 + 空档为步长，最后一列/行不需要额外空档）
  const gridWidth: number = (cols - 1) * horizontalStep + maxWidth
  const gridHeight: number = (rows - 1) * verticalStep + maxHeight

  // 居中偏移：让网格中心对齐画布中心
  const offsetX: number = (canvasWidth - gridWidth) / 2
  const offsetY: number = (canvasHeight - gridHeight) / 2

  const result: PlatformLayoutNode[] = []
  for (let i = 0; i < nodeCount; i++) {
    const col: number = i % cols
    const row: number = Math.floor(i / cols)

    result.push({
      nodeId: nodes[i].nodeId,
      x: offsetX + col * horizontalStep,
      y: offsetY + row * verticalStep,
      width: nodes[i].width,
      height: nodes[i].height,
      radius: nodes[i].radius,
      zIndex: nodes[i].zIndex,
      collapsed: nodes[i].collapsed
    })
  }

  return result
}

// ─── Position Patch ───

/**
 * 把平台层的布局坐标转成 position patch，用于通过 Core 的节点更新 API 写回。
 * 提取每个布局节点的 nodeId 和 x/y 坐标，生成位置补丁列表。
 *
 * @param layoutNodes 平台层布局节点列表
 * @returns 位置补丁列表
 */
export function generatePositionPatches(
  layoutNodes: PlatformLayoutNode[]
): NodePositionPatch[] {
  const patches: NodePositionPatch[] = []
  for (const node of layoutNodes) {
    patches.push({
      nodeId: node.nodeId,
      x: node.x,
      y: node.y
    })
  }
  return patches
}
