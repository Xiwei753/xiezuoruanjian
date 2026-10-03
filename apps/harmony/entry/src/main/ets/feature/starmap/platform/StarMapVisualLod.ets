// ─── StarMapVisualLod.ets ───
// 星图 Deep Zoom 显示细节档位 + 视觉焦点纯计算模块 —— Issue #821。
//
// 这一层是 HarmonyOS **显示层**的东西，不进 writer_core：
// Core 只知道"这张星图里有这些节点、这些 Embed、这些 authored position"，
// 完全不知道屏幕、相机和视觉焦点。这里负责把这些量算出来：
//
//   屏幕投影链（#821 规范）：
//     稳定 world geometry → 全局相机 → 每层 local fit → Deep Zoom detail
//   effectiveScale = globalCameraScale × 所有祖先 EmbedLocalFitScale
//   projectedDiameterVp = DEFAULT_EMBED_DIAMETER × ownerEffectiveScale
//   coverage = projectedDiameterVp / min(viewportWidth, viewportHeight)
//
// #821 的核心边界：**Deep Zoom 只决定"渲染多少细节 / 是否挂完整交互组件"，
// 绝不改 Embed 的形状、world bounds 或它在父 Scene 里的 authored position。**
// 子星图在任何档位、任何缩放下都还是那颗正圆（DEFAULT_EMBED_DIAMETER，
// 圆角 = 边长 / 2），不会再变成 160×80 的矩形节点摘要卡。用户缩放只改变视角，
// 同层所有对象按同一个相机比例等比缩放；只有真正创建 / 移动 / 缩放某个对象的
// world bounds 时才谈得上避让。缩放不是布局变化。
//
// 三档细节只看屏幕上有多少真实空间：
// - interactive：实例化 child Scene（内部可命中、可编辑、可继续递归）
// - preview：轻量 Canvas 缩略图，只回答"里面大概有什么"，不挂交互组件
// - shell：只画圆壳，连缩略图都不画（数据仍在 Core 里，随时可以再展开）
//
// 档位之间用滞回防抖：
// - interactive 进入 coverage ≥ 0.70 / 退出 ≥ 0.60
// - preview 进入 projectedDiameterVp ≥ 48vp / 退出 ≥ 40vp
// 视觉焦点链上的 Embed 至少保底 preview（不会掉到纯圆壳），但**保底的不是形状**，
// 只是"里面还有东西可看"。
//
// "手机两层、平板三层"这种层数是**小视口自然算出来的结果**，不是常量：
// 视口小或相机缩得小 → 圆投影小 → 内部可用尺寸不够 → 掉档；
// 视口大或相机放大 → 交互层。同一个设备改窗口大小会就地重算。
//
// 视口 resize 与相机缩放都**不碰 authored position、不碰 world 几何**，
// 只重算显示细节档位，focusScenePath 由 ui/StarMapScreen 的焦点状态机另行维护。
//
// 视觉焦点只是"显示参考根"，两组滞回常量都在这一层：
// - 覆盖率：进入 ≥ 0.70 / 退出 < 0.55
// - 位置：圆心进入要落在 30%~70%，保持在 20%~80%
// 两组都只为防抖，不是设备分类。退出必须同时看位置，否则用户把相机拖走之后，
// 一条已经不在视野里的焦点链会被继续强制保底。
//
// 所有函数都是纯函数：不读 Core、不碰 ArkUI、不做 per-frame UniFFI 调用。
// Deep Zoom 档位是高频显示计算，全部在本地一次算完。
//
// 关于依赖方向：本模块只依赖 StarMapLayout（纯尺寸常量），
// **不依赖 StarMapViewport**（#820 旧模型要算圆内可用尺寸才需要它，
// #821 稳定圆壳之后不再需要）。也不依赖 StarMapGeometry：
// 原因是 Geometry 若反向 import 本模块会绕成 platform 模块之间的环，
// 而 Viewport 顶层的常量在模块初始化时就要读 Geometry 的常量，环初始化会算成 NaN。

import { DEFAULT_EMBED_DIAMETER } from './StarMapLayout'

// ─── 类型 ───

/**
 * Deep Zoom 细节档位。
 *
 * 三档都画同一颗圆、同一份 world geometry，区别只在"圆里渲染多少东西"：
 * - interactive：挂完整 child Scene，内部节点是真实 ArkUI 组件，可点可拖
 * - preview：一张固定尺寸 Canvas 缩略图，节点/边/子星图都是画出来的，没有交互组件
 * - shell：只画圆壳
 */
export type DeepZoomDetail = 'interactive' | 'preview' | 'shell'

/** 档位计算需要的**根**视口上下文。整棵递归树共用同一份，子 Scene 不许拿自己的圆当视口。 */
export interface VisualLodContext {
  /** 根 StarMap 实际可绘制内容区宽度（vp） */
  viewportWidthVp: number
  /** 根 StarMap 实际可绘制内容区高度（vp） */
  viewportHeightVp: number
  /** 当前视觉焦点的 scenePath 描述（describeScenePath 结果，根为 'root'） */
  focusScenePath: string
}

/**
 * 一个 Embed 投影到屏幕上的度量。全部是屏幕侧 vp，与 authored position 无关。
 *
 * #821 起只有这两个字段：外壳尺寸恒为 DEFAULT_EMBED_DIAMETER × 有效比例，
 * 不再有"外框 / 圆内可用"两套尺寸，也不再有随视口变化的展开尺寸。
 */
export interface ProjectedEmbedMetrics {
  /** 圆壳投影后的屏幕直径（vp） */
  projectedDiameterVp: number
  /** projectedDiameterVp 占视口短边的比例 */
  coverage: number
}

/** 进入 interactive 档所需的覆盖率阈值（滞回上沿）。 */
export const INTERACTIVE_ENTER_COVERAGE: number = 0.70

/** 保持 interactive 档所需的覆盖率阈值（滞回下沿）。 */
export const INTERACTIVE_EXIT_COVERAGE: number = 0.60

/**
 * 进入 preview 档所需的投影直径下限（vp）。
 *
 * 再小就只剩圆壳：画出来的节点圆和字都糊成一团，比不画更难认。
 * 48vp 大约能让里面几个节点圆分辨出形状和相对位置 —— 这正是 preview 要回答的问题。
 */
export const PREVIEW_MIN_DIAMETER_VP: number = 48

/** 保持 preview 档所需的投影直径下限（vp），防抖用。 */
export const PREVIEW_EXIT_DIAMETER_VP: number = 40

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
 * 口径（#821 规范）：
 * - `ownerEffectiveScale` 必须是 Embed **所属 Scene** 的最终有效比例，
 *   也就是 `全局相机比例 × 所有祖先 Embed 的局部适配比例`，由
 *   `ui/StarMapScene.viewportScaleValue()` 提供；
 * - 直径恒为 `DEFAULT_EMBED_DIAMETER × ownerEffectiveScale`，
 *   **绝不能**把 Embed 自己 child Scene 的 local fit 乘进去：
 *   那是圆壳"里面"的内容大小，不是外壳大小。
 *
 * 之所以不再有 embedDisplaySizeVp 入参：#821 之后外壳尺寸是稳定常量，
 * 任何地方都不许再传一个"随视口变化的展开尺寸"进来。
 *
 * @param ownerEffectiveScale Embed 所属 Scene 的最终有效比例（相机 × 祖先 local fit）
 * @param context 根视口上下文
 * @returns 屏幕投影度量
 */
export function projectEmbedMetrics(
  ownerEffectiveScale: number,
  context: VisualLodContext
): ProjectedEmbedMetrics {
  const scale: number = effectiveOwnerScale(ownerEffectiveScale)
  const projectedDiameterVp: number = DEFAULT_EMBED_DIAMETER * scale
  const shortSide: number = viewportShortSideVp(context)
  const coverage: number = shortSide > 0 ? projectedDiameterVp / shortSide : 0
  return { projectedDiameterVp: projectedDiameterVp, coverage: coverage }
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
 * 焦点在 `root/embed:a` 时只有 a 属于焦点链，同层兄弟 b / c 仍按屏幕空间正常掉档。
 *
 * Detail 判定要用它保住这条链：从根到 `focusScenePath` 的每一层至少留住 preview，
 * 否则用户捏进去的子星图会退化成一颗空圆壳，"里面有什么"就再也看不到了。
 *
 * 焦点**之后**的更深层不在这条链上：那几层继续按屏幕空间正常判定，
 * 否则"语义缩放"就退化成"从焦点往下全部保底"，再深也不掉档了。
 *
 * @param embedScenePath 待判定的 **candidate Embed 进入之后**的 childScenePath 描述
 * @param focusScenePath 当前视觉焦点的 scenePath 描述
 * @returns true 表示它是焦点自身或焦点的祖先
 */
export function isOnFocusChain(embedScenePath: string, focusScenePath: string): boolean {
  return embedScenePath === focusScenePath || focusScenePath.startsWith(embedScenePath + '/')
}

// ─── Deep Zoom 档位判定 ───

/**
 * 单个 Embed 的 Deep Zoom 细节档位（只看屏幕上有多少真实空间）。
 *
 * 判据只有两条，都是**连续**的屏幕量，没有"第几层"也没有设备断点：
 * - `coverage ≥ 0.70` → 圆几乎占满视口短边，里面塞得下真实组件 → interactive
 * - `projectedDiameterVp ≥ 48` → 还能看出里面大概有什么 → preview
 * - 再小 → 只剩圆壳
 *
 * @param metrics 该 Embed 的屏幕投影度量
 * @param previousDetail 上一帧的档位（滞回用），首次渲染传 null
 * @returns interactive / preview / shell
 */
export function resolveDeepZoomDetail(
  metrics: ProjectedEmbedMetrics,
  previousDetail: DeepZoomDetail | null
): DeepZoomDetail {
  if (!Number.isFinite(metrics.projectedDiameterVp) || !Number.isFinite(metrics.coverage)) {
    return 'shell'
  }
  if (metrics.coverage >= INTERACTIVE_ENTER_COVERAGE) {
    return 'interactive'
  }
  if (previousDetail === 'interactive' && metrics.coverage >= INTERACTIVE_EXIT_COVERAGE) {
    return 'interactive'
  }
  if (metrics.projectedDiameterVp >= PREVIEW_MIN_DIAMETER_VP) {
    return 'preview'
  }
  if (previousDetail === 'preview' && metrics.projectedDiameterVp >= PREVIEW_EXIT_DIAMETER_VP) {
    return 'preview'
  }
  return 'shell'
}

/**
 * 带焦点链保底的完整档位求解。
 *
 * 焦点链上的 Embed 至少保底 preview：用户捏进去的那条链不能退化成空圆壳。
 * 保底的只是"里面还有东西可看"，**不是形状**——它照样是同一颗正圆。
 *
 * @param metrics 该 Embed 的屏幕投影度量
 * @param embedScenePath 该 Embed 进入之后的 childScenePath 描述（**不是**所属父 Scene 的路径）
 * @param focusScenePath 当前视觉焦点的 scenePath 描述
 * @param previousDetail 上一帧的档位（滞回用），首次渲染传 null
 * @returns Deep Zoom 细节档位
 */
export function resolveEmbedDetailForScene(
  metrics: ProjectedEmbedMetrics,
  embedScenePath: string,
  focusScenePath: string,
  previousDetail: DeepZoomDetail | null
): DeepZoomDetail {
  const detail: DeepZoomDetail = resolveDeepZoomDetail(metrics, previousDetail)
  if (detail === 'shell' && isOnFocusChain(embedScenePath, focusScenePath)) {
    return 'preview'
  }
  return detail
}

// ─── 焦点候选与滞回 ───

/**
 * 从候选里挑出"当前视觉焦点应该是谁"。
 *
 * 规则（#820 保留，#821 不改）：
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
 * 那条已经看不见的链会被继续保底，焦点参考根和用户实际在看的区域脱节。
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
 * 焦点只改变"显示参考根"：不改 Core、不新建页面、不换相机、不落盘，
 * 更不参与任何布局几何计算。
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
