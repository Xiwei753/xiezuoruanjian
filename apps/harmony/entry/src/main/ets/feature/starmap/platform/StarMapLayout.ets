// ─── StarMapLayout.ets ───
// 平台层显示布局模块：freeform 直接用 node.position、自动网格布局、节点宽高圆角层级、保存时生成 position patch。
// Embed 的宽高圆角按"当前视觉档位"给（#820），档位判定在 StarMapVisualLod，这里只负责把尺寸落到布局节点上。
// Embed 的显示矩形以 authored 中心为锚点派生（折叠 / 展开 / 视口 resize 都不挪星图），
// 写回 Core 时用 embedAuthoredPositionFromDisplay 反解回 authored 口径。
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

/**
 * 一个 Embed 在某个视觉档位下的显示边界（局部坐标，不含任何缩放）。
 *
 * #820：这是"画出来多大"和"按多大算命中"的唯一一份真相。
 * 布局节点、命中测试、选中描边、边端点、触摸响应区全部读它，
 * 免得出现"画的是折叠摘要卡 160×80，命中和边端点还按展开的 200×200 圆算"的错位。
 * 具体档位 → 尺寸的映射见 StarMapVisualLod.embedDisplayBoundsForLod。
 */
export interface EmbedDisplayBounds {
  /** 显示宽度（vp） */
  width: number
  /** 显示高度（vp） */
  height: number
  /** 圆角 / 圆半径（vp） */
  radius: number
  /** true = 按圆处理（命中、边端点走圆逻辑），false = 按矩形处理 */
  isCircle: boolean
}

// ─── 常量 ───

/**
 * 节点在画布坐标系里的显示尺寸（vp）。
 * 和 DEFAULT_EMBED_DIAMETER 一样导出：子星图里新建 / 移动节点要拿真实尺寸去过安全区
 * clamp（#818 复审），UI 侧禁止再手写 160 / 80。
 */
export const DEFAULT_NODE_WIDTH: number = 160
export const DEFAULT_NODE_HEIGHT: number = 80
/**
 * 节点圆角（vp）。折叠成摘要卡的 Embed 也用这个圆角（#820），
 * 所以折叠态看起来和普通节点是一路的。UI 侧禁止再手写 16。
 */
export const DEFAULT_NODE_RADIUS: number = 16
const DEFAULT_NODE_ZINDEX: number = 0
/**
 * 子星图 Embed 的显示直径（vp）。Embed 展开态的 width / height / borderRadius 全部从这一个值推导，
 * 保证 UI 与几何计算看到的永远是同一个圆。UI 侧禁止再手写 200 / 120 / 12 这类尺寸。
 */
export const DEFAULT_EMBED_DIAMETER: number = 200
const GRID_HORIZONTAL_SPACING: number = 200
const GRID_VERTICAL_SPACING: number = 120

/**
 * 展开态 Embed 的显示边界（正圆）。
 *
 * @param diameterVp 展开态圆的外壳直径（vp）。默认是基准直径；
 *                   传当前 Viewport 下的显示尺寸即可得到"这一档真的画多大"的边界（#820）。
 */
export function expandedEmbedDisplayBounds(diameterVp?: number): EmbedDisplayBounds {
  const diameter: number = diameterVp !== undefined &&
    Number.isFinite(diameterVp) && diameterVp > 0 ? diameterVp : DEFAULT_EMBED_DIAMETER
  return {
    width: diameter,
    height: diameter,
    radius: diameter / 2,
    isCircle: true
  }
}

/**
 * 把派生出来的显示矩形反解回 Core 的 authored position（#820）。
 *
 * 显示矩形是**以 authored 中心为锚点**派生的（见 buildEmbedLayoutNodes），
 * 所以展开 / 折叠 / 视口 resize 只是尺寸在变，中心不动。
 * 反过来，移动保存时也必须把显示左上角换算回 authored 左上角再写进 Core，
 * 绝不能把 displayX / displayY 直接存成 authored position。
 *
 * @param displayX 显示矩形左上角 x（画布坐标）
 * @param displayY 显示矩形左上角 y（画布坐标）
 * @param displayWidth 显示宽度（vp）
 * @param displayHeight 显示高度（vp）
 * @returns authored position（Core 的 embed.position 口径）
 */
export function embedAuthoredPositionFromDisplay(
  displayX: number,
  displayY: number,
  displayWidth: number,
  displayHeight: number
): PlatformPoint {
  const width: number = Number.isFinite(displayWidth) ? displayWidth : DEFAULT_EMBED_DIAMETER
  const height: number = Number.isFinite(displayHeight) ? displayHeight : DEFAULT_EMBED_DIAMETER
  return {
    x: displayX + (width - DEFAULT_EMBED_DIAMETER) / 2,
    y: displayY + (height - DEFAULT_EMBED_DIAMETER) / 2
  }
}

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
 *
 * Embed 的**位置真相**永远只读 embed.position（Core 的 authored position），
 * 这里不因缩放、LOD 或视口 resize 回写 Core。
 *
 * 但显示矩形是**以 authored 中心为锚点派生**的：
 *   authoredCenter = authored position + DEFAULT_EMBED_DIAMETER / 2
 *   displayX/Y     = authoredCenter − 当前显示尺寸 / 2
 * 这样展开（正圆）↔ 折叠（摘要卡）之间切换、或者视口变大让圆壳变大时，
 * 都只是尺寸在变，**星图本身不会左右上下跳一下**。
 *
 * 宽高 / 圆角来自"当前视觉档位的显示边界"：
 * 展开是正圆（宽高=直径，圆角=半径），折叠是和节点同尺寸的摘要卡（#820）。
 *
 * 不带 displayBounds 就按基准直径的展开态算 —— 但那会让画出来的是摘要卡、
 * 命中和边端点还按圆算，所以递归渲染时必须由 StarMapScene 把每个 Embed 的
 * LOD 档位映射传进来。
 *
 * @param embeds 星图内嵌列表
 * @param displayBounds embed instanceId → 该 Embed 当前档位的显示边界（缺省即基准展开态）
 * @returns 平台层布局节点列表（用于 Embed 渲染）
 */
export function buildEmbedLayoutNodes(
  embeds: StarMapEmbed[],
  displayBounds?: Map<string, EmbedDisplayBounds>
): PlatformLayoutNode[] {
  const fallback: EmbedDisplayBounds = expandedEmbedDisplayBounds()
  const result: PlatformLayoutNode[] = []
  for (const embed of embeds) {
    const bounds: EmbedDisplayBounds = displayBounds !== undefined
      ? displayBounds.get(embed.instanceId) ?? fallback
      : fallback
    // 以 authored 中心为锚点派生显示矩形（#820）
    const authoredCenterX: number = embed.position.x + DEFAULT_EMBED_DIAMETER / 2
    const authoredCenterY: number = embed.position.y + DEFAULT_EMBED_DIAMETER / 2
    result.push({
      nodeId: embed.instanceId,
      x: authoredCenterX - bounds.width / 2,
      y: authoredCenterY - bounds.height / 2,
      width: bounds.width,
      height: bounds.height,
      radius: bounds.radius,
      zIndex: DEFAULT_NODE_ZINDEX + 1,
      collapsed: !bounds.isCircle
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
