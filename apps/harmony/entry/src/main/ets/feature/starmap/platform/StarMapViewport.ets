// ─── StarMapViewport.ets ───
// 星图视口计算（Issue #816）：内容包围盒、初始适配比例、最终比例、围绕锚点缩放的偏移、
// Scene 局部坐标 / 画布坐标 / 父 Scene 坐标互转，以及递归 Scene 的注册表。
//
// 这一层是纯计算，不依赖 ArkUI 组件，不依赖 corebridge，所有函数都能在 node 里直接跑。
//
// 比例的语义（#816 明确要求）：
//   最终比例 = fitScale × userZoomScale
// - fitScale 由"内容包围盒"和"父 Embed 的可用圆形内容区"算出来，只算一次（首次加载 / 显式 fit view）；
// - userZoomScale 是用户自己双指缩放出来的倍数，默认 1，独立于 fitScale 保存；
// - 任何一层都不允许用"每深一层乘 0.6"这种写死系数，那会让嵌套子图越深越小到看不见，
//   也无法解释用户双指放大后为什么还是比父图大。

import { StarMapTargetPath, StarMapPathSegment } from '../../../corebridge/dto/StarMapDtos'
import { PlatformPoint } from './StarMapLayout'
import {
  PlatformNodeRect,
  PlatformEdgeRender,
  RecursiveSceneSource,
  describeScenePath
} from './StarMapGeometry'

// ─── 内容包围盒 ───

/** 一组矩形在画布坐标下的并集包围盒。 */
export interface ContentBounds {
  minX: number
  minY: number
  maxX: number
  maxY: number
  width: number
  height: number
}

/**
 * 计算一组矩形的并集包围盒。
 * 空列表返回 null（没有内容可适配），调用方据此退回"不缩放"。
 */
export function computeContentBounds(rects: PlatformNodeRect[]): ContentBounds | null {
  if (rects.length === 0) {
    return null
  }
  let minX: number = rects[0].x
  let minY: number = rects[0].y
  let maxX: number = rects[0].x + rects[0].width
  let maxY: number = rects[0].y + rects[0].height
  for (let i = 1; i < rects.length; i++) {
    const r: PlatformNodeRect = rects[i]
    if (r.x < minX) { minX = r.x }
    if (r.y < minY) { minY = r.y }
    if (r.x + r.width > maxX) { maxX = r.x + r.width }
    if (r.y + r.height > maxY) { maxY = r.y + r.height }
  }
  return { minX, minY, maxX, maxY, width: maxX - minX, height: maxY - minY }
}

// ─── 比例与偏移 ───

/** 一个 Scene 的视口参数。 */
export interface Viewport {
  /** 有效比例（fitScale × userZoomScale） */
  zoomScale: number
  offsetX: number
  offsetY: number
}

/**
 * 圆形 Embed 内部"手能可靠点到"的内接正方形比例。
 *
 * Embed 的内容区是一个正圆，但子 Scene 要铺满一个正方形布局，
 * 只有内接正方形（1/√2）的那部分完全在圆内；四角超出圆外的区域虽然画出来了、
 * 但超出部分在父层会被 clip 裁掉（#813 已定的圆形热区），所以适配时按内接正方形算，
 * 不会出现"子图内容铺到圆外被裁掉"。
 */
export const CIRCLE_INNER_SAFE_RATIO: number = 1 / Math.SQRT2

/** fitScale 的下限。再小就完全看不见了，保留一点下限免得内容糊成一点。 */
export const MIN_FIT_SCALE: number = 0.02

/** fitScale 的上限。子星图内容少的时候不该被放大到糊掉。 */
export const MAX_FIT_SCALE: number = 4

/**
 * 按可用区域计算适配比例。
 *
 * @param bounds 内容包围盒（画布坐标）
 * @param availableWidth 可用宽度（vp）
 * @param availableHeight 可用高度（vp）
 * @param paddingVp 四边留白（vp）
 * @returns 适配比例，已夹在 [MIN_FIT_SCALE, MAX_FIT_SCALE]
 */
export function computeFitScale(
  bounds: ContentBounds,
  availableWidth: number,
  availableHeight: number,
  paddingVp: number
): number {
  const usableWidth: number = Math.max(1, availableWidth - paddingVp * 2)
  const usableHeight: number = Math.max(1, availableHeight - paddingVp * 2)
  const raw: number = Math.min(usableWidth / bounds.width, usableHeight / bounds.height)
  if (!isFinite(raw) || raw <= 0) {
    return MIN_FIT_SCALE
  }
  return Math.max(MIN_FIT_SCALE, Math.min(MAX_FIT_SCALE, raw))
}

/**
 * 把内容居中到 Scene 的真实中心。
 *
 * 注意：缩放比例按"可用区"算（子星图里是圆的内接正方形），但偏移必须按
 * Scene 自己的尺寸算。子 Scene 组件铺满整个 Embed 圆，组件中心是
 * sceneWidth/2、sceneHeight/2；如果拿内接正方形的中心当锚，内容会整体偏左上
 * （直径 200 的圆，内接正方形只有约 141，内容会落在 ~70 而不是 100）。
 *
 * @param bounds 内容包围盒（画布坐标）
 * @param fitScale 已算好的缩放比例
 * @param sceneWidth Scene 组件真实宽度（vp）
 * @param sceneHeight Scene 组件真实高度（vp）
 * @returns 让内容中心与 Scene 中心重合的偏移
 */
export function computeCenteredOffset(
  bounds: ContentBounds,
  fitScale: number,
  sceneWidth: number,
  sceneHeight: number
): PlatformPoint {
  const centerX: number = (bounds.minX + bounds.maxX) / 2
  const centerY: number = (bounds.minY + bounds.maxY) / 2
  return {
    x: sceneWidth / 2 - centerX * fitScale,
    y: sceneHeight / 2 - centerY * fitScale
  }
}

/**
 * 计算"内容居中铺满可用区域"的初始视口。
 *
 * 缩放按可用区算，偏移按 Scene 真实中心算，两者分工见 computeCenteredOffset。
 *
 * @param bounds 内容包围盒（画布坐标）
 * @param availableWidth 可用宽度（vp），只参与 fitScale 计算
 * @param availableHeight 可用高度（vp），只参与 fitScale 计算
 * @param paddingVp 四边留白（vp）
 * @param sceneWidth Scene 组件真实宽度（vp），用于居中
 * @param sceneHeight Scene 组件真实高度（vp），用于居中
 * @returns 视口（zoomScale 即 fitScale）
 */
export function computeFittedViewport(
  bounds: ContentBounds,
  availableWidth: number,
  availableHeight: number,
  paddingVp: number,
  sceneWidth: number,
  sceneHeight: number
): Viewport {
  const fitScale: number = computeFitScale(bounds, availableWidth, availableHeight, paddingVp)
  const offset: PlatformPoint = computeCenteredOffset(bounds, fitScale, sceneWidth, sceneHeight)
  return {
    zoomScale: fitScale,
    offsetX: offset.x,
    offsetY: offset.y
  }
}

/** 最终比例 = fitScale × userZoomScale。 */
export function effectiveScale(fitScale: number, userZoomScale: number): number {
  return fitScale * userZoomScale
}

/** 把用户缩放倍数夹在合法区间内。 */
export function clampUserZoom(userZoomScale: number, minZoom: number, maxZoom: number): number {
  if (!isFinite(userZoomScale) || userZoomScale <= 0) {
    return 1
  }
  return Math.max(minZoom, Math.min(maxZoom, userZoomScale))
}

/**
 * 围绕屏幕锚点缩放时的偏移。
 *
 * 锚点画布坐标 anchorCanvas 固定，屏幕锚点 anchorScreen 固定，
 * 缩到 nextScale 后要让 anchorCanvas 仍落在 anchorScreen 上：
 *   offset = anchorScreen - anchorCanvas * nextScale
 *
 * @param anchorCanvas 锚点在**该 Scene 画布坐标**里的位置
 * @param anchorScreen 锚点在**该 Scene 局部屏幕坐标**里的位置
 * @param nextScale 缩放后的有效比例
 * @returns 新的 Scene 局部偏移
 */
export function computeZoomAroundOffset(
  anchorCanvas: PlatformPoint,
  anchorScreen: PlatformPoint,
  nextScale: number
): PlatformPoint {
  return {
    x: anchorScreen.x - anchorCanvas.x * nextScale,
    y: anchorScreen.y - anchorCanvas.y * nextScale
  }
}

// ─── 坐标互转 ───

/**
 * Scene 局部屏幕坐标 → 本 Scene 画布坐标。
 * 屏幕 = 画布 * scale + offset，所以反过来是 (屏幕 - offset) / scale。
 */
export function sceneLocalToCanvas(
  x: number,
  y: number,
  scale: number,
  offsetX: number,
  offsetY: number
): PlatformPoint {
  if (scale === 0) {
    return { x: 0, y: 0 }
  }
  return { x: (x - offsetX) / scale, y: (y - offsetY) / scale }
}

/** 本 Scene 画布坐标 → Scene 局部屏幕坐标。 */
export function canvasToSceneLocal(
  x: number,
  y: number,
  scale: number,
  offsetX: number,
  offsetY: number
): PlatformPoint {
  return { x: x * scale + offsetX, y: y * scale + offsetY }
}

// ─── 递归 Scene 注册表 ───

/**
 * 一层 Scene 在注册表里的记录。
 *
 * 几何字段（rects / scale / offset / fitScale）是递归命中要读的快照，
 * 由 Scene 在重算几何和视口变化后 sync 一次；命令方法让"统一手势入口"
 * 可以隔着层级调用最深那一层：双指缩放只能改归属 Scene 的视口，
 * 连线只能落在宿主 Scene 上，靠的就是这几条命令而不是"父层顺手改一改"（#816）。
 *
 * 构造函数直接收函数而不是收一个对象：ArkTS 不允许在结构体里就地构造
 * 带函数字段的对象字面量，函数参数就没有这个限制。
 */
export class StarMapSceneNode implements RecursiveSceneSource {
  scenePath: StarMapPathSegment[] = []
  starmapId: string = ''
  rects: PlatformNodeRect[] = []
  embedInstanceIds: Set<string> = new Set<string>()
  /** 本层画布坐标下的全部边。递归命中要靠它才能返回 edge 这一类目标（#816）。 */
  edges: PlatformEdgeRender[] = []
  scale: number = 1
  offsetX: number = 0
  offsetY: number = 0
  /** 适配比例。最终比例 = fitScale × userZoomScale。 */
  fitScale: number = 1

  private getChildren: () => RecursiveSceneSource[]
  private zoomToScaleFn: (
    anchorCanvasX: number, anchorCanvasY: number,
    anchorLocalX: number, anchorLocalY: number, requestedScale: number
  ) => void
  private applyZoomFn: (userScale: number, offsetX: number, offsetY: number) => void
  private fitViewFn: () => void
  private createEdgeFn: (from: StarMapTargetPath, to: StarMapTargetPath) => Promise<boolean>
  private redrawEdgesFn: () => void

  constructor(
    scenePath: StarMapPathSegment[],
    starmapId: string,
    getChildren: () => RecursiveSceneSource[],
    zoomToScaleFn: (
      anchorCanvasX: number, anchorCanvasY: number,
      anchorLocalX: number, anchorLocalY: number, requestedScale: number
    ) => void,
    applyZoomFn: (userScale: number, offsetX: number, offsetY: number) => void,
    fitViewFn: () => void,
    createEdgeFn: (from: StarMapTargetPath, to: StarMapTargetPath) => Promise<boolean>,
    redrawEdgesFn: () => void
  ) {
    this.scenePath = scenePath
    this.starmapId = starmapId
    this.getChildren = getChildren
    this.zoomToScaleFn = zoomToScaleFn
    this.applyZoomFn = applyZoomFn
    this.fitViewFn = fitViewFn
    this.createEdgeFn = createEdgeFn
    this.redrawEdgesFn = redrawEdgesFn
  }

  /**
   * 让本层重画一遍边的 Canvas。
   *
   * 选中是整棵树共享的一份：选中子层的边时，别的层也要把上一条高亮擦掉。
   * ArkUI 的 Canvas onReady 只在首次准备好时触发一次，外部状态变化后不会自动
   * 重放之前的绘图命令，所以画边的高亮必须由 Scene 自己在状态变化时重画。
   */
  redrawEdges(): void {
    this.redrawEdgesFn()
  }

  /** 把最新的布局与视口快照到记录上。 */
  sync(
    rects: PlatformNodeRect[],
    embedInstanceIds: Set<string>,
    edges: PlatformEdgeRender[],
    scale: number,
    offsetX: number,
    offsetY: number,
    fitScale: number
  ): void {
    this.rects = rects
    this.embedInstanceIds = embedInstanceIds
    this.edges = edges
    this.scale = scale
    this.offsetX = offsetX
    this.offsetY = offsetY
    this.fitScale = fitScale
  }

  getChildEmbeds(): RecursiveSceneSource[] {
    return this.getChildren()
  }

  /**
   * 以锚点为中心，缩到**绝对**总比例 requestedScale。
   *
   * 这里收绝对值而不是"相对上一帧放大几倍"：统一入口每帧都会发一次命令，
   * 收相对量的话每一帧都拿上一帧的结果再乘一次，两指 100 → 110 → 120
   * 就会变成 1.00×1.1×1.2 = 1.32，而不是 1.20。手势起手时是什么比例、
   * 现在两指捏到几倍，由统一入口算好后传进来。
   *
   * 锚点同时给"画布坐标"和"局部屏幕坐标"，因为 `屏幕 = 画布 × scale + offset`
   * 只在 Scene 自己的局部空间成立：统一入口给的两指中心是根坐标，必须先换算到本层，
   * 画布锚点也必须留在本层坐标系里。夹取范围、最终比例怎么由 fitScale 合成，
   * 都由 Scene 自己算，父层只发命令。
   */
  zoomToScale(
    anchorCanvasX: number,
    anchorCanvasY: number,
    anchorLocalX: number,
    anchorLocalY: number,
    requestedScale: number
  ): void {
    this.zoomToScaleFn(anchorCanvasX, anchorCanvasY, anchorLocalX, anchorLocalY, requestedScale)
  }

  applyUserZoom(userScale: number, offsetX: number, offsetY: number): void {
    this.applyZoomFn(userScale, offsetX, offsetY)
  }

  fitView(): void {
    this.fitViewFn()
  }

  /**
   * 让宿主 Scene 落一条边，返回 Core 是否真的收下了。
   *
   * 必须回传结果：connect 的诊断要能区分"递归命中没找到目标"和
   * "路径算出来了但 Core 拒绝"。返回 void 的话 connect_end 只会在取消路径上被写到，
   * 真机上的成功/失败反而没有结构化记录。
   */
  async createEdgeBetween(from: StarMapTargetPath, to: StarMapTargetPath): Promise<boolean> {
    return this.createEdgeFn(from, to)
  }
}

/**
 * 递归 Scene 注册表：以 describeScenePath 为主键，记录当前挂载的每一层 Scene。
 *
 * 存在的理由：双指到底属于哪一层、connect 松手落在谁身上、跨层连线宿主是谁，
 * 这三个判断都需要"从根往下看整棵树"，而这些判断发生在根 Scene 的手势入口里，
 * 根 Scene 拿不到子组件实例。注册表就是这层间接访问的唯一通道。
 *
 * StarMapScreen 持有唯一一份，沿组件树一路传下去；每层 StarMapScene 在
 * aboutToAppear 注册、aboutToDisappear 注销，句柄上的几何字段由 Scene 自己同步。
 */
export class StarMapSceneRegistry {
  private handles: Map<string, StarMapSceneNode> = new Map<string, StarMapSceneNode>()

  /** 注册一层 Scene。同一路径重复注册以最后一次为准。 */
  register(handle: StarMapSceneNode): void {
    this.handles.set(describeScenePath(handle.scenePath), handle)
  }

  /** 注销一层 Scene。 */
  unregister(scenePath: StarMapPathSegment[]): void {
    this.handles.delete(describeScenePath(scenePath))
  }

  /** 按绝对路径查 Scene 句柄；没挂载或没加载完返回 null。 */
  find(scenePath: StarMapPathSegment[]): StarMapSceneNode | null {
    const handle: StarMapSceneNode | undefined = this.handles.get(describeScenePath(scenePath))
    return handle !== undefined ? handle : null
  }

  /** 取根 Scene 句柄。 */
  getRoot(): StarMapSceneNode | null {
    return this.find([])
  }

  /**
   * 取某个 Embed 下面的子 Scene 句柄。
   * 子图还没加载完时没有注册，返回 null（此时点 Embed 内部按"选中这个子星图"处理）。
   */
  findChildEmbed(parentScenePath: StarMapPathSegment[], embedInstanceId: string): StarMapSceneNode | null {
    const childPath: StarMapPathSegment[] = parentScenePath.concat([{
      type: 'enterEmbed', instanceId: embedInstanceId, nodeId: null
    }])
    return this.find(childPath)
  }

  /** 当前已注册的 Scene 数量（诊断和测试用）。 */
  size(): number {
    return this.handles.size
  }

  /**
   * 让**所有**已注册的 Scene 重画边的 Canvas。
   *
   * 共享选中是整棵树唯一一份，选中态一变，每一层的边都可能要改：选中子层的边时
   * 兄弟层的旧高亮要擦掉，点根层节点时子层那条高亮也要消失。所以只重画当前这层
   * 是不够的（#816 复审第 8 条）。
   */
  redrawAllEdges(): void {
    this.handles.forEach((handle: StarMapSceneNode) => {
      handle.redrawEdges()
    })
  }
}
