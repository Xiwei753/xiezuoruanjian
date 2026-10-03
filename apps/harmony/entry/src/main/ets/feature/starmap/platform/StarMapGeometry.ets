// ─── StarMapGeometry.ets ───
// 纯显示几何计算模块：节点矩形派生、命中测试、点到线段距离、边几何、坐标互转。
// 不依赖 Core bridge，不依赖 ArkUI 组件。所有函数都是纯函数，无副作用。
//
// #820：Embed 有展开（正圆）与折叠（节点尺寸摘要卡）两副面孔，形状一律从
// 显示矩形自己读出来（isCircularDisplayRect），所以命中、边端点、响应区
// 和画出来的那一块永远是同一块几何；具体尺寸见 StarMapVisualLod.embedDisplayBoundsForLod。

import {
  StarMapEdge,
  StarMapTargetPath,
  StarMapTargetDetail,
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

/**
 * 递归 Scene 层级深度上限（Issue #816）。
 * 与 Core 侧 resolve_target 的深度限制对齐；病态自引用数据也不会把递归打爆。
 */
export const MAX_RECURSE_SCENE_DEPTH: number = 32

/**
 * Embed 命中规格（vp）——**唯一真相源**。
 *
 * 这里定义的是*交互热区*，不是视觉描边宽度：
 * - 产品规则是"点文字/边框选中子星图，点内部编辑子星图"；
 * - 视觉描边只有 1vp（未选中）/ 3vp（选中），那是给人看的；
 * - 手指点圆环时按视觉宽度命中只有 1~3vp，远窄于手指精度，所以交互热区单独取值。
 *
 * StarMapScene 的 title 层高度和 onTouchIntercept 的圆环判定都 import 这两个常量，
 * 不允许在 UI 侧再抄一份：一旦两边不一致，就会出现"手指按这里算外层边框、
 * 拉线松手到同一点却算子图内部"的错位（Issue #813 复审）。
 *
 * 任何调整只改这里一处，命中测试与真实触摸就必然一致。
 */
export const EMBED_TITLE_HIT_HEIGHT: number = 24
export const EMBED_BORDER_HIT_WIDTH: number = 12

/** Embed 选择热区换算到某 Scene 画布坐标后的结果（#818 复审）。 */
export interface EmbedHitMetrics {
  /** 顶部标题带高度（本 Scene 画布坐标） */
  titleHitHeight: number
  /** 外圈圆环宽度（本 Scene 画布坐标） */
  borderHitWidth: number
}

/**
 * 把屏幕侧的 Embed 选择热区规格换算成某个 Scene 画布坐标下的宽度。
 *
 * #818 复审发现的错位：24 / 12 是**手指在屏幕上**能点到的尺寸，
 * 但 `viewportScaleValue()` 已经把本层 local fit 乘了进去，深一层再乘一层，
 * 于是"视觉圆变小"的同时"可点的标题 / 圆环"也跟着缩成几 vp，
 * 到子子星图时 #373 的"点文字 / 边框选中子星图"事实上已经不可操作。
 *
 * 画布坐标 = 屏幕坐标 / 本 Scene 累计比例，所以换算就是除一次；
 * 同时上限卡在半径，避免本层比例极小时热区反过来吃掉整个 innerContent。
 *
 * 真实触摸分流（ui/StarMapScene 的 onTouchIntercept）和递归命中
 * （resolveRecursiveHit → hitTestWithScene）必须都调这一个函数，
 * 不允许一处自己乘 scale、另一处拿 24 / 12 当画布常量（#818 复审）。
 *
 * @param sceneScale 本 Scene 的累计有效比例（根 = camera，子 = camera × fit₁ × …）
 * @param embedRadius 本 Scene 画布坐标下 Embed 圆的半径
 * @returns 画布坐标下的标题带高度与圆环宽度
 */
export function embedHitMetricsForScene(sceneScale: number, embedRadius: number): EmbedHitMetrics {
  const scale: number = sceneScale > 0 ? sceneScale : 1
  const limit: number = embedRadius > 0 ? embedRadius : 0
  return {
    titleHitHeight: Math.min(limit, EMBED_TITLE_HIT_HEIGHT / scale),
    borderHitWidth: Math.min(limit, EMBED_BORDER_HIT_WIDTH / scale)
  }
}

/**
 * Embed 壳上那个承载 child Scene 的输入节点的稳定 id 前缀（#818 复审）。
 *
 * `onChildTouchTest` 只看得到**显式设了 .id() 的命名节点**，所以递归 Scene 必须靠这个
 * 稳定 id 才能被父级的父级分发事件时明确指认，而不是退回 Stack 默认命中顺序。
 */
export const EMBED_INPUT_NODE_ID_PREFIX: string = 'starmap_embed_input_'

/**
 * 某个 Scene 画布里某个 Embed 壳的稳定输入节点 id。
 *
 * 必须带 scenePath：同一颗 Embed instanceId 会在每一层父图里各出现一次，
 * 只用 instanceId 会重名，父级就分不清自己该转发给哪一层。
 *
 * @param scenePath 该 Embed 所属 Scene 的路径
 * @param embedInstanceId 该 Embed 的 instanceId
 * @returns 稳定输入节点 id
 */
export function describeEmbedInputNodeId(
  scenePath: StarMapPathSegment[],
  embedInstanceId: string
): string {
  return `${EMBED_INPUT_NODE_ID_PREFIX}${describeScenePath(scenePath)}_${embedInstanceId}`
}

/** 判断一个节点 id 是不是 Embed 输入节点（父级 onChildTouchTest 只转发这类）。 */
export function isEmbedInputNodeId(nodeId: string): boolean {
  return nodeId.indexOf(EMBED_INPUT_NODE_ID_PREFIX) === 0
}

/**
 * 某个 Scene 自己（不是它内部的 Embed 壳）的稳定输入节点 id。
 *
 * 根 Scene 也会拿到一个（`..._root`）；子 Scene 的 id 就是父层那颗 Embed 壳的 id，
 * 于是"父级要指名发给当前 child"这件事有唯一可查的名字，不再依赖 Stack 默认命中顺序。
 */
export function describeSceneInputNodeId(scenePath: StarMapPathSegment[]): string {
  return `${EMBED_INPUT_NODE_ID_PREFIX}${describeScenePath(scenePath)}`
}

/**
 * 从输入节点 id 里取回 instanceId；不是本 Scene 的 Embed 输入节点时返回 null。
 *
 * 父级 onChildTouchTest 拿到的是一串命名节点，只有知道它是哪颗 Embed，
 * 才能算出该往这颗圆的标题带 / 圆环 / 内部继续分发（#818 复审）。
 */
export function extractEmbedInstanceIdFromInputNodeId(
  scenePath: StarMapPathSegment[],
  nodeId: string
): string | null {
  const prefix: string = `${EMBED_INPUT_NODE_ID_PREFIX}${describeScenePath(scenePath)}_`
  if (nodeId.indexOf(prefix) !== 0) {
    return null
  }
  const instanceId: string = nodeId.substring(prefix.length)
  return instanceId.length > 0 ? instanceId : null
}

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
 * 这个显示矩形是否要按**正圆**处理（#820）。
 *
 * Embed 有两副面孔（`embedDisplayBoundsForLod`）：
 * - 展开：正圆，宽高都等于直径、圆角 = 直径 / 2
 * - 折叠：和普通节点同尺寸的摘要卡（160×80、圆角 16）
 *
 * 形状不另开一个参数传进来，而是**从同一份显示边界读出来**（宽高相等且圆角盖满宽）。
 * 这样命中测试、边端点、响应区拿到的都是"画出来的那一个矩形"，
 * 不会出现"画的是摘要卡、命中和连线端点还按 200×200 的圆算"的错位（#820）。
 *
 * @param rect 显示矩形
 * @returns true 表示按圆处理
 */
export function isCircularDisplayRect(rect: PlatformNodeRect): boolean {
  return rect.width === rect.height && rect.radius * 2 >= rect.width
}

/**
 * 带 scene 作用域的命中测试。
 * 在现有 hitTest 基础上，返回包含 scene 路径、对象类型和命中区域的详细结果。
 *
 * 普通节点命中 → objectKind='node'，hitRegion='body'（矩形判断，与 Embed 无关）。
 *
 * Embed 折叠成摘要卡时（#820）是一张矩形：整块热区 → objectKind='embed'、hitRegion='body'，
 * 也就是"点它选中这个子星图"，不再往里递归——摘要卡里并没有 child Scene。
 *
 * Embed 展开时是正圆，所以命中判断先走圆：(x-cx)^2 + (y-cy)^2 <= r^2，不在圆内直接跳过该 Embed。
 * 圆内再区分：
 * - title（顶部标题带）→ objectKind='embedTitle'，表示"选中 Embed"
 * - border（半径方向最外侧的圆环）→ objectKind='embedBorder'，表示"选中 Embed"
 *   标题带与圆环的屏幕侧规格固定是 EMBED_TITLE_HIT_HEIGHT / EMBED_BORDER_HIT_WIDTH，
 *   由 embedHitMetricsForScene 按本 Scene 累计比例换算成画布坐标（#818 复审）
 * - innerContent（其余圆内区域）→ objectKind='embedInnerContent'，让调用方递归处理 child Scene
 *
 * @param rects 节点矩形列表
 * @param screenX 屏幕 X 坐标
 * @param screenY 屏幕 Y 坐标
 * @param scenePath 当前 scene 的路径段（从根星图到当前 scene 的穿越路径）
 * @param embedInstanceIds Embed 实例 ID 集合，用于区分 rect 是普通节点还是 Embed
 * @param sceneScale 当前 scene 的累计有效比例；热区按它换算成画布坐标，
 *        保证"手指在屏幕上能点到的宽度"不随递归深度缩水（#818 复审）
 * @returns 命中测试结果，或 null
 */
export function hitTestWithScene(
  rects: PlatformNodeRect[],
  screenX: number,
  screenY: number,
  scenePath: StarMapPathSegment[],
  embedInstanceIds: Set<string>,
  sceneScale: number
): HitTestResult | null {
  // 从后往前遍历：后绘制的节点在上层，优先命中
  for (let i = rects.length - 1; i >= 0; i--) {
    const r = rects[i]
    const isEmbed: boolean = embedInstanceIds.has(r.nodeId)

    if (isEmbed) {
      // Embed 折叠成摘要卡时是一张矩形（#820）：整块热区都属于这个 Embed，
      // 点它就是"选中这个子星图"，不再往内递归（摘要卡里没有 child Scene）。
      if (!isCircularDisplayRect(r)) {
        if (screenX >= r.x && screenX <= r.x + r.width &&
          screenY >= r.y && screenY <= r.y + r.height) {
          return {
            scenePath: scenePath,
            objectKind: 'embed',
            objectId: r.nodeId,
            hitRegion: 'body'
          }
        }
        continue
      }

      // 展开态 Embed 是圆：先做圆内判断，不在圆内不算命中，继续看更下层的 rect
      if (!pointInEmbedCircle(r, screenX, screenY)) {
        continue
      }

      // 屏幕侧的 24 / 12 换算成本 Scene 画布坐标下的宽度（#818 复审）：
      // 不换算的话每深一层就再乘一次 local fit，可点宽度会缩到几 vp。
      const metrics: EmbedHitMetrics = embedHitMetricsForScene(sceneScale, r.width / 2)

      // title 区域：圆内顶部 metrics.titleHitHeight 高度
      if (screenY <= r.y + metrics.titleHitHeight) {
        return {
          scenePath: scenePath,
          objectKind: 'embedTitle',
          objectId: r.nodeId,
          hitRegion: 'title'
        }
      }

      // border 区域：圆心到命中点距离落在最外侧 metrics.borderHitWidth 宽度的圆环内
      const cx: number = r.x + r.width / 2
      const cy: number = r.y + r.height / 2
      const outer: number = r.width / 2
      const inner: number = Math.max(0, outer - metrics.borderHitWidth)
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

/**
 * 单条热区带的最大高度（vp，屏幕坐标）。
 *
 * 分带数由"直径 / 该值"推导，而不是写死带数：内切算法必然丢掉最上和最下各一条
 * （极点处半宽为 0），丢掉的圆帽高度就等于带高。如果写死带数，圆帽高度会
 * `直径/带数` 随 zoom 一起放大——3x 时直径 600、带数 32，圆顶和圆底各有
 * 18.75vp 看得见却点不到，其中顶部本来就是 title 选中区（Issue #813 复审）。
 * 固定最大带高后，无论 zoom 是 0.3、1 还是 3，圆帽都只有这一个固定上限。
 *
 * 取 3 而不是 1：带高越小越贴合圆，但热区矩形数量按 `直径/带高` 线性增长，
 * 3x 缩放下 1vp 会是 600 条。3vp 已远小于手指精度。
 */
export const MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP: number = 3

/**
 * 把正圆切成若干条水平矩形热区带。
 *
 * ArkUI 组件的默认触摸热区是整个矩形，`borderRadius` + `clip(true)` 只负责视觉裁剪，
 * 不会把热区变成圆。所以圆形子视图必须自己声明热区，否则圆外四个方角仍然可点
 * （Issue #813 复审）。
 *
 * 每条带的半宽取该带**上下两条边所在高度处**圆半宽的较小值（内切），不是带中心高度：
 * - 取带中心会让最上和最下两条带在极点附近明显外凸（直径 200、24 带时可达 28vp），
 *   于是"点在圆外却选中了子星图"，正是要消灭的那类错位；
 * - 取内切值则永远不会把圆外的点算成圆内，代价只是极点附近最外侧一条带被丢弃
 *   （高度不超过 MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP，手指精度下可以忽略）。
 *
 * @param diameter 圆的直径（组件局部坐标，已含 zoom）
 * @param maxBandHeight 单条带的最大高度（vp）
 * @returns 热区带列表，按 y 从小到大排列；极点处宽度为 0 的带不输出
 */
export function buildCircleResponseBands(
  diameter: number,
  maxBandHeight: number = MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP
): CircleResponseBand[] {
  const result: CircleResponseBand[] = []
  if (diameter <= 0 || maxBandHeight <= 0) {
    return result
  }
  const r: number = diameter / 2
  // 带高向上取整到 ceil(直径/上限) 条，保证每条带高都 <= 上限
  const bands: number = Math.max(1, Math.ceil(diameter / maxBandHeight))
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
 * 普通节点和**折叠态 Embed** 是矩形，**展开态 Embed** 是正圆，不能共用一套边界算法。
 *
 * 形状同样从显示矩形自己读出来（isCircularDisplayRect），所以 Embed 折叠时连线端点
 * 会贴到摘要卡的矩形边上，而不是贴到它已经不存在的圆周上（#820）。
 *
 * 这是端点几何的唯一入口：正式边的渲染端点和拉线时的预览端点都必须走它，
 * 否则会出现"预览从圆心出发、松手后正式边却贴在圆周"的跳变（Issue #813 复审）。
 *
 * @param rect 端点所在的矩形（展开态 Embed 时宽高相等）
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
  if (isEmbed && isCircularDisplayRect(rect)) {
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

// ─── 递归 Scene 上下文与递归命中（Issue #816） ───

/**
 * 递归 Scene 的几何数据源（结构化契约，不依赖 ArkUI 组件）。
 *
 * StarMapScene 把自己注册成这个形状的对象放进 StarMapSceneRegistry，
 * 平台层递归命中只需要这五个几何量 + 子 Embed 列表，所以整个递归命中
 * 可以在没有 ArkUI 的情况下纯函数测试。
 *
 * 坐标约定（全局统一，#816）：
 * - `rects` 是**本 Scene 画布坐标**（与 node.position / embed.position 同一坐标系），
 *   不是屏幕坐标。hitTestWithScene 的参数名叫 screenX/screenY 是历史遗留，
 *   实际一直传的是画布坐标，这里沿用同一约定，避免两套算法混用。
 * - `scale / offsetX / offsetY` 是本 Scene 的视口换算：屏幕坐标 = 画布坐标 * scale + offset。
 */
export interface RecursiveSceneSource {
  /** 本 Scene 的绝对路径（从根星图到本 Scene 的 enterEmbed 序列） */
  scenePath: StarMapPathSegment[]
  /** 本 Scene 对应的星图 ID */
  starmapId: string
  /** 本 Scene 画布坐标下的全部矩形（普通节点 + Embed） */
  rects: PlatformNodeRect[]
  /** 本 Scene 的 Embed instanceId 集合（区分 rect 是普通节点还是 Embed） */
  embedInstanceIds: Set<string>
  /** 本 Scene 画布坐标下的全部边（#816：边也要能被点到，递归命中才有 edge 这一类） */
  edges: PlatformEdgeRender[]
  /**
   * 本 Scene 的画布 → 局部屏幕**累计**比例（#818）。
   * 根层 = 全局相机比例；子层 = 局部适配比例 × 祖先累计比例。
   * 写进来的是"画面上真正乘着的那个数"，命中换算才能和渲染一致。
   */
  scale: number
  /** 本 Scene 的累计 X 偏移（子层同样是局部偏移 × 祖先累计） */
  offsetX: number
  /** 本 Scene 的累计 Y 偏移（子层同样是局部偏移 × 祖先累计） */
  offsetY: number
  /** 本 Scene 直接子代的 Scene 列表（子图未加载完时就没有对应项） */
  getChildEmbeds(): RecursiveSceneSource[]
}

/** 递归命中的目标类型（Embed 标题/边框/内部统一归到 embed，区域由 hitRegion 区分）。 */
export type RecursiveHitKind = 'node' | 'embed' | 'edge'

/** 递归命中解析出的真实目标（已带绝对 scenePath）。 */
export interface RecursiveHitTarget {
  /** 目标所属 Scene 的绝对路径 */
  scenePath: StarMapPathSegment[]
  /** 目标所属 Scene 的星图 ID */
  starmapId: string
  /** 目标类型 */
  objectKind: RecursiveHitKind
  /** 目标 ID（nodeId / Embed instanceId / edgeId） */
  objectId: string
  /** 命中区域 */
  hitRegion: 'body' | 'title' | 'border' | 'innerContent'
}

/** 递归命中的完整结果。 */
export interface RecursiveHit {
  /**
   * 手势归属 Scene：这一层就是"手指所在的画布"。
   * 命中普通节点、命中 Embed 标题/边框、子图未加载时，都归属本层；
   * 命中 Embed 内部且子图已加载时，归属子 Scene。
   */
  ownerScenePath: StarMapPathSegment[]
  /** 归属 Scene 的星图 ID */
  ownerStarmapId: string
  /** 真实命中的对象；null 表示归属 Scene 内的空白 */
  target: RecursiveHitTarget | null
  /** 从根到归属 Scene 之间的 Embed 路径（不含归属 Scene 自身） */
  embedPath: StarMapPathSegment[]
}

/** 递归 Scene 上下文：把 Registry 提供的 SceneSource 展开成可递归查找的树。 */
export interface RecursiveSceneContext {
  scenePath: StarMapPathSegment[]
  starmapId: string
  rects: PlatformNodeRect[]
  embedInstanceIds: Set<string>
  edges: PlatformEdgeRender[]
  scale: number
  offsetX: number
  offsetY: number
  /** Embed instanceId → 子 Scene 上下文（子图加载完成才有） */
  children: Map<string, RecursiveSceneContext>
}

/** item 引用：一个可被选中/连线的对象，带绝对路径和宿主星图。 */
export interface StarMapItemRef {
  /** 容纳该 item 的 Scene 绝对路径 */
  scenePath: StarMapPathSegment[]
  /** 容纳该 item 的 Scene 的星图 ID */
  starmapId: string
  /** item 类型 */
  kind: 'node' | 'embed'
  /** item ID */
  itemId: string
}

/** 跨层连线的落点方案：宿主 Scene + 两条相对宿主的端点路径。 */
export interface CrossLayerEdgePlan {
  /** 宿主 Scene（两端的最近公共祖先）的绝对路径 */
  hostScenePath: StarMapPathSegment[]
  /** 宿主 Scene 的星图 ID，也就是 addStarMapEdge 的 starmapId */
  hostStarmapId: string
  /** 起点，相对宿主 Scene 的 StarMapTargetPath */
  from: StarMapTargetPath
  /** 终点，相对宿主 Scene 的 StarMapTargetPath */
  to: StarMapTargetPath
}

// ─── Scene 路径工具 ───

/** 深拷贝路径段。 */
export function cloneScenePath(path: StarMapPathSegment[]): StarMapPathSegment[] {
  const out: StarMapPathSegment[] = []
  for (const seg of path) {
    out.push({ type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId })
  }
  return out
}

/** 逐段比较两条 Scene 路径是否完全相同。 */
export function isSameScenePath(a: StarMapPathSegment[], b: StarMapPathSegment[]): boolean {
  if (a.length !== b.length) {
    return false
  }
  for (let i = 0; i < a.length; i++) {
    if (a[i].type !== b[i].type ||
      a[i].instanceId !== b[i].instanceId ||
      a[i].nodeId !== b[i].nodeId) {
      return false
    }
  }
  return true
}

/** 两条 Scene 路径的最长公共前缀。 */
export function commonScenePathPrefix(
  a: StarMapPathSegment[],
  b: StarMapPathSegment[]
): StarMapPathSegment[] {
  const out: StarMapPathSegment[] = []
  const max: number = Math.min(a.length, b.length)
  for (let i = 0; i < max; i++) {
    if (a[i].type !== b[i].type ||
      a[i].instanceId !== b[i].instanceId ||
      a[i].nodeId !== b[i].nodeId) {
      break
    }
    out.push({ type: a[i].type, instanceId: a[i].instanceId, nodeId: a[i].nodeId })
  }
  return out
}

/** 生成单个 enterEmbed 路径段。 */
export function embedSegment(instanceId: string): StarMapPathSegment {
  return { type: 'enterEmbed', instanceId: instanceId, nodeId: null }
}

/**
 * Scene 路径的可读描述，用作 Registry 主键和日志字段。
 * 根 Scene 是 `root`，进入子星图后是 `root/embed:<instanceId>`，逐层追加。
 */
export function describeScenePath(path: StarMapPathSegment[]): string {
  if (path.length === 0) {
    return 'root'
  }
  let out: string = 'root'
  for (const seg of path) {
    if (seg.type === 'enterEmbed') {
      out += `/embed:${seg.instanceId ?? ''}`
    } else if (seg.type === 'enterPortal') {
      out += `/portal:${seg.nodeId ?? ''}`
    } else {
      out += `/${seg.type}:${seg.instanceId ?? ''}`
    }
  }
  return out
}

/** 构造空 target 详情的骨架（Core DTO 的每个字段都是必填 + null）。 */
function emptyTargetDetail(type: string, nodeId: string | null): StarMapTargetDetail {
  return {
    type: type,
    nodeId: nodeId,
    anchorId: null,
    projectId: null,
    volumeId: null,
    chapterId: null,
    rangeStart: null,
    rangeEnd: null,
    entityType: null,
    entityId: null,
    uri: null
  }
}

/**
 * 构造 item 相对某个宿主 Scene 的 StarMapTargetPath。
 *
 * Core 的语义（starmap/types/reference.rs）：`starmapId` 是"这条引用从哪张图出发"，
 * `segments` 是从这张图出发逐层穿越的步骤，`target` 是最终落点。
 * 所以宿主选最近公共祖先、segments 只带宿主之后的那几段，Core 就能自己走完。
 *
 * @param hostStarmapId 宿主 Scene 的星图 ID
 * @param hostScenePath 宿主 Scene 的绝对路径
 * @param itemRef 目标 item
 * @returns 相对宿主的 StarMapTargetPath
 */
export function buildTargetPathForHost(
  hostStarmapId: string,
  hostScenePath: StarMapPathSegment[],
  itemRef: StarMapItemRef
): StarMapTargetPath {
  const segments: StarMapPathSegment[] = itemRef.scenePath.slice(hostScenePath.length)
    .map((seg: StarMapPathSegment): StarMapPathSegment => ({
      type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId
    }))
  if (itemRef.kind === 'embed') {
    segments.push(embedSegment(itemRef.itemId))
    return {
      starmapId: hostStarmapId,
      segments: segments,
      target: emptyTargetDetail('starmap', null)
    }
  }
  return {
    starmapId: hostStarmapId,
    segments: segments,
    target: emptyTargetDetail('node', itemRef.itemId)
  }
}

/** 两条端点路径是否描述同一个对象（用于去掉自环）。 */
export function isSameTargetPath(a: StarMapTargetPath, b: StarMapTargetPath): boolean {
  if (a.starmapId !== b.starmapId) {
    return false
  }
  if (!isSameScenePath(a.segments, b.segments)) {
    return false
  }
  return a.target.type === b.target.type && a.target.nodeId === b.target.nodeId
}

/** 端点路径的可读描述（日志用）。 */
export function describeTargetPath(path: StarMapTargetPath): string {
  return `${describeScenePath(path.segments)}#${path.target.type}:${path.target.nodeId ?? ''}`
}

// ─── 递归命中 ───

/**
 * 把 SceneSource 树展开成递归上下文树。
 *
 * 只在 gesture 边界（pinch 开始、connect 松手、选择）调用一次，
 * 不在渲染帧里调用，所以每次重建整棵树的成本可以接受。
 *
 * @param root 根 SceneSource（来自 StarMapSceneRegistry）
 * @returns 递归上下文树
 */
export function buildRecursiveSceneContext(root: RecursiveSceneSource): RecursiveSceneContext {
  const children: Map<string, RecursiveSceneContext> = new Map<string, RecursiveSceneContext>()
  const sourceChildren: RecursiveSceneSource[] = root.getChildEmbeds()
  for (const child of sourceChildren) {
    const instanceId: string = describeLastEmbedInstanceId(child.scenePath)
    if (instanceId === '') {
      continue
    }
    children.set(instanceId, buildRecursiveSceneContext(child))
  }
  return {
    scenePath: cloneScenePath(root.scenePath),
    starmapId: root.starmapId,
    rects: root.rects,
    embedInstanceIds: root.embedInstanceIds,
    edges: root.edges,
    scale: root.scale,
    offsetX: root.offsetX,
    offsetY: root.offsetY,
    children: children
  }
}

/**
 * 找出某个 Embed 在本 Scene 画布坐标里的矩形。
 * 找不到（Embed 还没进布局）返回 null，调用方按"圆心在原点"兜底。
 */
function findEmbedRect(context: RecursiveSceneContext, instanceId: string): PlatformNodeRect | null {
  for (const rect of context.rects) {
    if (rect.nodeId === instanceId) {
      return rect
    }
  }
  return null
}

// ─── 视觉焦点候选（#820）───

/**
 * 一颗 Embed 作为视觉焦点候选时，遍历递归树能拿到的原始量。
 *
 * 这里只报**几何事实**（圆心在视口里的相对位置、所属 Scene 的累计比例），
 * 不做判定：判定（覆盖率、中心区域、滞回）全在 StarMapVisualLod 的纯函数里，
 * UI 层不许自己写一套阈值逻辑。
 */
export interface EmbedFocusProbe {
  /** 进入这颗 Embed 之后的 scenePath 描述（也就是"钻进它"会落到哪一层） */
  childScenePath: string
  /** 圆心在视口内的归一化 X（0 = 左边缘，1 = 右边缘） */
  centerRatioX: number
  /** 圆心在视口内的归一化 Y */
  centerRatioY: number
  /** 这颗 Embed 所属 Scene 的累计有效比例（相机 × 所有祖先 local fit） */
  ownerScale: number
}

/**
 * 递归收集所有"当前真的画出来了"的 Embed 作为视觉焦点候选（#820）。
 *
 * 只走已实例化的 Scene 子树：折叠掉的 Embed 根本没有 child Scene 实例，
 * 自然不会进候选（用户看不到里面，也没法"钻进去看"）。
 *
 * @param root 递归 Scene 上下文的根
 * @param viewportWidthVp 根 StarMap 实际可绘制内容区宽度（vp）
 * @param viewportHeightVp 根 StarMap 实际可绘制内容区高度（vp）
 * @returns 候选 Embed 列表（顺序不保证稳定，调用方只按规则挑）
 */
export function collectEmbedFocusProbes(
  root: RecursiveSceneContext,
  viewportWidthVp: number,
  viewportHeightVp: number
): EmbedFocusProbe[] {
  const probes: EmbedFocusProbe[] = []
  if (viewportWidthVp <= 0 || viewportHeightVp <= 0) {
    return probes
  }
  collectEmbedFocusProbesInScene(root, viewportWidthVp, viewportHeightVp, probes)
  return probes
}

/** collectEmbedFocusProbes 的递归实现：先本层 Embed，再下钻子 Scene。 */
function collectEmbedFocusProbesInScene(
  context: RecursiveSceneContext,
  viewportWidthVp: number,
  viewportHeightVp: number,
  probes: EmbedFocusProbe[]
): void {
  for (const rect of context.rects) {
    if (!context.embedInstanceIds.has(rect.nodeId)) {
      continue
    }
    // 画布坐标 → 屏幕坐标：屏幕 = 画布 × 本 Scene 累计比例 + 累计偏移。
    // 折叠态 Embed 的 rect 已经是摘要卡尺寸（见 embedDisplayBoundsForLod），
    // 圆心仍然是同一块几何的中心，所以这里不用再分档。
    const centerScreenX: number = (rect.x + rect.width / 2) * context.scale + context.offsetX
    const centerScreenY: number = (rect.y + rect.height / 2) * context.scale + context.offsetY
    const childSegments: StarMapPathSegment[] = [
      ...cloneScenePath(context.scenePath),
      { type: 'enterEmbed', instanceId: rect.nodeId, nodeId: null }
    ]
    probes.push({
      childScenePath: describeScenePath(childSegments),
      centerRatioX: centerScreenX / viewportWidthVp,
      centerRatioY: centerScreenY / viewportHeightVp,
      ownerScale: context.scale
    })
  }
  for (const child of context.children.values()) {
    collectEmbedFocusProbesInScene(child, viewportWidthVp, viewportHeightVp, probes)
  }
}

/**
 * 父 Scene 画布坐标 → 某个 Embed 内部的子 Scene 局部屏幕坐标。
 *
 * 这一步只回答一个问题：**这个点在 Embed 那个盒子里偏到哪儿了**。
 * 答案完全由父层的显示几何决定——盒子大小是 `DEFAULT_EMBED_DIAMETER * parent.scale`，
 * 盒子左上角是 `embedRect.origin * parent.scale`（见 ui/StarMapScene.getEmbedScreenX），
 * 所以先扣掉 Embed 矩形原点、再乘父层 scale，就是子组件的局部屏幕坐标。
 *
 * **这里绝对不能碰 child.scale / child.offset。** 子视口会在下一步
 * `screenToCanvas(childLocalX, childLocalY, child.scale, child.offset)` 里作用一次。
 * 如果这里提前乘一遍子 scale，下一步立刻除掉，两次正好抵消——子图缩放和拖动
 * 就彻底对命中不可见了，表现为"缩放子图没反应、点子图里的节点没反应"（#816）。
 *
 * @param parent 父 Scene 上下文
 * @param embedInstanceId 该 Embed 的 instanceId
 * @param parentCanvasX 父 Scene 画布坐标 X
 * @param parentCanvasY 父 Scene 画布坐标 Y
 * @returns 子 Scene 局部屏幕坐标（未套用子视口）
 */
export function embedParentCanvasToChildLocal(
  parent: RecursiveSceneContext,
  embedInstanceId: string,
  parentCanvasX: number,
  parentCanvasY: number
): PlatformPoint {
  const rect: PlatformNodeRect | null = findEmbedRect(parent, embedInstanceId)
  const originX: number = rect !== null ? rect.x : 0
  const originY: number = rect !== null ? rect.y : 0
  return {
    x: (parentCanvasX - originX) * parent.scale,
    y: (parentCanvasY - originY) * parent.scale
  }
}

/**
 * 某个 Embed 内部子 Scene 的局部屏幕坐标 → 父 Scene 画布坐标。
 * embedParentCanvasToChildLocal 的逆运算，同样只看父层显示几何。
 */
export function childLocalToParentCanvas(
  parent: RecursiveSceneContext,
  embedInstanceId: string,
  childLocalX: number,
  childLocalY: number
): PlatformPoint {
  const rect: PlatformNodeRect | null = findEmbedRect(parent, embedInstanceId)
  const originX: number = rect !== null ? rect.x : 0
  const originY: number = rect !== null ? rect.y : 0
  const scale: number = parent.scale > 0 ? parent.scale : 1
  return { x: childLocalX / scale + originX, y: childLocalY / scale + originY }
}

/** 取路径最后一段 enterEmbed 的 instanceId；不是 enterEmbed 或为空时返回 ''。 */
function describeLastEmbedInstanceId(path: StarMapPathSegment[]): string {
  if (path.length === 0) {
    return ''
  }
  const last: StarMapPathSegment = path[path.length - 1]
  if (last.type !== 'enterEmbed' || last.instanceId === null) {
    return ''
  }
  return last.instanceId
}

/** 按绝对路径在上下文树里找 Scene；找不到返回 null。 */
export function findSceneContext(
  root: RecursiveSceneContext,
  scenePath: StarMapPathSegment[]
): RecursiveSceneContext | null {
  if (scenePath.length === 0) {
    return root
  }
  const seg: StarMapPathSegment = scenePath[0]
  if (seg.type !== 'enterEmbed' || seg.instanceId === null) {
    return null
  }
  const child: RecursiveSceneContext | undefined = root.children.get(seg.instanceId)
  if (child === undefined) {
    return null
  }
  return findSceneContext(child, scenePath.slice(1))
}

/**
 * 递归命中：从根 Scene 一路向下解析到最深的真实目标。
 *
 * 终止规则（#816）：
 * - 命中普通节点 → 停在当前层，目标是这个节点，归属也是当前层。
 * - 命中 Embed 标题 / 边框 → 停在当前层，目标是这个 Embed，归属也是当前层
 *   （Embed 本身属于父层，点它就是"选中这个子星图"，不该算进子图）。
 * - 命中 Embed 内部且子图已加载 → 把点换算到子 Scene 局部坐标继续递归，
 *   归属跟着下沉，直到命中真实目标或子图空白。
 * - 命中 Embed 内部但子图还没加载 → 停在当前层，目标是这个 Embed。
 * - 当前层空白 → 停在当前层，target 为 null。
 *
 * 选中、connect 松手、菜单归属、双指归属全部走这一个入口，
 * 不允许再各写一份命中判断（#816）。
 *
 * @param root 根 Scene 上下文
 * @param x 根 Scene 局部屏幕坐标 X
 * @param y 根 Scene 局部屏幕坐标 Y
 * @returns 递归命中结果（永远非 null，空白时 target 为 null）
 */
/**
 * 边的命中容差，单位是**画布坐标**（调用方按本层 scale 换算，保证屏幕上粗细一致）。
 */
export const EDGE_HIT_TOLERANCE_VP = 10

/**
 * 在一层 Scene 的边里做命中，返回距离命中点最近的那条。
 *
 * 复用现成的 pointToSegmentDistance，不另写一套几何。同一距离上有两条边时取
 * 先遍历到的那条，避免"重叠的边互相抢点击"这种不稳定行为。
 *
 * 边的命中优先级**低于**节点 / Embed：resolveRecursiveHit 先测对象，本层一个都没
 * 命中时才会走到这里。否则一条端点贴着节点的边会把节点的点击抢走。
 *
 * @param tolerance 画布坐标下的容差；不是 vp，调用方负责除以本层 scale。
 */
export function hitTestEdge(
  edges: PlatformEdgeRender[],
  x: number,
  y: number,
  tolerance: number
): PlatformEdgeRender | null {
  let best: PlatformEdgeRender | null = null
  let bestDistance: number = tolerance
  for (const edge of edges) {
    const distance: number = pointToSegmentDistance(
      x, y, edge.startX, edge.startY, edge.endX, edge.endY
    )
    // 严格小于：完全同距离时保留先遍历到的那条，和上面写的注释一致。
    // 写成 <= 会让后面那条直接覆盖掉先遍历到的，实际效果和注释正好相反
    //（#816 复审第 8 条）。
    if (distance < bestDistance) {
      bestDistance = distance
      best = edge
    }
  }
  return best
}

export function resolveRecursiveHit(
  root: RecursiveSceneContext,
  x: number,
  y: number
): RecursiveHit {
  let current: RecursiveSceneContext = root
  let localX: number = x
  let localY: number = y
  const embedPath: StarMapPathSegment[] = []

  // 递归深度上限：Core 侧 resolve_target 也是 32，这里多给一层余量后硬性兜底，
  // 避免病态数据（自引用 Embed）把栈打爆。
  for (let depth = 0; depth <= MAX_RECURSE_SCENE_DEPTH; depth++) {
    const canvasPoint: PlatformPoint = screenToCanvas(
      localX, localY, current.scale, current.offsetX, current.offsetY
    )
    // sceneScale 传当前层的累计比例：热区 24 / 12 是屏幕侧规格，
    // 不按本层比例换算的话，子子星图的可点标题 / 圆环会缩成几 vp（#818 复审）。
    const hit: HitTestResult | null = hitTestWithScene(
      current.rects, canvasPoint.x, canvasPoint.y, current.scenePath, current.embedInstanceIds,
      current.scale
    )

    if (hit === null) {
      // 本层没有任何对象命中，才轮到边。顺序不能反：端点贴着节点的边如果先测，
      // 就会把节点的点击抢走（#816）。
      // 容差按本层 scale 换算，缩放得再小边也还是屏幕上那么粗的一根，好点。
      const edgeTolerance: number = current.scale > 0 ? EDGE_HIT_TOLERANCE_VP / current.scale : EDGE_HIT_TOLERANCE_VP
      const edge: PlatformEdgeRender | null = hitTestEdge(
        current.edges, canvasPoint.x, canvasPoint.y, edgeTolerance
      )
      if (edge !== null) {
        return {
          ownerScenePath: cloneScenePath(current.scenePath),
          ownerStarmapId: current.starmapId,
          target: {
            scenePath: cloneScenePath(current.scenePath),
            starmapId: current.starmapId,
            objectKind: 'edge',
            objectId: edge.edgeId,
            hitRegion: 'body'
          },
          embedPath: cloneScenePath(embedPath)
        }
      }
      return {
        ownerScenePath: cloneScenePath(current.scenePath),
        ownerStarmapId: current.starmapId,
        target: null,
        embedPath: cloneScenePath(embedPath)
      }
    }

    if (hit.objectKind !== 'embedInnerContent') {
      // 普通节点 / Embed 标题 / Embed 边框：本层就是终点
      const kind: RecursiveHitKind =
        hit.objectKind === 'embedTitle' ? 'embed' :
          (hit.objectKind === 'embedBorder' ? 'embed' : 'node')
      return {
        ownerScenePath: cloneScenePath(current.scenePath),
        ownerStarmapId: current.starmapId,
        target: {
          scenePath: cloneScenePath(current.scenePath),
          starmapId: current.starmapId,
          objectKind: kind,
          objectId: hit.objectId,
          hitRegion: hit.hitRegion
        },
        embedPath: cloneScenePath(embedPath)
      }
    }

    // Embed 内部：换算到子 Scene 局部坐标继续下探
    const child: RecursiveSceneContext | undefined = current.children.get(hit.objectId)
    if (child === undefined) {
      return {
        ownerScenePath: cloneScenePath(current.scenePath),
        ownerStarmapId: current.starmapId,
        target: {
          scenePath: cloneScenePath(current.scenePath),
          starmapId: current.starmapId,
          objectKind: 'embed',
          objectId: hit.objectId,
          hitRegion: 'innerContent'
        },
        embedPath: cloneScenePath(embedPath)
      }
    }

    // 下探：只换到子组件的局部屏幕坐标；下一轮循环开头的 screenToCanvas 才套子视口，
    // 两者必须分开，否则子图缩放/拖动会被原地抵消，对命中彻底不可见。
    const childLocal: PlatformPoint = embedParentCanvasToChildLocal(
      current, hit.objectId, canvasPoint.x, canvasPoint.y
    )
    embedPath.push(embedSegment(hit.objectId))
    current = child
    localX = childLocal.x
    localY = childLocal.y
  }

  // 深度兜底：返回最深一层的归属，不做命中
  return {
    ownerScenePath: cloneScenePath(current.scenePath),
    ownerStarmapId: current.starmapId,
    target: null,
    embedPath: cloneScenePath(embedPath)
  }
}

/**
 * 把某 Scene 局部坐标里的点换算到根 Scene 局部坐标。
 * 路径在上下文树里找不到时按最深匹配处理（走到哪层算哪层），不抛错。
 *
 * @param root 根 Scene 上下文
 * @param scenePath 点所在的 Scene 绝对路径
 * @param x Scene 局部屏幕坐标 X
 * @param y Scene 局部屏幕坐标 Y
 * @returns 根 Scene 局部屏幕坐标
 */
export function convertPointToRoot(
  root: RecursiveSceneContext,
  scenePath: StarMapPathSegment[],
  x: number,
  y: number
): PlatformPoint {
  let localX: number = x
  let localY: number = y
  // 从最深的一段往外走：点本来就在最深 Scene 的局部坐标里，
  // 必须先退出最深那层，再退出它的父层——顺序反了就会拿错层的视口去套坐标。
  for (let index = scenePath.length - 1; index >= 0; index--) {
    const seg: StarMapPathSegment = scenePath[index]
    if (seg.type !== 'enterEmbed' || seg.instanceId === null) {
      break
    }
    const parent: RecursiveSceneContext | null =
      index === 0 ? root : findSceneContext(root, scenePath.slice(0, index))
    if (parent === null) {
      break
    }
    const child: RecursiveSceneContext | undefined = parent.children.get(seg.instanceId)
    if (child === undefined) {
      break
    }
    // 向上走一层：子 Scene 局部屏幕 → 除父 scale 加回 Embed 矩形原点 → 父画布 → 父局部
    const parentCanvas: PlatformPoint = childLocalToParentCanvas(
      parent, seg.instanceId, localX, localY
    )
    const parentLocal: PlatformPoint = canvasToScreen(
      parentCanvas.x, parentCanvas.y, parent.scale, parent.offsetX, parent.offsetY
    )
    localX = parentLocal.x
    localY = parentLocal.y
  }
  return { x: localX, y: localY }
}

/**
 * 把根 Scene 局部坐标里的点换算到某 Scene 的局部坐标（convertPointToRoot 的逆运算）。
 *
 * 双指缩放的锚点必须落在**归属 Scene 的局部坐标**里再用：`屏幕 = 画布 × scale + offset`
 * 这条式子只在 Scene 自己的局部空间成立。归属层在两指落下那一刻定死之后，每一帧都要把
 * 两指中心换算到它自己的局部坐标；父层拿自己的坐标去算子层的锚点，缩放就会朝错的方向跑。
 *
 * @param root 根 Scene 上下文
 * @param scenePath 目标 Scene 的绝对路径
 * @param x 根 Scene 局部屏幕坐标 X
 * @param y 根 Scene 局部屏幕坐标 Y
 * @returns 目标 Scene 局部屏幕坐标；路径走不下去时按已走到的最深层返回
 */
export function resolveSceneLocalPoint(
  root: RecursiveSceneContext,
  scenePath: StarMapPathSegment[],
  x: number,
  y: number
): PlatformPoint {
  let current: RecursiveSceneContext = root
  let localX: number = x
  let localY: number = y
  let index = 0
  while (index < scenePath.length) {
    const seg: StarMapPathSegment = scenePath[index]
    if (seg.type !== 'enterEmbed' || seg.instanceId === null) {
      break
    }
    const child: RecursiveSceneContext | undefined = current.children.get(seg.instanceId)
    if (child === undefined) {
      break
    }
    // 向下走一段：父 Scene 局部 → 父画布 → 扣 Embed 矩形原点并乘父 scale → 子 Scene 局部
    const parentCanvas: PlatformPoint = screenToCanvas(
      localX, localY, current.scale, current.offsetX, current.offsetY
    )
    const childLocal: PlatformPoint = embedParentCanvasToChildLocal(
      current, seg.instanceId, parentCanvas.x, parentCanvas.y
    )
    current = child
    localX = childLocal.x
    localY = childLocal.y
    index++
  }
  return { x: localX, y: localY }
}

/**
 * 解析"相对根星图的端点路径"对应的 item 引用（含容纳它的 Scene 的 starmapId）。
 *
 * @param root 根 Scene 上下文
 * @param path 相对根星图的 StarMapTargetPath
 * @returns item 引用；解析不出来（Portal 段 / Scene 未加载）时返回 null
 */
export function resolveItemRefFromTargetPath(
  root: RecursiveSceneContext,
  path: StarMapTargetPath
): StarMapItemRef | null {
  if (path.segments.length === 0) {
    if (path.target.type === 'node' && path.target.nodeId !== null) {
      return { scenePath: [], starmapId: root.starmapId, kind: 'node', itemId: path.target.nodeId }
    }
    return null
  }
  if (path.target.type === 'node' && path.target.nodeId !== null) {
    // node 端点：segments 全是"走到所在 Scene"的穿越步骤，节点就在末段到达的那张图里
    const holder: RecursiveSceneContext | null = findSceneContext(root, path.segments)
    if (holder === null) {
      return null
    }
    return {
      scenePath: cloneScenePath(path.segments),
      starmapId: holder.starmapId,
      kind: 'node',
      itemId: path.target.nodeId
    }
  }
  // Embed 端点：最后一段就是被引用的那个 Embed 自身，它属于**上一段**到达的 Scene
  const last: StarMapPathSegment = path.segments[path.segments.length - 1]
  if (last.type !== 'enterEmbed' || last.instanceId === null) {
    // enterPortal 不是 Embed 容器，跨层连线暂不支持（Core 侧也只把 enterEmbed 当层级）
    return null
  }
  const containerPath: StarMapPathSegment[] = path.segments.slice(0, path.segments.length - 1)
  const container: RecursiveSceneContext | null = findSceneContext(root, containerPath)
  if (container === null) {
    return null
  }
  return {
    scenePath: cloneScenePath(containerPath),
    starmapId: container.starmapId,
    kind: 'embed',
    itemId: last.instanceId
  }
}

/**
 * 规划一条跨层连线应该落在哪张图上。
 *
 * 规则（#816）：宿主 = 两个端点所在 Scene 的最近公共祖先。
 * 于是这五种情形都自动落到 Core 能接受的形状：
 * - 父 Node → 子 Node：宿主 = 父 Scene，to = [enterEmbed(...)] + node
 * - 子 Node → 父 Node：宿主 = 父 Scene，from = [enterEmbed(...)] + node
 * - 同一 Embed 里的两个子 Node：宿主 = 该 Embed 的 Scene，两端 segments 都为空
 * - 两个不同 Embed 里的 Node：宿主 = 根 Scene，两端各带一段 enterEmbed
 * - 子 Node → 更深一层的子 Embed/Node：宿主 = 两者的最近公共 Embed Scene
 *
 * 同一层的连线走这里得到的结果和原来的 buildItemTargetPath 完全一致（segments 为空 / 一段 enterEmbed）。
 *
 * @param root 根 Scene 上下文
 * @param sourcePath 起点（相对根星图）
 * @param targetPath 终点（相对根星图）
 * @returns 落点方案；自环或解析失败时返回 null
 */
export function planCrossLayerEdge(
  root: RecursiveSceneContext,
  sourcePath: StarMapTargetPath,
  targetPath: StarMapTargetPath
): CrossLayerEdgePlan | null {
  const from: StarMapItemRef | null = resolveItemRefFromTargetPath(root, sourcePath)
  const to: StarMapItemRef | null = resolveItemRefFromTargetPath(root, targetPath)
  if (from === null || to === null) {
    return null
  }
  if (from.kind === to.kind && from.itemId === to.itemId &&
    isSameScenePath(from.scenePath, to.scenePath)) {
    return null
  }
  const hostScenePath: StarMapPathSegment[] = commonScenePathPrefix(from.scenePath, to.scenePath)
  const host: RecursiveSceneContext | null = findSceneContext(root, hostScenePath)
  if (host === null) {
    return null
  }
  const hostStarmapId: string = host.starmapId
  const planFrom: StarMapTargetPath = buildTargetPathForHost(hostStarmapId, hostScenePath, from)
  const planTo: StarMapTargetPath = buildTargetPathForHost(hostStarmapId, hostScenePath, to)
  if (isSameTargetPath(planFrom, planTo)) {
    return null
  }
  return {
    hostScenePath: hostScenePath,
    hostStarmapId: hostStarmapId,
    from: planFrom,
    to: planTo
  }
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
