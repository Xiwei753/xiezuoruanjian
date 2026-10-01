// ─── StarMapGeometry.ets ───
// 纯显示几何计算模块：节点矩形派生、命中测试、点到线段距离、边几何、坐标互转。
// 不依赖 Core bridge，不依赖 ArkUI 组件。所有函数都是纯函数，无副作用。

import {
  StarMapEdge,
  StarMapTargetPath,
  StarMapPathSegment
} from '../../../corebridge/dto/StarMapDtos'
import { PlatformLayoutNode, PlatformPoint } from './StarMapLayout'

// ─── 接口定义 ───

/** 节点显示矩形：从 platform layout 或 embed.position 派生的屏幕坐标系矩形。 */
export interface PlatformNodeRect {
  nodeId: string
  x: number
  y: number
  width: number
  height: number
  radius: number
}

/** 边渲染几何：连线起点/终点、箭头三点、标签位置。 */
export interface PlatformEdgeRender {
  edgeId: string
  startX: number
  startY: number
  endX: number
  endY: number
  arrowTipX: number
  arrowTipY: number
  arrowLeftX: number
  arrowLeftY: number
  arrowRightX: number
  arrowRightY: number
  labelX: number
  labelY: number
  label: string | null
}

/** 命中测试结果：包含 scene 作用域信息，区分命中对象类型和区域。 */
export interface HitTestResult {
  /** 命中对象所属的 scene 路径（从根星图到当前 scene 的穿越路径） */
  scenePath: StarMapPathSegment[]
  /** 命中对象类型 */
  objectKind: 'node' | 'embed' | 'embedTitle' | 'embedBorder' | 'embedInnerContent'
  /** 命中对象 ID（nodeId 或 embed instanceId） */
  objectId: string
  /** 命中区域 */
  hitRegion: 'body' | 'title' | 'border' | 'innerContent'
}

// ─── 常量 ───

const ARROW_SIZE: number = 10
const ARROW_ANGLE: number = Math.PI / 6 // 30°
const LABEL_OFFSET: number = 8

// Embed 命中区域尺寸定义（vp）
const EMBED_TITLE_HEIGHT: number = 24   // Embed title 条高度
const EMBED_BORDER_WIDTH: number = 3    // Embed 边框宽度

// ─── 节点矩形派生 ───

/**
 * 从平台层布局节点派生节点显示矩形。
 * 普通节点与 Embed 都走同一条路径：位置来自平台布局（节点取 node.position，
 * Embed 取 embed.position），宽高圆角来自平台层的显示尺寸定义。
 *
 * @param layoutNode 平台层布局节点
 * @returns 节点显示矩形
 */
export function deriveNodeRectFromLayoutNode(layoutNode: PlatformLayoutNode): PlatformNodeRect {
  return {
    nodeId: layoutNode.nodeId,
    x: layoutNode.x,
    y: layoutNode.y,
    width: layoutNode.width,
    height: layoutNode.height,
    radius: layoutNode.radius
  }
}

// ─── 命中测试 ───

/**
 * 给定屏幕坐标，返回命中的 nodeId 或 null。
 * 从后往前遍历（后绘制的在上层），返回第一个命中的节点。
 *
 * @param rects 节点矩形列表
 * @param screenX 屏幕 X 坐标
 * @param screenY 屏幕 Y 坐标
 * @returns 命中的 nodeId，或 null
 */
export function hitTest(
  rects: PlatformNodeRect[],
  screenX: number,
  screenY: number
): string | null {
  // 从后往前遍历：后绘制的节点在上层，优先命中
  for (let i = rects.length - 1; i >= 0; i--) {
    const r = rects[i]
    if (screenX >= r.x && screenX <= r.x + r.width &&
      screenY >= r.y && screenY <= r.y + r.height) {
      return r.nodeId
    }
  }
  return null
}

/**
 * 带 scene 作用域的命中测试。
 * 在现有 hitTest 基础上，返回包含 scene 路径、对象类型和命中区域的详细结果。
 *
 * Embed 命中区域区分：
 * - title（顶部 EMBED_TITLE_HEIGHT 高度区域）→ objectKind='embedTitle'，表示"选中 Embed"
 * - border（四周 EMBED_BORDER_WIDTH 宽度区域，不含 title）→ objectKind='embedBorder'，表示"选中 Embed"
 * - innerContent（排除 title 和 border 后的内部区域）→ objectKind='embedInnerContent'，让调用方递归处理 child Scene
 *
 * 普通节点命中 → objectKind='node'，hitRegion='body'
 *
 * @param rects 节点矩形列表
 * @param screenX 屏幕 X 坐标
 * @param screenY 屏幕 Y 坐标
 * @param scenePath 当前 scene 的路径段（从根星图到当前 scene 的穿越路径）
 * @param embedInstanceIds Embed 实例 ID 集合，用于区分 rect 是普通节点还是 Embed
 * @returns 命中测试结果，或 null
 */
export function hitTestWithScene(
  rects: PlatformNodeRect[],
  screenX: number,
  screenY: number,
  scenePath: StarMapPathSegment[],
  embedInstanceIds: Set<string>
): HitTestResult | null {
  // 从后往前遍历：后绘制的节点在上层，优先命中
  for (let i = rects.length - 1; i >= 0; i--) {
    const r = rects[i]
    if (screenX >= r.x && screenX <= r.x + r.width &&
      screenY >= r.y && screenY <= r.y + r.height) {
      const isEmbed: boolean = embedInstanceIds.has(r.nodeId)

      if (!isEmbed) {
        // 普通节点命中
        return {
          scenePath: scenePath,
          objectKind: 'node',
          objectId: r.nodeId,
          hitRegion: 'body'
        }
      }

      // Embed 命中：区分 title / border / innerContent
      const titleBottom: number = r.y + EMBED_TITLE_HEIGHT
      const borderRight: number = r.x + EMBED_BORDER_WIDTH
      const borderLeftInner: number = r.x + r.width - EMBED_BORDER_WIDTH
      const borderBottomInner: number = r.y + r.height - EMBED_BORDER_WIDTH

      // title 区域：顶部 EMBED_TITLE_HEIGHT 高度
      if (screenY <= titleBottom) {
        return {
          scenePath: scenePath,
          objectKind: 'embedTitle',
          objectId: r.nodeId,
          hitRegion: 'title'
        }
      }

      // border 区域：四周 EMBED_BORDER_WIDTH 宽度（不含 title 区域）
      // 左边框
      if (screenX <= borderRight) {
        return {
          scenePath: scenePath,
          objectKind: 'embedBorder',
          objectId: r.nodeId,
          hitRegion: 'border'
        }
      }
      // 右边框
      if (screenX >= borderLeftInner) {
        return {
          scenePath: scenePath,
          objectKind: 'embedBorder',
          objectId: r.nodeId,
          hitRegion: 'border'
        }
      }
      // 下边框
      if (screenY >= borderBottomInner) {
        return {
          scenePath: scenePath,
          objectKind: 'embedBorder',
          objectId: r.nodeId,
          hitRegion: 'border'
        }
      }
      // title 下方与 innerContent 上方之间的边框条（title 底部到 innerContent 顶部）
      const innerContentTop: number = titleBottom + EMBED_BORDER_WIDTH
      if (screenY <= innerContentTop) {
        return {
          scenePath: scenePath,
          objectKind: 'embedBorder',
          objectId: r.nodeId,
          hitRegion: 'border'
        }
      }

      // innerContent 区域：排除 title 和 border 后的内部内容区
      return {
        scenePath: scenePath,
        objectKind: 'embedInnerContent',
        objectId: r.nodeId,
        hitRegion: 'innerContent'
      }
    }
  }
  return null
}

// ─── 点到线段距离 ───

/**
 * 计算点到线段的最短距离。
 *
 * @param px 点 X
 * @param py 点 Y
 * @param x1 线段起点 X
 * @param y1 线段起点 Y
 * @param x2 线段终点 X
 * @param y2 线段终点 Y
 * @returns 点到线段的最短距离
 */
export function pointToSegmentDistance(
  px: number,
  py: number,
  x1: number,
  y1: number,
  x2: number,
  y2: number
): number {
  const dx: number = x2 - x1
  const dy: number = y2 - y1
  const segLenSq: number = dx * dx + dy * dy

  if (segLenSq === 0) {
    // 线段退化为点
    const dpx: number = px - x1
    const dpy: number = py - y1
    return Math.sqrt(dpx * dpx + dpy * dpy)
  }

  // 投影参数 t：点在线段上的投影位置（0=起点，1=终点）
  let t: number = ((px - x1) * dx + (py - y1) * dy) / segLenSq
  t = Math.max(0, Math.min(1, t))

  // 投影点
  const projX: number = x1 + t * dx
  const projY: number = y1 + t * dy

  const distX: number = px - projX
  const distY: number = py - projY
  return Math.sqrt(distX * distX + distY * distY)
}

// ─── 边几何计算 ───

/**
 * 计算线段与矩形边界的交点。
 * 从矩形中心向目标点画线，找到线与矩形边界的交点。
 *
 * @param cx 矩形中心 X
 * @param cy 矩形中心 Y
 * @param tx 目标点 X
 * @param ty 目标点 Y
 * @param rectX 矩形左上角 X
 * @param rectY 矩形左上角 Y
 * @param rectW 矩形宽度
 * @param rectH 矩形高度
 * @returns 交点坐标
 */
function lineRectIntersection(
  cx: number,
  cy: number,
  tx: number,
  ty: number,
  rectX: number,
  rectY: number,
  rectW: number,
  rectH: number
): PlatformPoint {
  const dx: number = tx - cx
  const dy: number = ty - cy

  if (dx === 0 && dy === 0) {
    return { x: cx, y: cy }
  }

  const halfW: number = rectW / 2
  const halfH: number = rectH / 2

  // 计算与矩形四条边的交点参数 t，取最小正 t
  let tMin: number = Infinity

  // 与左边 x = rectX 相交：t = (rectX - cx) / dx
  if (dx !== 0) {
    const t: number = (rectX - cx) / dx
    if (t > 0) {
      const iy: number = cy + t * dy
      if (iy >= rectY && iy <= rectY + rectH) {
        tMin = Math.min(tMin, t)
      }
    }
  }

  // 与右边 x = rectX + rectW 相交
  if (dx !== 0) {
    const t: number = (rectX + rectW - cx) / dx
    if (t > 0) {
      const iy: number = cy + t * dy
      if (iy >= rectY && iy <= rectY + rectH) {
        tMin = Math.min(tMin, t)
      }
    }
  }

  // 与上边 y = rectY 相交
  if (dy !== 0) {
    const t: number = (rectY - cy) / dy
    if (t > 0) {
      const ix: number = cx + t * dx
      if (ix >= rectX && ix <= rectX + rectW) {
        tMin = Math.min(tMin, t)
      }
    }
  }

  // 与下边 y = rectY + rectH 相交
  if (dy !== 0) {
    const t: number = (rectY + rectH - cy) / dy
    if (t > 0) {
      const ix: number = cx + t * dx
      if (ix >= rectX && ix <= rectX + rectW) {
        tMin = Math.min(tMin, t)
      }
    }
  }

  if (tMin === Infinity) {
    // 无交点（不应发生），返回中心
    return { x: cx, y: cy }
  }

  return { x: cx + tMin * dx, y: cy + tMin * dy }
}

/**
 * 根据边的 from/to 节点矩形计算连线的几何信息。
 *
 * 起点：fromRect 中心到 toRect 中心的连线与 fromRect 边界的交点。
 * 终点：toRect 中心到 fromRect 中心的连线与 toRect 边界的交点。
 * 箭头：在终点处沿连线方向画三角形。
 * 标签：连线中点，略偏上方。
 *
 * @param edge 边数据
 * @param fromRect 起点 node 的矩形
 * @param toRect 终点 node 的矩形
 * @returns 边渲染几何
 */
export function computeEdgeRender(
  edge: StarMapEdge,
  fromRect: PlatformNodeRect,
  toRect: PlatformNodeRect
): PlatformEdgeRender {
  const fromCx: number = fromRect.x + fromRect.width / 2
  const fromCy: number = fromRect.y + fromRect.height / 2
  const toCx: number = toRect.x + toRect.width / 2
  const toCy: number = toRect.y + toRect.height / 2

  // 起点：fromRect 边界上指向 toRect 的交点
  const start: PlatformPoint = lineRectIntersection(
    fromCx, fromCy, toCx, toCy,
    fromRect.x, fromRect.y, fromRect.width, fromRect.height
  )

  // 终点：toRect 边界上指向 fromRect 的交点
  const end: PlatformPoint = lineRectIntersection(
    toCx, toCy, fromCx, fromCy,
    toRect.x, toRect.y, toRect.width, toRect.height
  )

  // 箭头几何
  const dx: number = end.x - start.x
  const dy: number = end.y - start.y
  const lineLen: number = Math.sqrt(dx * dx + dy * dy)

  let arrowTipX: number = end.x
  let arrowTipY: number = end.y
  let arrowLeftX: number = end.x
  let arrowLeftY: number = end.y
  let arrowRightX: number = end.x
  let arrowRightY: number = end.y

  if (lineLen > 0) {
    // 单位方向向量
    const ux: number = dx / lineLen
    const uy: number = dy / lineLen

    // 箭头尖在终点
    arrowTipX = end.x
    arrowTipY = end.y

    // 箭头翼点：从尖点沿反方向回退 ARROW_SIZE，再左右各偏移
    const backX: number = end.x - ux * ARROW_SIZE
    const backY: number = end.y - uy * ARROW_SIZE

    // 垂直方向（左侧）
    const perpX: number = -uy
    const perpY: number = ux

    const wingOffset: number = ARROW_SIZE * Math.tan(ARROW_ANGLE)

    arrowLeftX = backX + perpX * wingOffset
    arrowLeftY = backY + perpY * wingOffset
    arrowRightX = backX - perpX * wingOffset
    arrowRightY = backY - perpY * wingOffset
  }

  // 标签位置：连线中点，略偏上方（垂直于连线方向）
  const midX: number = (start.x + end.x) / 2
  const midY: number = (start.y + end.y) / 2

  let labelX: number = midX
  let labelY: number = midY

  if (lineLen > 0) {
    // 垂直于连线方向，偏移 LABEL_OFFSET
    const perpX: number = -dy / lineLen
    const perpY: number = dx / lineLen
    labelX = midX + perpX * LABEL_OFFSET
    labelY = midY + perpY * LABEL_OFFSET
  }

  return {
    edgeId: edge.id,
    startX: start.x,
    startY: start.y,
    endX: end.x,
    endY: end.y,
    arrowTipX,
    arrowTipY,
    arrowLeftX,
    arrowLeftY,
    arrowRightX,
    arrowRightY,
    labelX,
    labelY,
    label: edge.label
  }
}

/**
 * 解析边端点在当前画布上的可见锚点 ID。
 * 与 Linux_qt starmap_view/edge_render.rs::resolve_edge_endpoint_anchor 对齐。
 *
 * 规则：
 * - 本地路径（starmapId 匹配且 segments 为空）：
 *   - target.type === 'node' 或 'anchor' → 返回 target.nodeId
 *   - 其他 target type → null（非几何 target）
 * - 跨层路径（starmapId 不匹配）→ null
 * - 深路径（starmapId 匹配但 segments 非空），看第一段：
 *   - segments[0].type === 'enterEmbed' → 返回 segments[0].instanceId
 *   - segments[0].type === 'enterPortal' → 返回 segments[0].nodeId
 *
 * @param path 端点路径
 * @param currentStarmapId 当前画布星图 ID
 * @returns 可见锚点 ID（nodeId 或 Embed instanceId），或 null 表示无法在当前画布定位
 */
export function resolveEdgeEndpointAnchor(
  path: StarMapTargetPath,
  currentStarmapId: string
): string | null {
  const isLocal: boolean = path.starmapId === currentStarmapId && path.segments.length === 0

  if (isLocal) {
    if (path.target.type === 'node' || path.target.type === 'anchor') {
      return path.target.nodeId
    }
    return null
  }

  // 跨层路径：starmapId 不匹配 → 无法在当前画布定位
  if (path.starmapId !== currentStarmapId) {
    return null
  }

  // 深路径：starmapId 匹配但 segments 非空，看第一段
  const seg = path.segments[0]
  if (seg.type === 'enterEmbed') {
    return seg.instanceId
  }
  if (seg.type === 'enterPortal') {
    return seg.nodeId
  }
  return null
}

/**
 * 批量计算所有边的渲染几何。
 *
 * @param edges 边列表
 * @param rectMap nodeId → PlatformNodeRect 的映射
 * @param currentStarmapId 当前画布星图 ID，用于解析边端点在当前画布的可见锚点
 * @returns 边渲染几何列表
 */
export function computeAllEdgeRenders(
  edges: StarMapEdge[],
  rectMap: Map<string, PlatformNodeRect>,
  currentStarmapId: string
): PlatformEdgeRender[] {
  const renders: PlatformEdgeRender[] = []

  for (const edge of edges) {
    const fromId: string | null = resolveEdgeEndpointAnchor(edge.from, currentStarmapId)
    const toId: string | null = resolveEdgeEndpointAnchor(edge.to, currentStarmapId)

    if (fromId === null || toId === null) {
      continue
    }

    const fromRect: PlatformNodeRect | undefined = rectMap.get(fromId)
    const toRect: PlatformNodeRect | undefined = rectMap.get(toId)

    if (fromRect === undefined || toRect === undefined) {
      continue
    }

    renders.push(computeEdgeRender(edge, fromRect, toRect))
  }

  return renders
}

// ─── 坐标互转 ───

/**
 * 画布坐标 → 屏幕坐标。
 *
 * screenX = canvasX * zoomScale + offsetX
 * screenY = canvasY * zoomScale + offsetY
 *
 * @param canvasX 画布 X
 * @param canvasY 画布 Y
 * @param zoomScale 缩放比例
 * @param offsetX X 方向偏移
 * @param offsetY Y 方向偏移
 * @returns 屏幕坐标
 */
export function canvasToScreen(
  canvasX: number,
  canvasY: number,
  zoomScale: number,
  offsetX: number,
  offsetY: number
): PlatformPoint {
  return {
    x: canvasX * zoomScale + offsetX,
    y: canvasY * zoomScale + offsetY
  }
}

/**
 * 屏幕坐标 → 画布坐标。
 *
 * canvasX = (screenX - offsetX) / zoomScale
 * canvasY = (screenY - offsetY) / zoomScale
 *
 * @param screenX 屏幕 X
 * @param screenY 屏幕 Y
 * @param zoomScale 缩放比例
 * @param offsetX X 方向偏移
 * @param offsetY Y 方向偏移
 * @returns 画布坐标
 */
export function screenToCanvas(
  screenX: number,
  screenY: number,
  zoomScale: number,
  offsetX: number,
  offsetY: number
): PlatformPoint {
  if (zoomScale === 0) {
    return { x: 0, y: 0 }
  }
  return {
    x: (screenX - offsetX) / zoomScale,
    y: (screenY - offsetY) / zoomScale
  }
}
