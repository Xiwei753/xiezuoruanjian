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
// "手机两层、平板三层"这种层数是**小视口自然算出来的结果**，不是常量：
// 视口小或相机缩得小 → 圆投影小 → 内部可用尺寸不够 → 折叠；
// 视口大或相机放大 → 展开。同一个设备改窗口大小会就地重算。
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
  EmbedDisplayBounds
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

/** 视口中心区域半宽（归一化）：候选圆心落在这个框里才算"当前看的就是它"。 */
export const FOCUS_CENTER_REGION_RATIO: number = 0.30

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

// ─── 显示几何统一 ───

/**
 * 一个视觉档位下的 Embed 显示边界。
 *
 * 这是"画出来多大"和"按多大算命中"的**唯一一份真相**：布局节点、命中测试、
 * 选中描边、边端点、触摸响应区全部读它。否则会出现"画的是 160×80 的摘要卡，
 * 命中和边端点还按 200×200 的圆算"这种错位（#820）。
 *
 * - expanded：正圆，宽高都等于直径，圆角 = 直径 / 2
 * - collapsed：和普通节点同尺寸的摘要卡，圆角与节点一致
 * - hidden：祖先已折叠，本层不渲染；返回摘要卡尺寸作为兜底（不应被用到）
 *
 * @param lod 视觉档位
 * @returns 显示边界（局部坐标，不含任何缩放）
 */
export function embedDisplayBoundsForLod(lod: EmbedLodMode): EmbedDisplayBounds {
  if (lod === 'expanded') {
    return {
      width: DEFAULT_EMBED_DIAMETER,
      height: DEFAULT_EMBED_DIAMETER,
      radius: DEFAULT_EMBED_DIAMETER / 2,
      isCircle: true
    }
  }
  return {
    width: DEFAULT_NODE_WIDTH,
    height: DEFAULT_NODE_HEIGHT,
    radius: DEFAULT_NODE_RADIUS,
    isCircle: false
  }
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
 * - `outerSizeVp = DEFAULT_EMBED_DIAMETER × ownerEffectiveScale`，
 *   **绝不能**把 Embed 自己 child Scene 的 local fit 乘进去：
 *   那是圆壳"里面"的内容大小，不是外壳大小。乘进去就等于"每深一层外壳就小一截"，
 *   折叠判定会自我实现（越深越折叠），再也展不开。
 *
 * @param embedDisplaySizeVp Embed 外壳的局部显示边长（展开态是 DEFAULT_EMBED_DIAMETER）
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

/** 候选圆心是否落在视口中心区域内。 */
function inCenterRegion(candidate: FocusCandidate): boolean {
  return candidate.centerRatioX >= FOCUS_CENTER_REGION_RATIO &&
    candidate.centerRatioX <= 1 - FOCUS_CENTER_REGION_RATIO &&
    candidate.centerRatioY >= FOCUS_CENTER_REGION_RATIO &&
    candidate.centerRatioY <= 1 - FOCUS_CENTER_REGION_RATIO
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
 * 1. 圆心不在视口中心区域的一律不看 —— 免得屏幕边缘那颗大圆抢走焦点；
 * 2. 中心区域里多个候选时先取层级最深的那一个（用户往里钻得越深，越深那层是焦点），
 *    同层再按覆盖率从大到小；
 * 3. 一个都不在中心区域 → 返回 null（焦点保持不变）。
 *
 * @param candidates 候选 Embed 列表
 * @returns 该被提升为焦点的候选，或 null
 */
export function resolveFocusCandidate(candidates: FocusCandidate[]): FocusCandidate | null {
  let best: FocusCandidate | null = null
  let bestDepth: number = -1
  let bestCoverage: number = -1
  for (const candidate of candidates) {
    if (!inCenterRegion(candidate)) {
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

/** 是否应当退出视觉焦点（覆盖率掉到滞回下沿之下）。 */
export function shouldDemoteFocus(coverage: number): boolean {
  return !Number.isFinite(coverage) || coverage < FOCUS_EXIT_COVERAGE
}

/** 在候选里找出某个 scenePath 的覆盖率；找不到时返回 0。 */
function coverageOfScenePath(scenePath: string, candidates: FocusCandidate[]): number {
  for (const candidate of candidates) {
    if (candidate.scenePath === scenePath) {
      return candidate.coverage
    }
  }
  return 0
}

/**
 * 一次性解出新的 focusScenePath（纯函数，不写状态）。
 *
 * 滞回（hysteresis）就在这里：进入用 FOCUS_ENTER_COVERAGE（0.70），
 * 退出用 FOCUS_EXIT_COVERAGE（0.55），中间那段保持不动。
 * 没有滞回的话，捏合停在 0.69 附近焦点会一帧一层地来回抖。
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
  const winner: FocusCandidate | null = resolveFocusCandidate(candidates)
  if (winner !== null && shouldPromoteFocus(winner.coverage)) {
    return winner.scenePath
  }
  if (currentFocusScenePath === 'root') {
    return 'root'
  }
  const currentCoverage: number = coverageOfScenePath(currentFocusScenePath, candidates)
  if (shouldDemoteFocus(currentCoverage)) {
    // 当前焦点已经不够大（或这轮根本没看见它）→ 退回父层视觉焦点
    return parentScenePathOf(currentFocusScenePath)
  }
  return currentFocusScenePath
}