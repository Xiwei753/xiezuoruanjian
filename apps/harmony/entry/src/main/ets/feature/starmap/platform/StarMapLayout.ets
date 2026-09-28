// ─── StarMapLayout.ets ───
// 平台层显示布局模块：freeform 直接用 node.position、自动网格布局、节点宽高圆角层级、保存时生成 position patch。
// 不依赖 Core bridge，不依赖 ArkUI 组件。所有函数都是纯函数，无副作用。

// ─── 接口定义 ───

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

const DEFAULT_NODE_WIDTH: number = 160
const DEFAULT_NODE_HEIGHT: number = 80
const DEFAULT_NODE_RADIUS: number = 16
const DEFAULT_NODE_ZINDEX: number = 0
const GRID_HORIZONTAL_SPACING: number = 200
const GRID_VERTICAL_SPACING: number = 120

// ─── 默认布局节点 ───

/**
 * 返回默认布局节点。
 * 默认尺寸：width=160, height=80, radius=16, zIndex=0, collapsed=false。
 * 位置默认为 (0, 0)，由布局函数或调用方设置实际坐标。
 *
 * @param nodeId 节点 ID
 * @returns 默认布局节点
 */
export function defaultLayoutNode(nodeId: string): PlatformLayoutNode {
  return {
    nodeId,
    x: 0,
    y: 0,
    width: DEFAULT_NODE_WIDTH,
    height: DEFAULT_NODE_HEIGHT,
    radius: DEFAULT_NODE_RADIUS,
    zIndex: DEFAULT_NODE_ZINDEX,
    collapsed: false
  }
}

/**
 * 批量创建默认布局节点。
 *
 * @param nodeIds 节点 ID 列表
 * @returns 默认布局节点列表
 */
export function defaultLayoutNodes(nodeIds: string[]): PlatformLayoutNode[] {
  const nodes: PlatformLayoutNode[] = []
  for (const id of nodeIds) {
    nodes.push(defaultLayoutNode(id))
  }
  return nodes
}

// ─── Freeform 布局 ───

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
  positions: Map<string, { x: number, y: number }>
): PlatformLayoutNode[] {
  const result: PlatformLayoutNode[] = []
  for (const node of nodes) {
    const pos: { x: number, y: number } | undefined = positions.get(node.nodeId)
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
 * 自动网格布局：按行列排布节点。
 * 默认列数：Math.ceil(Math.sqrt(nodeCount))，行数：Math.ceil(nodeCount / cols)。
 * 节点间距：水平 200vp，垂直 120vp。
 * 起始偏移：让网格在画布中居中。
 *
 * 返回的节点 nodeId 为空字符串，由调用方根据实际节点 ID 列表填充。
 *
 * @param nodeCount 节点数量
 * @param canvasWidth 画布宽度
 * @param canvasHeight 画布高度
 * @returns 自动网格布局的节点列表
 */
export function applyAutoGridLayout(
  nodeCount: number,
  canvasWidth: number,
  canvasHeight: number
): PlatformLayoutNode[] {
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

  const nodes: PlatformLayoutNode[] = []
  for (let i = 0; i < nodeCount; i++) {
    const col: number = i % cols
    const row: number = Math.floor(i / cols)

    nodes.push({
      nodeId: '',
      x: offsetX + col * GRID_HORIZONTAL_SPACING,
      y: offsetY + row * GRID_VERTICAL_SPACING,
      width: DEFAULT_NODE_WIDTH,
      height: DEFAULT_NODE_HEIGHT,
      radius: DEFAULT_NODE_RADIUS,
      zIndex: DEFAULT_NODE_ZINDEX,
      collapsed: false
    })
  }

  return nodes
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
