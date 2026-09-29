// ─── StarMapLayerNavigation.ets ───
// 星图层级下钻导航：在同一画布内切换层级，不 push 新 NavDestination。
// 纯平台状态，不碰 Core 数据。所有函数都是纯函数，无副作用。

// ─── 接口定义 ───

/** 层级 frame：保存某一层的星图 ID、标题、缩放和偏移，用于层级切换时恢复状态。 */
export interface StarMapLayerFrame {
  starmapId: string
  title: string
  zoomScale: number
  offsetX: number
  offsetY: number
}

/** popLayer 返回结果：新栈和弹出的 frame（栈空时 frame 为 null）。 */
export interface PopLayerResult {
  stack: StarMapLayerFrame[]
  frame: StarMapLayerFrame | null
}

// ─── 层级栈操作 ───

/**
 * 把当前层 frame 压入层级栈，准备进入子层。
 * 返回新栈（不原地修改）。
 *
 * @param stack 当前层级栈
 * @param currentFrame 当前层的 frame（即将被压入栈中保存）
 * @param newFrame 即将进入的子层 frame（仅用于语义说明，不压入栈）
 * @returns 新栈，包含被压入的 currentFrame
 */
export function pushLayer(
  stack: StarMapLayerFrame[],
  currentFrame: StarMapLayerFrame,
  newFrame: StarMapLayerFrame
): StarMapLayerFrame[] {
  const newStack: StarMapLayerFrame[] = [...stack, currentFrame]
  return newStack
}

/**
 * 从层级栈弹出最顶层 frame，用于返回父层。
 * 返回新栈和弹出的 frame（栈空时 frame 为 null）。
 *
 * @param stack 当前层级栈
 * @returns 新栈和弹出的 frame；栈空时返回 { stack: [], frame: null }
 */
export function popLayer(
  stack: StarMapLayerFrame[]
): PopLayerResult {
  if (stack.length === 0) {
    const emptyResult: PopLayerResult = { stack: [], frame: null }
    return emptyResult
  }

  const frame: StarMapLayerFrame = stack[stack.length - 1]
  const newStack: StarMapLayerFrame[] = stack.slice(0, stack.length - 1)
  const result: PopLayerResult = { stack: newStack, frame: frame }
  return result
}

// ─── Frame 工厂 ───

/**
 * 创建层级 frame。
 *
 * @param starmapId 星图 ID
 * @param title 层级标题
 * @param zoomScale 缩放比例
 * @param offsetX X 方向偏移
 * @param offsetY Y 方向偏移
 * @returns 新的 StarMapLayerFrame
 */
export function createLayerFrame(
  starmapId: string,
  title: string,
  zoomScale: number,
  offsetX: number,
  offsetY: number
): StarMapLayerFrame {
  return {
    starmapId: starmapId,
    title: title,
    zoomScale: zoomScale,
    offsetX: offsetX,
    offsetY: offsetY
  }
}
