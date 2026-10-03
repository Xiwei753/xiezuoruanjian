// ─── StarMapVisualLod.ets ───
// 星图视觉 LOD（语义缩放）纯计算模块 —— Issue #820。
//
// 这一层是 HarmonyOS **显示层**的东西，不进 writer_core：
// Core 只知道"这张星图里有这些节点、这些 Embed、这些 authored position"，
// 完全不知道屏幕、相机、折叠和视觉焦点。这里负责把这些量算出来：
//
//   屏幕投影链（#820 规范）：
//     child local → 父 Embed local fit → … → root world → 全局相机 → 屏幕
//   effectiveScale = globalCameraScale × 所有祖先 EmbedLocalFitScale
//   projectedEmbedSize = embedDisplaySize × ownerSceneEffectiveScale
//   coverage = projectedEmbedSize / min(viewportWidth, viewportHeight)
//
// 三档 LOD 的判定**只看屏幕上有多少真实空间**，不看"这是第几层"：
// - expanded：完整圆壳 + 实例化 child Scene（内部可命中、可编辑、可继续递归）
// - collapsed：节点尺寸的摘要卡（标题 + 子星图身份 + 选中态），
//   整个摘要热区都属于这个 Embed，不实例化 child Scene，也不再向下递归
// - hidden：祖先已折叠 / 不需要，压根不创建 Scene 实例（数据仍在 Core 里）
//
// Viewport 改大小要真的改**显示预算**，不是只改 coverage（#820 复核）：
//   视口短边 → viewportDetailScale()（连续比例，有上下限，不是设备断点表）
//            → expandedEmbedSizeVp() = 基准直径 × 该比例
//            → 圆内可用空间随之变化 → LOD 跟着变
// 展开态圆壳因此在大视口下拿到更多真实 vp 空间，child local fit 也自然更宽裕，
// 深一层才有可能继续 expanded；小视口则反过来。
//
// 这份尺寸必须是**同一份显示真相**：LOD、显示边界、布局矩形、渲染盒子全部读
// `expandedEmbedSizeVp()`，不能出现"LOD 按 320 算、UI 还画 200"的第二套尺寸。
// 折叠摘要卡不跟着视口无限放大，保持普通节点附近的尺寸。
//
// "手机两层、平板三层"这种层数是**小视口自然算出来的结果**，不是常量：
// 视口小或相机缩得小 → 圆投影小 → 内部可用尺寸不够 → 折叠；
// 视口大或相机放大 → 展开。同一个设备改窗口大小会就地重算。
//
// 视口 resize 只重算显示层级，**不碰视觉焦点**（#820 复核）：
// 保持 focusScenePath、保持相机，把新视口尺寸往下传，各 Scene 自己重算 LOD。
// 否则折叠屏展开一下、分屏拖宽一点就会莫名其妙换焦点。
//
// 视觉焦点只是"显示参考根"，两组滞回常量都在这一层：
// - 覆盖率：进入 ≥ 0.70 / 退出 < 0.55
// - 位置：圆心进入要落在 30%~70%，保持在 20%~80%
// 两组都只为防抖，不是设备分类。退出必须同时看位置，否则用户把相机拖走之后，
// 一条已经不在视野里的焦点链会被继续强制展开。
//
// 所有函数都是纯函数：不读 Core、不碰 ArkUI、不做 per-frame UniFFI 调用。
// LOD 是高频显示计算，全部在本地一次算完。
//
// 关于依赖方向：本模块只依赖 StarMapLayout（纯尺寸常量）和 StarMapViewport
// （投影所需的纯几何），**不依赖 StarMapGeometry**。原因是 Viewport 已经依赖
// Geometry，再让 Geometry 反向 import 本模块会绕成 platform 模块之间的环，
// 而 Viewport 顶层的常量在模块初始化时就要读 Geometry 的常量，环初始化会算成 NaN。

import {
  DEFAULT_EMBED_DIAMETER,
  DEFAULT_NODE_WIDTH,
  DEFAULT_NODE_HEIGHT,
  DEFAULT_NODE_RADIUS,
  EmbedDisplayBounds,
  expandedEmbedDisplayBounds
} from './StarMapLayout'
import { computeEmbedInnerContentSafeSide } from './StarMapViewport'

// ─── 类型 ───

/** Embed 的视觉档位。hidden 表示祖先折叠，本层连 Scene 实例都不创建。 */
export type EmbedLodMode = 'expanded' | 'collapsed' | 'hidden'

/** LOD 计算需要的**根**视口上下文。整棵递归树共用同一份，子 Scene 不许拿自己的圆当视口。 */
export interface VisualLodContext {
  /** 根 StarMap 实际可绘制内容区宽度（vp） */
  viewportWidthVp: number
  /** 根 StarMap 实际可绘制内容区高度（vp） */
  viewportHeightVp: number
  /** 当前视觉焦点的 scenePath 描述（describeScenePath 结果，根为 'root'） */
  focusScenePath: string
}

/** 一个 Embed 投影到屏幕上的度量。全部是屏幕侧 vp，与 authored position 无关。 */
export interface ProjectedEmbedMetrics {
  /** Embed 外壳投影后的屏幕边长（vp） */
  outerSizeVp: number
  /** 圆内"手能可靠点到"的内容区投影后的屏幕边长（vp） */
  innerUsableSizeVp: number
  /** outerSizeVp 占视口短边的比例 */
  coverage: number
}

/** 展开所需的下限：圆内可用尺寸至少要放得下一个节点（现有节点宽度就是可编辑下限）。 */
export const EXPANDED_MIN_INNER_USABLE_VP: number = DEFAULT_NODE_WIDTH

/** 进入视觉焦点所需的覆盖率阈值（滞回上沿）。 */
export const FOCUS_ENTER_COVERAGE: number = 0.70

/** 退出视觉焦点所需的覆盖率阈值（滞回下沿）。 */
export const FOCUS_EXIT_COVERAGE: number = 0.55

/**
 * 进入视觉焦点的位置窗口（归一化，滞回上沿）：圆心要落在 30% ~ 70% 之间。
 *
 * 免得屏幕边缘那颗大圆刚过覆盖率阈值就抢走焦点。
 */
export const FOCUS_CENTER_ENTER_RATIO: number = 0.30

/**
 * 保持视觉焦点的位置窗口（归一化，滞回下沿）：圆心还在 20% ~ 80% 之间就继续挂着。
 *
 * 和进入窗口分开构成**位置滞回**：手指刚碰边（0.29 / 0.71）不退出，
 * 真的把对象拖出视野（< 0.20 / > 0.80）才退出。
 * 和覆盖率的 0.70 / 0.55 一样是显示常量，不进 Core、不按设备分支。
 */
export const FOCUS_CENTER_EXIT_RATIO: number = 0.20

/** 焦点判定用的候选 Embed（调用方负责把 Embed 屏幕中心换算成视口内比例）。 */
export interface FocusCandidate {
  /** 该 Embed 所属 Scene 的 scenePath 描述 */
  scenePath: string
  /** 该 Embed 的投影覆盖率 */
  coverage: number
  /** 圆心在视口内的归一化 X（0 = 左边缘，1 = 右边缘） */
  centerRatioX: number
  /** 圆心在视口内的归一化 Y */
  centerRatioY: number
}

/**
 * 参考短边（vp）：视口短边达到这个值就按 1.0 的显示预算走。
 *
 * 取常见手机竖屏短边。这只是**连续比例的分母**，不是设备断点，
 * 不判断"这是不是手机 / 平板"，也没有任何机型表。
 */
export const VIEWPORT_REFERENCE_SHORT_SIDE_VP: number = 360

/** 视口显示预算的下限：视口再小也按基准尺寸画，不会无限缩小圆壳。 */
export const MIN_VIEWPORT_DETAIL_SCALE: number = 1.0

/** 视口显示预算的上限：视口再大圆壳也只放大到这里，避免大屏上圆壳糊满屏。 */
export const MAX_VIEWPORT_DETAIL_SCALE: number = 1.6

// ─── 显示几何统一 ───

/**
 * 一个视觉档位下的 Embed 显示边界。
 *
 * 这是"画出来多大"和"按多大算命中"的**唯一一份真相**：布局节点、命中测试、
 * 选中描边、边端点、触摸响应区全部读它。否则会出现"画的是 160×80 的摘要卡，
 * 命中和边端点还按 200×200 的圆算"这种错位（#820）。
 *
 * - expanded：正圆，宽高都等于**当前 Viewport 下的展开尺寸**
 *   （`expandedEmbedSizeVp(context)`，不是写死的 200），圆角 = 边长 / 2
 * - collapsed：和普通节点同尺寸的摘要卡，圆角与节点一致，**不跟着视口无限放大**
 * - hidden：祖先已折叠，本层不渲染；返回摘要卡尺寸作为兜底（不应被用到）
 *
 * @param lod 视觉档位
 * @param context 根视口上下文（expanded 分支要按它算显示尺寸）
 * @returns 显示边界（局部坐标，不含任何缩放）
 */
export function embedDisplayBoundsForLod(
  lod: EmbedLodMode,
  context: VisualLodContext
): EmbedDisplayBounds {
  if (lod === 'expanded') {
    return expandedEmbedDisplayBounds(expandedEmbedSizeVp(context))
  }
  return {
    width: DEFAULT_NODE_WIDTH,
    height: DEFAULT_NODE_HEIGHT,
    radius: DEFAULT_NODE_RADIUS,
    isCircle: false
  }
}

// ─── 视口显示预算 ───

/** 把比例夹到 [min, max]。 */
function clampDetailScale(scale: number): number {
  if (!Number.isFinite(scale)) {
    return MIN_VIEWPORT_DETAIL_SCALE
  }
  return Math.max(MIN_VIEWPORT_DETAIL_SCALE, Math.min(MAX_VIEWPORT_DETAIL_SCALE, scale))
}

/**
 * 视口显示预算：一个**连续**比例，输入只有视口短边，不判断设备类型。
 *
 * 这是"同一台设备换窗口大小 → 视觉层数自然变化"的闭环所在（#820）：
 * Viewport 变了 → 显示预算变了 → 展开态圆壳的真实 vp 尺寸变了 →
 * 圆内可用空间变了 → LOD 档位跟着变。写死 200vp 的话，视口只影响 coverage，
 * 不影响 LOD，"大屏多展开几层"就永远实现不了。
 *
 * 之所以是连续比例而不是断点表：断点表就是变相的设备分类，
 * 折叠屏展开、半屏、分屏这些中间态全都要单独开一条规则。
 *
 * @param context 根视口上下文
 * @returns 显示预算比例（≥ MIN_VIEWPORT_DETAIL_SCALE，≤ MAX_VIEWPORT_DETAIL_SCALE）
 */
export function viewportDetailScale(context: VisualLodContext): number {
  const shortSide: number = viewportShortSideVp(context)
  if (shortSide <= 0) {
    return MIN_VIEWPORT_DETAIL_SCALE
  }
  return clampDetailScale(shortSide / VIEWPORT_REFERENCE_SHORT_SIDE_VP)
}

/**
 * 当前 Viewport 下展开态 Embed 的**真实显示边长**（局部坐标）。
 *
 * 这是全链路的单一尺寸来源：LOD 判定、显示边界、布局矩形、渲染盒子
 * 都必须读它，不允许任何地方再写死 DEFAULT_EMBED_DIAMETER 当"展开尺寸"，
 * 否则就会出现"LOD 按 320 算、UI 还画 200"的第二套尺寸。
 *
 * @param context 根视口上下文
 * @returns 展开态圆壳的局部显示边长（vp）
 */
export function expandedEmbedSizeVp(context: VisualLodContext): number {
  return DEFAULT_EMBED_DIAMETER * viewportDetailScale(context)
}

// ─── 投影 ───

/** 视口短边（coverage 的分母）。非有限值或非正数时按 0 处理，避免除出 NaN。 */
function viewportShortSideVp(context: VisualLodContext): number {
  const w: number = context.viewportWidthVp
  const h: number = context.viewportHeightVp
  if (!Number.isFinite(w) || !Number.isFinite(h) || w <= 0 || h <= 0) {
    return 0
  }
  return Math.min(w, h)
}

/** 有效的 owner Scene 累计比例。非有限值或非正数时按 1 处理（等价"没有祖先缩放"）。 */
function effectiveOwnerScale(ownerEffectiveScale: number): number {
  if (!Number.isFinite(ownerEffectiveScale) || ownerEffectiveScale <= 0) {
    return 1
  }
  return ownerEffectiveScale
}

/**
 * 把一个 Embed 的屏幕投影度量算出来。
 *
 * 口径（#820 规范，评审重点）：
 * - `ownerEffectiveScale` 必须是 Embed **所属 Scene** 的最终有效比例，
 *   也就是 `全局相机比例 × 所有祖先 Embed 的局部适配比例`，由
 *   `ui/StarMapScene.viewportScaleValue()` 提供；
 * - `outerSizeVp = embedDisplaySizeVp × ownerEffectiveScale`，
 *   **绝不能**把 Embed 自己 child Scene 的 local fit 乘进去：
 *   那是圆壳"里面"的内容大小，不是外壳大小。乘进去就等于"每深一层外壳就小一截"，
 *   折叠判定会自我实现（越深越折叠），再也展不开。
 *
 * `embedDisplaySizeVp` 必须传 `expandedEmbedSizeVp(context)`（当前 Viewport 下的展开尺寸），
 * 不许写死 DEFAULT_EMBED_DIAMETER：那样 Viewport 只影响 coverage、不影响 LOD，
 * "同一平台不同屏幕尺寸自然出现不同视觉层数"就落不了地（#820 复核）。
 *
 * @param embedDisplaySizeVp Embed 外壳的局部显示边长（展开态用 expandedEmbedSizeVp(context)）
 * @param ownerEffectiveScale Embed 所属 Scene 的最终有效比例（相机 × 祖先 local fit）
 * @param context 根视口上下文
 * @param paddingVp 内容区四边留白（与 fitView / clamp 用同一份）
 * @returns 屏幕投影度量
 */
export function projectEmbedMetrics(
  embedDisplaySizeVp: number,
  ownerEffectiveScale: number,
  context: VisualLodContext,
  paddingVp: number
): ProjectedEmbedMetrics {
  const scale: number = effectiveOwnerScale(ownerEffectiveScale)
  const size: number = Number.isFinite(embedDisplaySizeVp) && embedDisplaySizeVp > 0
    ? embedDisplaySizeVp
    : DEFAULT_EMBED_DIAMETER
  const outerSizeVp: number = size * scale
  const innerLocal: number = computeEmbedInnerContentSafeSide(size, paddingVp)
  const innerUsableSizeVp: number = innerLocal * scale
  const shortSide: number = viewportShortSideVp(context)
  const coverage: number = shortSide > 0 ? outerSizeVp / shortSide : 0
  return { outerSizeVp: outerSizeVp, innerUsableSizeVp: innerUsableSizeVp, coverage: coverage }
}

// ─── 视觉焦点 ───

/** 候选圆心是否落在**进入**窗口（30% ~ 70%）内——新候选进焦点要看这个。 */
function inCenterEnterRegion(candidate: FocusCandidate): boolean {
  return inCenterWindow(candidate, FOCUS_CENTER_ENTER_RATIO)
}

/**
 * 候选圆心是否还在**保持**窗口（20% ~ 80%）内——已持有的焦点要看这个。
 *
 * 比进入窗口宽，这就是位置滞回：手指稍微拖一下不会把焦点抖掉，
 * 真的把整颗圆拖出视野才会退。
 */
function inCenterExitRegion(candidate: FocusCandidate): boolean {
  return inCenterWindow(candidate, FOCUS_CENTER_EXIT_RATIO)
}

/** 归一化视口里的一个居中窗口判定（ratio 越小框越大）。 */
function inCenterWindow(candidate: FocusCandidate, ratio: number): boolean {
  return candidate.centerRatioX >= ratio &&
    candidate.centerRatioX <= 1 - ratio &&
    candidate.centerRatioY >= ratio &&
    candidate.centerRatioY <= 1 - ratio
}

/** scenePath 描述的层级深度（'root' 是 0，'root/embed:a' 是 1，逐层 +1）。 */
export function scenePathDepthOf(scenePath: string): number {
  if (scenePath === 'root') {
    return 0
  }
  let depth: number = 0
  for (let i = 0; i < scenePath.length; i++) {
    if (scenePath.charAt(i) === '/') {
      depth += 1
    }
  }
  return depth
}

/** 取父层 scenePath 描述（'root/embed:a/embed:b' → 'root/embed:a'）。已在根层时原样返回。 */
export function parentScenePathOf(scenePath: string): string {
  if (scenePath === 'root') {
    return 'root'
  }
  const idx: number = scenePath.lastIndexOf('/embed:')
  if (idx < 0) {
    return 'root'
  }
  return scenePath.substring(0, idx)
}

/**
 * 某个 candidate Embed 的 childScenePath 是否在"通往视觉焦点的那条链"上
 * （焦点自身或它的祖先）。
 *
 * 注意入参是**这颗 Embed 进入之后的路径**，不是它所属的父 Scene：
 * 焦点在 `root/embed:a` 时只有 a 属于焦点链，同层兄弟 b / c 仍按屏幕空间正常折叠；
 * 传父 Scene 的 `root` 会让初始焦点 `root` 把整层 Embed 全部强制展开，根层 LOD 直接失效（#820 复核）。
 *
 * LOD 判定要用它保住这条链：从根到 `focusScenePath` 的每一层都必须展开，
 * 否则用户捏进去的子星图会因为"里面不够大"自己把自己折起来，
 * 或者它的祖先先折了，用户就地掉出焦点层。
 *
 * 焦点**之后**的更深层不在这条链上：那几层继续按屏幕空间正常判定，
 * 否则"语义缩放"就退化成"从焦点往下全部展开"，再深也不折叠了。
 *
 * @param embedScenePath 待判定的 **candidate Embed 进入之后**的 childScenePath 描述
 * @param focusScenePath 当前视觉焦点的 scenePath 描述
 * @returns true 表示它是焦点自身或焦点的祖先
 */
export function isOnFocusChain(embedScenePath: string, focusScenePath: string): boolean {
  return embedScenePath === focusScenePath || focusScenePath.startsWith(embedScenePath + '/')
}

// ─── LOD 判定 ───

/**
 * 单个 Embed 的视觉档位（只看屏幕上有多少真实空间）。
 *
 * 判据不是"第几层"，而是圆内真正能用的屏幕尺寸：
 * `innerUsableSizeVp >= DEFAULT_NODE_WIDTH`（放得下一个节点）才展开，
 * 否则折叠成节点尺寸的摘要卡。
 *
 * @param metrics 该 Embed 的屏幕投影度量
 * @returns expanded / collapsed
 */
export function resolveEmbedLod(metrics: ProjectedEmbedMetrics): EmbedLodMode {
  if (!Number.isFinite(metrics.innerUsableSizeVp)) {
    return 'hidden'
  }
  if (metrics.innerUsableSizeVp >= EXPANDED_MIN_INNER_USABLE_VP) {
    return 'expanded'
  }
  return 'collapsed'
}

/**
 * 带祖先折叠与焦点链判定的完整 LOD 求解。
 *
 * @param metrics 该 Embed 的屏幕投影度量
 * @param embedScenePath 该 Embed 进入之后的 childScenePath 描述（**不是**所属父 Scene 的路径）
 * @param focusScenePath 当前视觉焦点的 scenePath 描述
 * @param ancestorCollapsed 祖先是否已经折叠（折叠后不创建任何后代 Scene）
 * @returns 视觉档位
 */
export function resolveEmbedLodForScene(
  metrics: ProjectedEmbedMetrics,
  embedScenePath: string,
  focusScenePath: string,
  ancestorCollapsed: boolean
): EmbedLodMode {
  if (ancestorCollapsed) {
    return 'hidden'
  }
  if (isOnFocusChain(embedScenePath, focusScenePath)) {
    return 'expanded'
  }
  return resolveEmbedLod(metrics)
}

// ─── 焦点候选与滞回 ───

/**
 * 从候选里挑出"当前视觉焦点应该是谁"。
 *
 * 规则（#820）：
 * 1. 圆心不在**进入窗口**（30% ~ 70%）的一律不看 —— 免得屏幕边缘那颗大圆抢走焦点；
 * 2. 窗口里多个候选时先取层级最深的那一个（用户往里钻得越深，越深那层是焦点），
 *    同层再按覆盖率从大到小；
 * 3. 一个都不在窗口内 → 返回 null（焦点保持不变）。
 *
 * **调用方必须先按覆盖率过滤再传进来**：这里只做"位置 + 深度 + 同层大小"的排序，
 * 不做覆盖率判断。先在这里选出最深的再回头查覆盖率，会出现"一颗不够大的深层候选
 * 把合法父候选整批否决"的情况（#820 复核）。
 *
 * @param candidates 候选 Embed 列表（通常已过滤到 coverage ≥ FOCUS_ENTER_COVERAGE）
 * @returns 该被提升为焦点的候选，或 null
 */
export function resolveFocusCandidate(candidates: FocusCandidate[]): FocusCandidate | null {
  let best: FocusCandidate | null = null
  let bestDepth: number = -1
  let bestCoverage: number = -1
  for (const candidate of candidates) {
    if (!inCenterEnterRegion(candidate)) {
      continue
    }
    const depth: number = scenePathDepthOf(candidate.scenePath)
    if (best === null || depth > bestDepth ||
      (depth === bestDepth && candidate.coverage > bestCoverage)) {
      best = candidate
      bestDepth = depth
      bestCoverage = candidate.coverage
    }
  }
  return best
}

/** 是否应当把某个候选提升为视觉焦点（覆盖率过滞回上沿）。 */
export function shouldPromoteFocus(coverage: number): boolean {
  return Number.isFinite(coverage) && coverage >= FOCUS_ENTER_COVERAGE
}

/**
 * 是否应当退出视觉焦点。
 *
 * 两个条件各管一件事，缺一不可：
 * - `coverage < 0.55`：太小了，不值得继续当参考根；
 * - 圆心已经离开**保持窗口**（20% ~ 80%）：用户把相机拖走了，它根本不在看的东西里。
 *
 * 只看 coverage 会让"被拖出视口但仍然很大"的焦点永久赖着：
 * 那条已经看不见的链会被继续强制 expanded，LOD 参考根和用户实际在看的区域脱节。
 *
 * @param candidate 当前焦点对应的完整候选（含覆盖率与圆心位置）
 * @returns true 表示应当退出焦点（退回父层）
 */
export function shouldDemoteFocus(candidate: FocusCandidate): boolean {
  return !Number.isFinite(candidate.coverage) ||
    candidate.coverage < FOCUS_EXIT_COVERAGE ||
    !inCenterExitRegion(candidate)
}

/** 在候选里找出某个 scenePath 对应的**完整**候选；找不到时返回 null。 */
export function findFocusCandidate(
  scenePath: string,
  candidates: FocusCandidate[]
): FocusCandidate | null {
  for (const candidate of candidates) {
    if (candidate.scenePath === scenePath) {
      return candidate
    }
  }
  return null
}

/**
 * 过滤出"有资格进入焦点"的候选（覆盖率过滞回上沿）。
 *
 * 抽成小纯函数是为了让"根层正常选"和"往当前焦点里面继续钻"两处共用同一套筛选，
 * 免得两边的进入门槛悄悄长歪。
 */
function promotableCandidates(candidates: FocusCandidate[]): FocusCandidate[] {
  const promotable: FocusCandidate[] = []
  for (const candidate of candidates) {
    if (shouldPromoteFocus(candidate.coverage)) {
      promotable.push(candidate)
    }
  }
  return promotable
}

/**
 * 一次性解出新的 focusScenePath（纯函数，不写状态）。
 *
 * 这是一个**有记忆的状态机**，不是"每帧从全树重选一个 winner"：
 *
 * 1. 根层没有需要保持的旧焦点，正常从全体 promotable 里选；
 * 2. 非根层时，先看**当前焦点的严格后代**里有没有 promotable —— 有就晋升
 *    （用户继续往当前子星图里面钻，这是唯一允许的"跳"）；
 * 3. 没有更深的后代，但当前焦点仍在保持区间（coverage ≥ 0.55 且圆心还在
 *    20%~80%）就原地保持，**祖先和兄弟不能抢**；
 * 4. 当前焦点真的掉出保持区间，退到父层。下一轮再从父层正常选，
 *    不在这一轮里跨分支瞬移到一个兄弟。
 *
 * 两层滞回：
 * - **覆盖率**：进入 ≥ 0.70，退出 < 0.55。捏合停在 0.69 附近不会一帧一层地抖。
 * - **位置**：进入要落在 30% ~ 70%，保持窗口是 20% ~ 80%。手指碰边不退出，
 *   真的把对象拖出视野才退出。
 *
 * 两条顺序坑（#820 复核）：
 * - 先选最深的、再查它够不够 70%，会让一颗很小的深层候选把合法父候选整批否决；
 * - 每帧先从全树重选 winner、最后才判 demote，会让保持区间只在没有别的
 *   promotable 候选时才生效，深层焦点的滞回等于没有。
 *
 * 焦点只改变"显示参考根"：不改 Core、不新建页面、不换相机、不落盘。
 *
 * @param currentFocusScenePath 当前视觉焦点的 scenePath 描述
 * @param candidates 候选 Embed 列表
 * @returns 新的 focusScenePath（不变时原样返回）
 */
export function resolveFocusScenePath(
  currentFocusScenePath: string,
  candidates: FocusCandidate[]
): string {
  const promotable: FocusCandidate[] = promotableCandidates(candidates)
  if (currentFocusScenePath === 'root') {
    const winner: FocusCandidate | null = resolveFocusCandidate(promotable)
    return winner !== null ? winner.scenePath : 'root'
  }
  const current: FocusCandidate | null = findFocusCandidate(currentFocusScenePath, candidates)
  const descendants: FocusCandidate[] = []
  for (const candidate of promotable) {
    if (candidate.scenePath.startsWith(currentFocusScenePath + '/')) {
      descendants.push(candidate)
    }
  }
  const deeper: FocusCandidate | null = resolveFocusCandidate(descendants)
  if (deeper !== null) {
    return deeper.scenePath
  }
  if (current !== null && !shouldDemoteFocus(current)) {
    // 仍在保持区间内 → 保持，不允许祖先 / 兄弟顶掉
    return currentFocusScenePath
  }
  // 不够大、或已经被拖出保持窗口（也包括这轮根本没看见它）→ 退回父层
  return parentScenePathOf(currentFocusScenePath)
}