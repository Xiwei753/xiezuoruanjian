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
import { PlatformNodeRect, RecursiveSceneSource, describeScenePath } from './StarMapGeometry'

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
 * 计算"内容居中铺满可用区域"的初始视口。
 *
 * 偏移让包围盒中心落在可用区域中心，所以子图一打开就在圆里居中，
 * 不需要用户再手动平移才能看到。
 *
 * @param bounds 内容包围盒（画布坐标）
 * @param availableWidth 可用宽度（vp）
 * @param availableHeight 可用高度（vp）
 * @param paddingVp 四边留白（vp）
 * @returns 视口（zoomScale 即 fitScale）
 */
export function computeFittedViewport(
  bounds: ContentBounds,
  availableWidth: number,
  availableHeight: number,
  paddingVp: number
): Viewport {
  const fitScale: number = computeFitScale(bounds, availableWidth, availableHeight, paddingVp)
  return {
    zoomScale: fitScale,
    offsetX: (availableWidth - bounds.width * fitScale) / 2 - bounds.minX * fitScale,
    offsetY: (availableHeight - bounds.height * fitScale) / 2 - bounds.minY * fitScale
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
  scale: number = 1
  offsetX: number = 0
  offsetY: number = 0
  /** 适配比例。最终比例 = fitScale × userZoomScale。 */
  fitScale: number = 1

  private getChildren: () => RecursiveSceneSource[]
  private zoomFn: (
    anchorCanvasX: number, anchorCanvasY: number,
    anchorLocalX: number, anchorLocalY: number, ratio: number
  ) => void
  private applyZoomFn: (userScale: number, offsetX: number, offsetY: number) => void
  private fitViewFn: () => void
  private createEdgeFn: (from: StarMapTargetPath, to: StarMapTargetPath) => void

  constructor(
    scenePath: StarMapPathSegment[],
    starmapId: string,
    getChildren: () => RecursiveSceneSource[],
    zoomFn: (
      anchorCanvasX: number, anchorCanvasY: number,
      anchorLocalX: number, anchorLocalY: number, ratio: number
    ) => void,
    applyZoomFn: (userScale: number, offsetX: number, offsetY: number) => void,
    fitViewFn: () => void,
    createEdgeFn: (from: StarMapTargetPath, to: StarMapTargetPath) => void
  ) {
    this.scenePath = scenePath
    this.starmapId = starmapId
    this.getChildren = getChildren
    this.zoomFn = zoomFn
    this.applyZoomFn = applyZoomFn
    this.fitViewFn = fitViewFn
    this.createEdgeFn = createEdgeFn
  }

  /** 把最新的布局与视口快照到记录上。 */
  sync(
    rects: PlatformNodeRect[],
    embedInstanceIds: Set<string>,
    scale: number,
    offsetX: number,
    offsetY: number,
    fitScale: number
  ): void {
    this.rects = rects
    this.embedInstanceIds = embedInstanceIds
    this.scale = scale
    this.offsetX = offsetX
    this.offsetY = offsetY
    this.fitScale = fitScale
  }

  getChildEmbeds(): RecursiveSceneSource[] {
    return this.getChildren()
  }

  /**
   * 以锚点为中心缩放。
   *
   * 锚点同时给"画布坐标"和"局部屏幕坐标"，因为 `屏幕 = 画布 × scale + offset`
   * 只在 Scene 自己的局部空间成立：统一入口给的两指中心是根坐标，必须先换算到本层，
   * 画布锚点也必须留在本层坐标系里。夹取范围、最终比例怎么由 fitScale 合成，
   * 都由 Scene 自己算，父层只发命令。
   */
  zoomAround(
    anchorCanvasX: number,
    anchorCanvasY: number,
    anchorLocalX: number,
    anchorLocalY: number,
    ratio: number
  ): void {
    this.zoomFn(anchorCanvasX, anchorCanvasY, anchorLocalX, anchorLocalY, ratio)
  }

  applyUserZoom(userScale: number, offsetX: number, offsetY: number): void {
    this.applyZoomFn(userScale, offsetX, offsetY)
  }

  fitView(): void {
    this.fitViewFn()
  }

  createEdgeBetween(from: StarMapTargetPath, to: StarMapTargetPath): void {
    this.createEdgeFn(from, to)
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
}
