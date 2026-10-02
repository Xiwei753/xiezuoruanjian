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
 * 判断点是否落在以 Embed 外接矩形为基准的正圆内。
 *
 * @param rect Embed 的外接矩形（宽高相等，圆心为几何中心）
 * @param x 命中点 X
 * @param y 命中点 Y
 * @returns 命中点在圆内（含圆周）时为 true
 */
export function pointInEmbedCircle(rect: PlatformNodeRect, x: number, y: number): boolean {
  const cx: number = rect.x + rect.width / 2
  const cy: number = rect.y + rect.height / 2
  const dx: number = x - cx
  const dy: number = y - cy
  return dx * dx + dy * dy <= (rect.width / 2) * (rect.width / 2)
}

/**
 * 带 scene 作用域的命中测试。
 * 在现有 hitTest 基础上，返回包含 scene 路径、对象类型和命中区域的详细结果。
 *
 * 普通节点命中 → objectKind='node'，hitRegion='body'（矩形判断，与 Embed 无关）。
 *
 * Embed 是正圆，所以命中判断先走圆：(x-cx)^2 + (y-cy)^2 <= r^2，不在圆内直接跳过该 Embed。
 * 圆内再区分：
 * - title（顶部 EMBED_TITLE_HEIGHT 高度区域）→ objectKind='embedTitle'，表示"选中 Embed"
 * - border（半径方向最后 EMBED_BORDER_WIDTH 的圆环）→ objectKind='embedBorder'，表示"选中 Embed"
 * - innerContent（其余圆内区域）→ objectKind='embedInnerContent'，让调用方递归处理 child Scene
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
    const isEmbed: boolean = embedInstanceIds.has(r.nodeId)

    if (isEmbed) {
      // Embed 是圆：先做圆内判断，不在圆内不算命中，继续看更下层的 rect
      if (!pointInEmbedCircle(r, screenX, screenY)) {
        continue
      }

      // title 区域：圆内顶部 EMBED_TITLE_HEIGHT 高度
      if (screenY <= r.y + EMBED_TITLE_HEIGHT) {
        return {
          scenePath: scenePath,
          objectKind: 'embedTitle',
          objectId: r.nodeId,
          hitRegion: 'title'
        }
      }

      // border 区域：圆心到命中点距离落在最外侧 EMBED_BORDER_WIDTH 宽度的圆环内
      const cx: number = r.x + r.width / 2
      const cy: number = r.y + r.height / 2
      const outer: number = r.width / 2
      const inner: number = Math.max(0, outer - EMBED_BORDER_WIDTH)
      const dx: number = screenX - cx
      const dy: number = screenY - cy
      const dist: number = Math.sqrt(dx * dx + dy * dy)
      if (dist >= inner) {
        return {
          scenePath: scenePath,
          objectKind: 'embedBorder',
          objectId: r.nodeId,
          hitRegion: 'border'
        }
      }

      // innerContent 区域：圆内剩余部分，事件交给 child Scene
      return {
        scenePath: scenePath,
        objectKind: 'embedInnerContent',
        objectId: r.nodeId,
        hitRegion: 'innerContent'
      }
    }

    // 普通节点命中：矩形判断
    if (screenX >= r.x && screenX <= r.x + r.width &&
      screenY >= r.y && screenY <= r.y + r.height) {
      return {
        scenePath: scenePath,
        objectKind: 'node',
        objectId: r.nodeId,
        hitRegion: 'body'
      }
    }
  }
  return null
}

// ─── 圆形触摸热区 ───

/** 一条水平热区带（组件局部坐标）。UI 层转成 ArkUI 的 Rectangle 交给 responseRegion。 */
export interface CircleResponseBand {
  x: number
  y: number
  width: number
  height: number
}

/** 默认水平分带数：带高 = 直径 / bands，带数越多越贴合圆，代价是热区矩形数量线性增长。 */
export const DEFAULT_CIRCLE_RESPONSE_BANDS: number = 32

/**
 * 把正圆切成若干条水平矩形热区带。
 *
 * ArkUI 组件的默认触摸热区是整个矩形，`borderRadius` + `clip(true)` 只负责视觉裁剪，
 * 不会把热区变成圆。所以圆形子视图必须自己声明热区，否则圆外四个方角仍然可点
 * （Issue #813 复审）。
 *
 * 每条带的半宽取该带**上下两条边所在高度处**圆半宽的较小值（内切），而不是带中心高度：
 * - 取带中心会让最上和最下两条带在极点附近明显外凸（直径 200、24 带时可达 28vp），
 *   于是"点在圆外却选中了子星图"，正是要消灭的那类错位；
 * - 取内切值则永远不会把圆外的点算成圆内，代价只是极点附近最外侧一条带被丢弃
 *   （默认 32 带下约 6vp 高），手指精度下可以忽略。
 *
 * @param diameter 圆的直径（组件局部坐标）
 * @param bands 水平分带数
 * @returns 热区带列表，按 y 从小到大排列；极点处宽度为 0 的带不输出
 */
export function buildCircleResponseBands(
  diameter: number,
  bands: number = DEFAULT_CIRCLE_RESPONSE_BANDS
): CircleResponseBand[] {
  const result: CircleResponseBand[] = []
  if (diameter <= 0 || bands <= 0) {
    return result
  }
  const r: number = diameter / 2
  const bandHeight: number = diameter / bands
  for (let i = 0; i < bands; i++) {
    const y: number = i * bandHeight
    const topY: number = y
    const bottomY: number = y + bandHeight
    const halfTop: number = circleHalfWidthAtY(r, topY)
    const halfBottom: number = circleHalfWidthAtY(r, bottomY)
    const half: number = Math.min(halfTop, halfBottom)
    if (half <= 0) {
      continue
    }
    result.push({
      x: r - half,
      y: topY,
      width: half * 2,
      height: bandHeight
    })
  }
  return result
}

/** 圆在给定 y 高度处的半宽（超出圆顶/圆底返回 0）。 */
function circleHalfWidthAtY(r: number, y: number): number {
  const dy: number = y - r
  if (dy <= -r || dy >= r) {
    return 0
  }
  return Math.sqrt(r * r - dy * dy)
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
 * 计算线段与圆周边界的交点。
 * 从圆心向目标点画线，找到线与圆周的真实交点，使连线端点落在圆形 Embed 的圆边上，
 * 而不是落在已经不存在的矩形边上。
 *
 * @param cx 圆心 X
 * @param cy 圆心 Y
 * @param radius 半径
 * @param tx 目标点 X
 * @param ty 目标点 Y
 * @returns 圆周上的交点坐标
 */
export function lineCircleIntersection(
  cx: number,
  cy: number,
  radius: number,
  tx: number,
  ty: number
): PlatformPoint {
  const dx: number = tx - cx
  const dy: number = ty - cy
  const distSq: number = dx * dx + dy * dy

  if (distSq === 0) {
    // 目标点与圆心重合：没有方向，直接返回圆心
    return { x: cx, y: cy }
  }

  const dist: number = Math.sqrt(distSq)
  // 目标在圆内时，按目标方向推到圆周；目标在圆外时拉回到圆周
  const t: number = radius / dist
  return { x: cx + dx * t, y: cy + dy * t }
}

/**
 * 计算单个端点所在形状的边界交点。
 * 普通节点是矩形，Embed 是正圆，两者不能共用一套边界算法。
 *
 * 这是端点几何的唯一入口：正式边的渲染端点和拉线时的预览端点都必须走它，
 * 否则会出现"预览从圆心出发、松手后正式边却贴在圆周"的跳变（Issue #813 复审）。
 *
 * @param rect 端点所在的矩形（Embed 时宽高相等）
 * @param isEmbed 该端点是否为 Embed
 * @param tx 另一个端点的目标 X（画布坐标；普通节点可传目标中心）
 * @param ty 另一个端点的目标 Y（画布坐标；普通节点可传目标中心）
 * @returns 边界交点坐标（画布坐标）
 */
export function edgeEndpointBoundaryPoint(
  rect: PlatformNodeRect,
  isEmbed: boolean,
  tx: number,
  ty: number
): PlatformPoint {
  const cx: number = rect.x + rect.width / 2
  const cy: number = rect.y + rect.height / 2
  if (isEmbed) {
    return lineCircleIntersection(cx, cy, rect.width / 2, tx, ty)
  }
  return lineRectIntersection(cx, cy, tx, ty, rect.x, rect.y, rect.width, rect.height)
}

/**
 * 根据边的 from/to 节点矩形计算连线的几何信息。
 *
 * 起点：from 端形状中心到 to 端形状中心的连线与 from 边界的交点。
 * 终点：to 端形状中心到 from 端形状中心的连线与 to 边界的交点。
 * from/to 各自按自身形状取边界：普通节点用矩形，Embed 用圆周。
 * 箭头：在终点处沿连线方向画三角形。
 * 标签：连线中点，略偏上方。
 *
 * @param edge 边数据
 * @param fromRect 起点 node 的矩形
 * @param toRect 终点 node 的矩形
 * @param fromIsEmbed 起点是否为 Embed（决定用圆还是矩形求交点）
 * @param toIsEmbed 终点是否为 Embed（决定用圆还是矩形求交点）
 * @returns 边渲染几何
 */
export function computeEdgeRender(
  edge: StarMapEdge,
  fromRect: PlatformNodeRect,
  toRect: PlatformNodeRect,
  fromIsEmbed: boolean,
  toIsEmbed: boolean
): PlatformEdgeRender {
  const fromCx: number = fromRect.x + fromRect.width / 2
  const fromCy: number = fromRect.y + fromRect.height / 2
  const toCx: number = toRect.x + toRect.width / 2
  const toCy: number = toRect.y + toRect.height / 2

  // 起点：from 边界上指向 to 中心的交点
  const start: PlatformPoint = edgeEndpointBoundaryPoint(fromRect, fromIsEmbed, toCx, toCy)

  // 终点：to 边界上指向 from 中心的交点
  const end: PlatformPoint = edgeEndpointBoundaryPoint(toRect, toIsEmbed, fromCx, fromCy)

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
 * 每个端点都要先判断是普通节点还是 Embed：普通节点用矩形求交点，Embed 用圆周求交点，
 * 否则连线端点会停在不存在的矩形边上。
 *
 * @param edges 边列表
 * @param rectMap nodeId → PlatformNodeRect 的映射
 * @param currentStarmapId 当前画布星图 ID，用于解析边端点在当前画布的可见锚点
 * @param embedInstanceIds 当前星图的 Embed instanceId 集合
 * @returns 边渲染几何列表
 */
export function computeAllEdgeRenders(
  edges: StarMapEdge[],
  rectMap: Map<string, PlatformNodeRect>,
  currentStarmapId: string,
  embedInstanceIds: Set<string>
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

    renders.push(computeEdgeRender(
      edge,
      fromRect,
      toRect,
      embedInstanceIds.has(fromId),
      embedInstanceIds.has(toId)
    ))
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
