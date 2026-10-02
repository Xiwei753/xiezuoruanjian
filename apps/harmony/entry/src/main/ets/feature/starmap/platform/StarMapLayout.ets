// ─── StarMapLayout.ets ───
// 平台层显示布局模块：freeform 直接用 node.position、自动网格布局、节点宽高圆角层级、保存时生成 position patch。
// 不依赖 Core bridge，不依赖 ArkUI 组件。所有函数都是纯函数，无副作用。

import { StarMapNode, StarMapEmbed } from '../../../corebridge/dto/StarMapDtos'

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
 * 节点在画布坐标系里的显示尺寸（vp）。
 * 和 DEFAULT_EMBED_DIAMETER 一样导出：子星图里新建 / 移动节点要拿真实尺寸去过安全区
 * clamp（#818 复审），UI 侧禁止再手写 160 / 80。
 */
export const DEFAULT_NODE_WIDTH: number = 160
export const DEFAULT_NODE_HEIGHT: number = 80
const DEFAULT_NODE_RADIUS: number = 16
const DEFAULT_NODE_ZINDEX: number = 0
/**
 * 子星图 Embed 的显示直径（vp）。Embed 的 width / height / borderRadius 全部从这一个值推导，
 * 保证 UI 与几何计算看到的永远是同一个圆。UI 侧禁止再手写 200 / 120 / 12 这类尺寸。
 */
export const DEFAULT_EMBED_DIAMETER: number = 200
const GRID_HORIZONTAL_SPACING: number = 200
const GRID_VERTICAL_SPACING: number = 120

// ─── Freeform 布局 ───

/**
 * Freeform 布局：直接从 graph.nodes 的 authored position 构建布局节点。
 * position 是唯一真相来源 — Core 已保存的 authored position 直接映射到 PlatformLayoutNode.x/y。
 * 不重新计算、不排网格。
 *
 * @param nodes 星图节点列表（每个节点有 position.x/y）
 * @returns 平台层布局节点列表
 */
export function buildFreeformLayout(nodes: StarMapNode[]): PlatformLayoutNode[] {
  const result: PlatformLayoutNode[] = []
  for (const node of nodes) {
    result.push({
      nodeId: node.id,
      x: node.position.x,
      y: node.position.y,
      width: DEFAULT_NODE_WIDTH,
      height: DEFAULT_NODE_HEIGHT,
      radius: DEFAULT_NODE_RADIUS,
      zIndex: DEFAULT_NODE_ZINDEX,
      collapsed: false
    })
  }
  return result
}

/**
 * 从 graph.embeds 构建 Embed 布局节点。
 * Embed 位置只读 embed.position，宽高/圆角由平台层定义（正圆：宽高等于直径，圆角等于半径）。
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
 * 节点间距：水平 200vp，垂直 120vp。
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

  // 网格总宽高（以间距为步长，最后一列/行不需要额外间距，但简化处理用完整网格尺寸）
  const gridWidth: number = (cols - 1) * GRID_HORIZONTAL_SPACING + DEFAULT_NODE_WIDTH
  const gridHeight: number = (rows - 1) * GRID_VERTICAL_SPACING + DEFAULT_NODE_HEIGHT

  // 居中偏移：让网格中心对齐画布中心
  const offsetX: number = (canvasWidth - gridWidth) / 2
  const offsetY: number = (canvasHeight - gridHeight) / 2

  const result: PlatformLayoutNode[] = []
  for (let i = 0; i < nodeCount; i++) {
    const col: number = i % cols
    const row: number = Math.floor(i / cols)

    result.push({
      nodeId: nodes[i].nodeId,
      x: offsetX + col * GRID_HORIZONTAL_SPACING,
      y: offsetY + row * GRID_VERTICAL_SPACING,
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
