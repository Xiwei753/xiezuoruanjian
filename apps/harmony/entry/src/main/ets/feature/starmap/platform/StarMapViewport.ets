// ─── StarMapViewport.ets ───
// 星图视口计算（Issue #816）：内容包围盒、初始适配比例、最终比例、围绕锚点缩放的偏移、
// Scene 局部坐标 / 画布坐标 / 父 Scene 坐标互转，以及递归 Scene 的注册表。
//
// 这一层是纯计算，不依赖 ArkUI 组件，不依赖 corebridge，所有函数都能在 node 里直接跑。
//
// 变换只有两种（#818 明确要求，取代 #816 的 fitScale × userZoomScale）：
//
// 1) Global Camera Transform（全局视角相机）
//    整棵递归星图只有**一台**相机：`{ scale, offsetX, offsetY }`，由 StarMapScreen 持有。
//    只有用户手势能改它 —— 画布双指捏合、工具栏的 +/−、空白处拖动。
//    节点、子星图、嵌套子星图都只是这台相机里的内容，不存在"单独缩放某个子星图"。
//    旧方向"双指中心落在哪一层 → 找最深 ownerScenePath → 缩放那个 Scene"已经作废。
//
// 2) Embed Local Fit Transform（子星图局部适配）
//    每个子 Scene 有一份自己的 `fitScale + 居中偏移`，由"内容包围盒"和"父 Embed 的
//    可用圆形内容区"算出。它是**布局适配**，不是相机：用户改不了，也不在相机语义里。
//    每一层都按自己的父 Embed 尺寸各算一次，所以嵌套深度不会让子图越深越小到看不见。
//    这份 fit 是**局部数据**：算的时候要先把父层尺寸除掉祖先累计比例还原成本层原始尺寸，
//    否则同一张子图会因为"打开时相机是多少"而算出不同的 fitScale。
//
// 一张子星图里的点，从局部画布到最终屏幕要依次经过：
//   子 Scene 局部 → 父 Embed 局部适配 → 更上层 Embed 局部适配 → 根 world → 全局相机 → 屏幕
// 子层没有任何"用户缩放"可以混进这条链。
//
// 因此每个 Scene 真正乘在画面上的比例是**累计值**：
//   根：camera
//   子：camera × fit₁
//   孙：camera × fit₁ × fit₂
// Scene 注册表 sync() 写进去的 scale / offset 就是这个累计值（见 ui/StarMapScene
// 的 viewportScaleValue() / viewportOffsetX()），递归命中那套换算才能和画面一致。
//
// 两条由此推出的硬约束（#818 复审）：
//
// 3) 子 Scene 不是独立窗口，没有自己的平移相机。
//    用户在任何一层空白处单指拖动，拖的都是**根的全局相机**（拖整张星图的视角），
//    子 Scene 的 fitOffset 只有 fitView / syncFitToSceneSize 会写。
//
// 4) 子星图里的对象不能被拖到圆外，也盖不住父圆的标题 / 边框。
//    外层 Embed 有 clip(true)，圆外的内容会被直接裁掉，所以"移动节点 / 移动子星图"
//    写入前必须过 clampItemToEmbedSafeArea（复用 CIRCLE_INNER_SAFE_RATIO，不另造常量），
//    并且写进 Core 的坐标就是夹好的那一份，数据里不留圆外位置。
//    这份安全区同时把父 Embed 的标题带与边框热区当"交互壳"扣掉
//    （computeEmbedInnerContentSafeSide），所以子子星图天然比父子星图小，
//    而父圆的"点标题 / 边框选中"入口始终露在外面——不用"每深一层乘固定系数"。
//
// 5) 画布拖动传的是**目标值**，不是每帧增量。
//    ArkUI PanGesture 的 event.offsetX/Y 是相对手势起点的累计位移（华为官方示例也是
//    `offsetX = positionX + event.offsetX`）。所以 Scene 层在起手时记一份基准，
//    更新时算 基准 + 累计位移 得到目标 offset，根整份覆盖相机 offset。
//    写成 `camera += event.offsetX` 会把同一次手势的每一帧都叠一遍，越拖越远。
//
// 6) 新建同样要守边界，而且光 clamp 不够。
//    performCreateNode / performCreateEmbed 写 Core 前也要过安全区 clamp
//    （尺寸取 StarMapLayout 导出的 DEFAULT_NODE_WIDTH/HEIGHT、DEFAULT_EMBED_DIAMETER）。
//    但空子星图首次 fitView() 时没有内容，fitScale 会停在 1 且 hasFittedView 已为 true，
//    之后 maybeFitView 不再跑——新对象可能比当前安全区还大。
//    所以新建成功后还要显式重算一次 local fit：重算的是 local fit，不是 global camera，
//    用户双指缩放好的视角不会被重置。移动已有对象不重算，继续每帧 clamp 即可。

import { StarMapTargetPath, StarMapPathSegment } from '../../../corebridge/dto/StarMapDtos'
import { PlatformPoint } from './StarMapLayout'
import {
  PlatformNodeRect,
  PlatformEdgeRender,
  RecursiveSceneSource,
  describeScenePath,
  EMBED_TITLE_HIT_HEIGHT,
  EMBED_BORDER_HIT_WIDTH
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

/** 一个 Scene 的**局部适配**结果：内容铺进父 Embed 后的比例与偏移（#818 布局用，不是相机）。 */
export interface Viewport {
  zoomScale: number
  offsetX: number
  offsetY: number
}

// ─── 全局视角相机（#818）───

/**
 * 整棵递归星图唯一的相机。
 *
 * 屏幕坐标 = 根画布坐标 × scale + offset。只有用户手势改它。
 */
export interface GlobalCameraTransform {
  scale: number
  offsetX: number
  offsetY: number
}

/** 相机比例下限。再小整棵树就糊成一点。 */
export const CAMERA_SCALE_MIN: number = 0.3

/** 相机比例上限。再大就只能看到局部。 */
export const CAMERA_SCALE_MAX: number = 3

/** 把相机比例夹在合法区间内；非有限值或非正数一律退回 1。 */
export function clampCameraScale(scale: number): number {
  if (!isFinite(scale) || scale <= 0) {
    return 1
  }
  return Math.max(CAMERA_SCALE_MIN, Math.min(CAMERA_SCALE_MAX, scale))
}

/**
 * 双指捏合时相机的新状态（#818 公式）：
 *   anchorWorldX = (centerX - offsetX) / cameraScale
 *   anchorWorldY = (centerY - offsetY) / cameraScale
 *   nextScale    = clamp(baseScale * pinchRatio)
 *   offsetX      = centerX - anchorWorldX * nextScale
 *   offsetY      = centerY - anchorWorldY * nextScale
 *
 * 收 baseScale 而不是上一帧的比例：手势起手时的比例固定下来，两指每一帧都从它算，
 * 否则 100 → 110 → 120 会变成 1.00×1.1×1.2 = 1.32 而不是 1.20。
 *
 * @param camera 手势起手时的相机状态
 * @param baseScale 手势起手时的相机比例
 * @param baseDistance 手势起手时的两指距离
 * @param currentDistance 当前两指距离
 * @param centerX 当前两指中心的屏幕坐标 X
 * @param centerY 当前两指中心的屏幕坐标 Y
 * @returns 新的相机状态
 */
export function computeCameraPinch(
  camera: GlobalCameraTransform,
  baseScale: number,
  baseDistance: number,
  currentDistance: number,
  centerX: number,
  centerY: number
): GlobalCameraTransform {
  const ratio: number = baseDistance > 0 ? currentDistance / baseDistance : 1
  const nextScale: number = clampCameraScale(baseScale * ratio)
  const anchorWorldX: number = (centerX - camera.offsetX) / camera.scale
  const anchorWorldY: number = (centerY - camera.offsetY) / camera.scale
  return {
    scale: nextScale,
    offsetX: centerX - anchorWorldX * nextScale,
    offsetY: centerY - anchorWorldY * nextScale
  }
}

/**
 * 工具栏 +/− 这类"没有起手锚点"的缩放，围绕屏幕中心改相机。
 */
export function computeCameraZoomAround(
  camera: GlobalCameraTransform,
  centerX: number,
  centerY: number,
  nextScale: number
): GlobalCameraTransform {
  const clamped: number = clampCameraScale(nextScale)
  const anchorWorldX: number = (centerX - camera.offsetX) / camera.scale
  const anchorWorldY: number = (centerY - camera.offsetY) / camera.scale
  return {
    scale: clamped,
    offsetX: centerX - anchorWorldX * clamped,
    offsetY: centerY - anchorWorldY * clamped
  }
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

/**
 * 父 Embed 自己必须留出来的**交互壳厚度**（#818 复审）。
 *
 * 子 Scene 的可用区不能只是圆的内接正方形：父 Embed 的顶部标题带
 * （EMBED_TITLE_HIT_HEIGHT）和外圈边框热区（EMBED_BORDER_HIT_WIDTH）
 * 是"选中这个子星图"的入口，内容压上去就等于把入口盖住。
 *
 * #373 的规则是"点文字 / 边框选中子星图，点内部编辑子星图"，所以这块壳
 * 必须始终露在外面、随时可点。取自 StarMapGeometry 的屏幕侧命中规格，
 * **不另造第三套数字**：几何命中和布局预留用的是同一份来源。
 */
export const EMBED_INTERACTION_SHELL_VP: number = EMBED_TITLE_HIT_HEIGHT + EMBED_BORDER_HIT_WIDTH

/**
 * 子 Scene 真正可用的内容区边长：圆的内接正方形先扣掉父 Embed 的交互壳，再扣留白。
 *
 * 这一份同时喂给三处，保证"算出来的"和"允许存在的"永远一致（#818 复审）：
 * - `ui/StarMapScene.fitView()`：算 local fit 时用它当可用区
 * - `clampItemToEmbedSafeArea()`：移动 / 新建时的 clamp 安全区
 * - 新建 Node / Embed 的初始位置约束（走同一个 clamp）
 *
 * 效果上，子子星图会比父子星图自然小一截，因为每一层都要让出自己父圆的标题与边框；
 * 这是几何必然，不是"每深一层乘 0.6"那种拍出来的系数。
 *
 * @param localSceneSize 本层局部尺寸（vp，已扣掉祖先累计比例）
 * @param paddingVp 内容区四边留白（vp）
 * @returns 可用内容区边长（vp），不小于 0
 */
export function computeEmbedInnerContentSafeSide(
  localSceneSize: number,
  paddingVp: number
): number {
  if (!isFinite(localSceneSize) || localSceneSize <= 0) {
    return 0
  }
  const innerSquare: number = localSceneSize * CIRCLE_INNER_SAFE_RATIO
  return Math.max(0, innerSquare - EMBED_INTERACTION_SHELL_VP - Math.max(0, paddingVp) * 2)
}

/**
 * 把一段**局部长度**投影成屏幕侧长度（vp）。
 *
 * 星图里"这个 Embed 有多大"有两套数字混着：局部坐标（200 直径、24 标题带、
 * 160 节点宽……）和屏幕上的实际大小（用户捏合、逐层 local fit 之后的结果）。
 * #820 要求这两者之间只留**这一个**换算口，否则每个地方各乘一次 scale，
 * 早晚会有一处漏乘或多乘，表现为"画的是摘要卡、命中还按圆算"或者
 * "折叠阈值随层级漂移"。
 *
 * 坐标链（规范原话）：
 *   child local → 父 Embed local fit → … → root world → 全局相机 → 屏幕
 * 局部长度要变成屏幕长度，就得乘上**全部**祖先级的累计比例，
 * 也就是该长度所属 Scene 的 effectiveScale：
 *   `globalCameraScale × 所有祖先 EmbedLocalFitScale`
 * 这个值由 `ui/StarMapScene.viewportScaleValue()` 提供（子 Scene 是
 * `fitScale × parentScale()`，根 Scene 是 cameraScale）。
 *
 * 注意：这里**不含**某个 Embed 自己 child Scene 的 local fit。
 * 那是"圆壳里面的内容大小"，不是外壳大小；混进去会让每深一层外壳就小一截，
 * 折叠判定自我实现（越深越折叠）而再也展不开。
 *
 * @param localLength 局部坐标里的长度（vp）
 * @param ownerEffectiveScale 该长度所属 Scene 的累计有效比例
 * @returns 屏幕侧长度（vp）
 */
export function projectLocalLengthToScreen(localLength: number, ownerEffectiveScale: number): number {
  if (!isFinite(localLength)) {
    return 0
  }
  if (!isFinite(ownerEffectiveScale) || ownerEffectiveScale <= 0) {
    return localLength
  }
  return localLength * ownerEffectiveScale
}

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

// ─── 子星图内容安全区（#818 复审）───

/**
 * 把一个画布坐标的 item 夹回父 Embed 圆形内容区的安全区内。
 *
 * 为什么需要：子 Scene 是父 Embed 圆里的一个正方形盒子，外层有 `clip(true)`，
 * 跑出圆外的内容不是"显示小了"，是**直接被裁掉看不见**。真机上表现就是
 * "在子星图里拖节点，节点拖到边上就消失了"。
 * 以前的做法是给 child Scene 留一套自己的平移相机把内容拖回来 —— 那是把
 * 子星图当独立窗口在用，和"子星图 = 父星图里的嵌入区域"矛盾（#818 复审）。
 *
 * 正确做法：child 的 `fitScale / fitOffset` 只由布局算，用户碰不到；
 * 用户显式移动节点 / 移动子星图时，把候选坐标限制在圆的内接正方形里，
 * **写进 Core 之前就夹好**，数据里不留圆外坐标。
 *
 * 坐标口径：全部是**本层局部数据**（即扣掉祖先累计比例之后的那一份，
 * 与 fitView / syncFitToSceneSize 同一口径），所以这里的 scale / offset
 * 就是本层的 fitScale / fitOffsetX / fitOffsetY。
 *
 * @param x item 画布坐标（左上角 X）
 * @param y item 画布坐标（左上角 Y）
 * @param width item 画布宽度
 * @param height item 画布高度
 * @param scale 本层局部适配比例
 * @param offsetX 本层局部适配偏移 X
 * @param offsetY 本层局部适配偏移 Y
 * @param localSceneSize 本层局部尺寸（圆形内容区的边长）
 * @param paddingVp 安全区四边留白（vp），与 fitView 用同一份
 * @returns 夹好之后的画布坐标
 */
export function clampItemToEmbedSafeArea(
  x: number,
  y: number,
  width: number,
  height: number,
  scale: number,
  offsetX: number,
  offsetY: number,
  localSceneSize: number,
  paddingVp: number
): PlatformPoint {
  const fitScale: number = scale > 0 ? scale : 1
  // 尺寸非法（容器还没量出来 / 坐标算坏了）时原样返回，不在这里把数据改成 NaN。
  if (!isFinite(x) || !isFinite(y) || localSceneSize <= 0) {
    return { x, y }
  }
  const itemWidth: number = width * fitScale
  const itemHeight: number = height * fitScale
  if (!isFinite(itemWidth) || !isFinite(itemHeight)) {
    return { x, y }
  }
  // 安全区 = 圆的内接正方形先扣掉父 Embed 的标题 / 边框交互壳，再扣留白，
  // 与 fitView 的可用区走同一个函数（computeEmbedInnerContentSafeSide）：
  // 这样 fit 刚算完时内容一定在区内，用户拖出去的距离才会被夹回来而不是"先越界再回弹"，
  // 父圆的"点标题 / 边框选中"入口也永远不会被内容盖住（#818 复审）。
  const safeSize: number = computeEmbedInnerContentSafeSide(localSceneSize, paddingVp)
  const center: number = localSceneSize / 2
  const halfSafe: number = safeSize / 2
  const minLeft: number = center - halfSafe
  const maxLeft: number = center + halfSafe - itemWidth
  const minTop: number = center - halfSafe
  const maxTop: number = center + halfSafe - itemHeight
  // item 比整个安全区还大时没有合法区间，退回居中（至少不会贴着一侧溢出）。
  const clampedLeft: number = maxLeft < minLeft
    ? center - itemWidth / 2
    : Math.max(minLeft, Math.min(maxLeft, x * fitScale + offsetX))
  const clampedTop: number = maxTop < minTop
    ? center - itemHeight / 2
    : Math.max(minTop, Math.min(maxTop, y * fitScale + offsetY))
  return {
    x: (clampedLeft - offsetX) / fitScale,
    y: (clampedTop - offsetY) / fitScale
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
 * 几何字段（rects / scale / offset）是递归命中要读的快照，
 * 由 Scene 在重算几何和视口变化后 sync 一次；命令方法让"统一手势入口"
 * 可以隔着层级调用最深那一层：连线只能落在宿主 Scene 上，
 * 靠的就是这几条命令而不是"父层顺手改一改"（#816）。
 *
 * 缩放**不在**这里（#818）：双指只改全局相机，句柄上不保留任何用户缩放状态。
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
  /** 本层画布 → 本层局部屏幕的比例。根层是全局相机比例，子层是局部适配比例（#818）。 */
  scale: number = 1
  offsetX: number = 0
  offsetY: number = 0

  private getChildren: () => RecursiveSceneSource[]
  private createEdgeFn: (from: StarMapTargetPath, to: StarMapTargetPath) => Promise<boolean>
  private redrawEdgesFn: () => void

  constructor(
    scenePath: StarMapPathSegment[],
    starmapId: string,
    getChildren: () => RecursiveSceneSource[],
    createEdgeFn: (from: StarMapTargetPath, to: StarMapTargetPath) => Promise<boolean>,
    redrawEdgesFn: () => void
  ) {
    this.scenePath = scenePath
    this.starmapId = starmapId
    this.getChildren = getChildren
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
    offsetY: number
  ): void {
    this.rects = rects
    this.embedInstanceIds = embedInstanceIds
    this.edges = edges
    this.scale = scale
    this.offsetX = offsetX
    this.offsetY = offsetY
  }

  getChildEmbeds(): RecursiveSceneSource[] {
    return this.getChildren()
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
 * 存在的理由：connect 松手落在谁身上、跨层连线宿主是谁，
 * 这些判断都需要"从根往下看整棵树"，而这些判断发生在根 Scene 的手势入口里，
 * 根 Scene 拿不到子组件实例。注册表就是这层间接访问的唯一通道。
 *
 * 双指缩放**不**用注册表（#818）：全局相机只有一台，由 StarMapScreen 持有，
 * 不需要查"这一层是哪一层"。
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
