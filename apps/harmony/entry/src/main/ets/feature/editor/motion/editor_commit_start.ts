// editor_commit_start.ts — 提交瞬间的运动起始状态计算（纯逻辑）。
//
// Issue #879 复核评论6077187962 的 3 个核心问题修复：
//
// 问题1（最严重）：部分裁切的字形在交棒时跳字
//   旧 clusterVisible() 只返回 {left, top} 或 null——只表达"可见/不可见"，
//   不表达精确可见区间。visibleGlyphRange() 用 ownLeft/ownRight 取字形的完整边界，
//   而不是部分可见的裁切区间。交棒瞬间从完整边界变成裁切区间会产生跳字。
//   修复：clusterVisible() 改为返回精确可见区间 {left, right, top, bottom}，
//   visibleGlyphPieces() 用精确可见区间计算 startClipLeft/Right。
//
// 问题2：不连续的多段可见字形被压成"最长一段"
//   旧 visibleGlyphRange() 用 bestFirst/bestLast/bestLength 只保留最长一段，
//   丢弃其他可见段。修复：改为 visibleGlyphPieces() 返回全部可见区间数组。
//
// 问题3：ghost字形（已删除但仍被运动窗口绘制）
//   旧 clusterVisible 先在 displayed revision 的身份表中查找，找不到立刻返回 null。
//   但删除动画中字形已从 displayed 正文删除，旧 sourceRevision 的运动窗口仍在绘制它。
//   修复：将"displayed 正文里存在的静态字形"和"仍持有独立 old sourceRevision 的
//   未完成运动字形"视为两个并列的可见来源。FrozenWindowSpan 增加 sourceRevision
//   和 glyphUtf16Ranges 字段，clusterVisible 优先查询冻结窗口是否提供还在绘制的 glyphId。
//
// 纯逻辑：不依赖 ArkUI、不 import .ets，生产由 Planner/Coordinator 调用，Node 单测直接 import。

import type { LineLayout } from '../render/editor_render_geometry.ts'
import { glyphRectForRange } from '../render/editor_render_geometry.ts'
import type { LineRange } from '../render/editor_layout_math.ts'
import { resolveVisualLineIndex, CaretAffinity } from '../render/editor_layout_math.ts'
import type { GlyphIdentityTable } from './editor_glyph_identity.ts'
import { clusterBoundaries } from './editor_glyph_identity.ts'

/** 冻结帧里一个运动窗口暴露给起始状态计算的信息（vp，content 坐标）。 */
export interface FrozenWindowSpan {
  /** 该窗口覆盖的稳定字形身份 */
  glyphIds: string[]
  /** 当前裁切左边界（绝对 vp） */
  clipLeft: number
  /** 当前裁切右边界（绝对 vp） */
  clipRight: number
  /**
   * 该窗口相对自身布局的平移量（vp）。
   *
   * 吐字/吞字窗口为 0（字形就在布局位置上，clip 只表达可见范围）；
   * 保留字平移窗口等于 currentPosition - glyph 基准位置——
   * 此时 clip 区间本身就是平移后的位置。两者都靠这个字段统一表达，
   * 不能靠「clipLeft - glyphX」反推（那会把部分裁切误当成平移）。
   */
  offsetX: number
  /** 该窗口相对自身布局的纵向平移量（vp） */
  offsetY: number
  /**
   * Issue #879 评论6096421590 问题2：该窗口在冻结帧的所有可见裁切矩形（多岛并集）。
   * 单岛场景退化为只有一个元素。用于 clusterVisible/buildPiece 对每个 clipRect 求交，
   * 避免只看包围盒 clipLeft/clipRight 把两个可见岛之间不可见的字误判为可见。
   */
  clipRects: RectLike[]
  /** 该窗口所属的源修订号（用于识别 ghost 字形的来源） */
  sourceRevision: number
  /**
   * 该窗口内各 glyphId 在源修订正文中的 UTF-16 区间映射（用于在窗口源文本布局中定位字形几何）。
   * null 表示窗口不提供此映射（窗口内字形都在 displayed 正文身份表中，无需 ghost 逻辑）。
   */
  glyphUtf16Ranges: Map<string, [number, number]> | null
  /**
   * 窗口源文本的行布局（用于在 ghost 字形几何计算中定位字形像素位置）。
   * null 表示窗口不提供源文本布局（窗口内字形都在 displayed 正文身份表中，无需 ghost 逻辑）。
   * 当 glyphUtf16Ranges 非 null 时，sourceLayout 必须也非 null，否则 ghost 字形无法定位几何。
   */
  sourceLayout: LineLayout[] | null
}

/** 计算起始状态时的「在屏」上下文。 */
export interface DisplayedContext {
  /** 在屏正文（displayed revision 的 text） */
  text: string
  /** 在屏行布局（displayed revision 的几何） */
  layout: LineLayout[]
  /** 在屏正文的字形身份表（glyphId → 在屏 UTF-16 区间）；null 表示身份不可信 */
  identities: GlyphIdentityTable | null
  /**
   * 冻结的运动窗口；**空数组合法**，表示旧动画已结束、屏幕上是完整静态正文，
   * 此时 displayed 正文里的字形全部完整可见。
   */
  frozenWindows: FrozenWindowSpan[]
}

/** 一个 run 的起始状态（在 run 自己的几何坐标系里）。 */
export interface RunStartState {
  startClipLeft: number
  startClipRight: number
  startPositionX: number
  startPositionY: number
}

/** run 参与几何计算的信息。 */
export interface RunGeometry {
  /** 该 run 携带的稳定字形身份（与 ownText 中该范围的字符簇一一对应） */
  glyphIds: string[]
  /** run 所在正文（吐字＝新正文，吞字/保留字＝旧正文） */
  ownText: string
  /** run 在自己的正文里的 UTF-16 区间 */
  ownUtf16Start: number
  ownUtf16End: number
  /** run 在自己的布局里的矩形（vp，content 坐标） */
  ownRect: RectLike
  /** run 自己的行布局 */
  ownLayout: LineLayout[]
}

/** 矩形（vp）——与 render 层几何同构，避免依赖 .ets 侧类型。 */
export interface RectLike {
  x: number
  y: number
  width: number
  height: number
}

/** 可见字形段——run 簇序里的一个连续可见区间（可能有多段）。 */
export interface VisibleRunPiece {
  /** run 簇序里第一个可见簇的下标 */
  firstIndex: number
  /** run 簇序里最后一个可见簇的下标（含） */
  lastIndex: number
  /** 第一个可见簇在 run 自己布局里的精确可见左边界（vp，含部分裁切） */
  ownLeft: number
  /** 最后一个可见簇在 run 自己布局里的精确可见右边界（vp，含部分裁切） */
  ownRight: number
  /** 第一个可见簇此刻在屏的精确可见左边界（vp，含窗口平移和裁切） */
  onScreenLeft: number
  /** 第一个可见簇此刻在屏的上边界（vp） */
  onScreenTop: number
  /** 最后一个可见簇此刻在屏的精确可见右边界（vp，含窗口平移和裁切） */
  onScreenRight: number
  /**
   * Issue #879 复核评论6078682695 问题4：首字形在**源布局**中的完整左边界（vp）。
   *
   * 源布局 = 冻结窗口的 sourceLayout（ghost 字形）或 displayed.layout（静态/被窗口覆盖的 displayed 字形）。
   * 与 ownLeft（run 自己布局里的投影后边界）不同——ownLeft 是投影到目标局部坐标系后的值，
   * sourceGlyphX0 是源布局坐标系里的原始值。用于跨字体/换行几何的可见比例投影。
   */
  sourceGlyphX0: number
  /** Issue #879 复核评论6078682695 问题4：尾字形在源布局中的完整右边界（vp） */
  sourceGlyphX1: number
  /**
   * Issue #879 复核评论6078682695 问题4：首字形在源布局中的真实可见左边界（vp，含裁切）。
   *
   * = max(sourceGlyphX0, clipLeft - offsetX)，在源布局坐标系里。
   * 与 onScreenLeft（含 offset 的在屏坐标）不同——visiblePixelLeft 不含 offset。
   */
  visiblePixelLeft: number
  /** Issue #879 复核评论6078682695 问题4：尾字形在源布局中的真实可见右边界（vp，含裁切） */
  visiblePixelRight: number
  /**
   * Issue #879 复核评论6099070438 问题4：该可见段的多段裁切子矩形（在 run 自己布局坐标系中）。
   *
   * buildPiece 把首/尾字形与冻结窗口 clipRects 的逐段交集投影保留下来，
   * 避免只取 min/max 把不连续可见压成连续区间。intervalForIsland 传递到
   * RunStartInterval.clipRects，buildFrame 据此逐段插值。
   * 缺省时回落到 [ownLeft, ownRight] 单段（legacy 语义）。
   */
  clipRects?: RectLike[]
  /**
   * Issue #879 复核评论6100665268 问题3：每段 clipRect 对应的源字形完整矩形
   * （在 run 自己布局坐标系中）。
   *
   * buildFrame retained 多矩形算法用此精确映射每段裁切矩形从冻结布局到目标 glyph
   * 对应边界。与 clipRects 一一对应；缺省时 buildFrame 回落到按比例分配（legacy 语义）。
   */
  sourceGlyphRects?: RectLike[]
}

/**
 * Issue #879 复核评论6078682695 问题1：一个 run 的一个可见片段的起始状态。
 *
 * 多段可见片段不再合并成连续矩形——每个 piece 各有独立的 startClipLeft/Right，
 * buildFrame 按 piece 分别采样，每个 piece 生成独立的 MotionGlyphWindow。
 * 区间之间的空洞绝不填补（丙被旧动画吞没时，新 plan 第一帧不会让丙冒出来）。
 */
export interface RunStartPiece {
  /** 该片段覆盖的 run 簇序起始下标（含） */
  firstIndex: number
  /** 该片段覆盖的 run 簇序末下标（含） */
  lastIndex: number
  /** 该片段的起始裁切左边界（vp，在 run 自己布局里） */
  startClipLeft: number
  /** 该片段的起始裁切右边界（vp，在 run 自己布局里） */
  startClipRight: number
  /** 该片段的起始位置 x（vp）——保留字平移用 */
  startPositionX: number
  /** 该片段的起始位置 y（vp）——保留字平移用 */
  startPositionY: number
  /** 该片段覆盖的 glyphId 子序列 */
  glyphIds: string[]
  /**
   * Issue #879 本轮复核评论（高优先级）：该 piece 内所有不连续可见岛的起始裁切区间列表。
   *
   * 固定 piece 分区下，交棒瞬间某字形可能被冻结窗口局部裁切而断开成多段；
   * 每一段各自独立计算 own/source/onScreen clip。buildFrame 据此为同一物理 Text
   * （同一 renderNodeKey）生成「多岛并集」clipRects，不丢任何可见字形。
   * 该字段为 undefined 或空时，buildFrame 回落到 piece 自身的单段 clip（legacy 语义）。
   */
  intervals?: RunStartInterval[]
}

/**
 * Issue #879 本轮复核评论（高优先级）：一个固定 piece 内的一段不连续可见岛的起始状态。
 *
 * 与 RunStartPiece 不同——RunStartPiece 是「按固定分区划分的稳定物理节点单元」，
 * 而 RunStartInterval 是「该 piece 在某时刻被冻结窗口局部裁切后实际露出的一个可见子段」，
 * 一个 piece 可能包含多个不连续 RunStartInterval。buildFrame 为每个 interval 各生成一段 clipRect，
 * 合成并集后由同一个物理 Text 绘制。
 */
export interface RunStartInterval {
  /** 该岛覆盖的 run 簇序起始下标（含） */
  firstIndex: number
  /** 该岛覆盖的 run 簇序末下标（含） */
  lastIndex: number
  /** 该岛覆盖的 glyphId 子序列 */
  glyphIds: string[]
  /** 该岛起始裁切左边界（vp，在 run 自己布局里） */
  startClipLeft: number
  /** 该岛起始裁切右边界（vp，在 run 自己布局里） */
  startClipRight: number
  /** 该岛起始位置 x（vp）——保留字平移用 */
  startPositionX: number
  /** 该岛起始位置 y（vp）——保留字平移用 */
  startPositionY: number
  /**
   * 该岛在 run 自己正文里的 UTF-16 区间（exclusive end）。
   *
   * 由 retargetRunStarts 在可见性已知的时刻用 clusterBoundaries 算出，
   * 供 computeStaticRangesMultiChannel 精确扣除「被运动层接管」的字形范围——
   * 只有真正落在可见岛里的字形才从静态层扣除，岛之间的空洞（被冻结窗口局部裁切断开、
   * 此刻仍不可见的字形）留在静态层，避免真实丢字。
   * 缺省（legacy/未填充）时调用方回落到整 piece/整 run 扣除。
   * 注：insert 通道 own 即新正文，本区间即新正文偏移；deleted/retained 通道 own 为旧正文，
   * 本区间仅用于旧正文侧几何，静态扣除对 deleted 不生效、对 retained 走 newUtf16 整段。
   */
  utf16Start?: number
  /** 该岛在 run 自己正文里的 UTF-16 结束 offset（exclusive） */
  utf16End?: number
  /**
   * Issue #879 复核评论6099070438 问题4：该岛的多段源/目标裁切子矩形（在 run 自己布局坐标系中）。
   *
   * 同一个字形的左右两块露出、中间有空洞时，不能用 [最左,最右] 连续区间——
   * 那会画出该字原本不可见的中间区域。buildPiece 把首/尾字形与冻结窗口 clipRects
   * 的逐段交集投影保留下来，buildFrame 从这些矩形逐段插值并作为一个稳定物理窗口
   * 的多个 clip 输出。
   * 缺省（legacy/未填充）时回落到 startClipLeft/startClipRight 单段语义。
   */
  clipRects?: RectLike[]
  /**
   * Issue #879 复核评论6100665268 问题3：每段 clipRect 对应的源字形完整矩形
   * （在 run 自己布局坐标系中）。
   *
   * buildFrame retained 多矩形算法用此精确映射每段裁切矩形从冻结布局到目标 glyph
   * 对应边界——不按起始可见宽度比例虚构 targetWidth，每段的最终几何来自字形真实
   * 源/目标矩形。clipRects 可以逐帧合并，但不能填平原本的间隙或溢出字形边界。
   * 与 clipRects 一一对应；缺省时 buildFrame 回落到按比例分配（legacy 语义）。
   */
  sourceGlyphRects?: RectLike[]
}

/** 可见性判定用的微小容差（vp）——避免浮点误差把零宽度可见判成不可见。 */
const VISIBLE_EPSILON = 0.01

/**
 * Issue #879 复核评论6082616112 问题4：找 offset 所在的行。
 *
 * 用 resolveVisualLineIndex + 指定 affinity 选行——
 * 软换行边界 offset 属于下一行（Downstream）或上一行（Upstream）。
 */
function lineForOffset(layout: LineLayout[], offset: number,
  affinity: CaretAffinity = CaretAffinity.Downstream): LineLayout | null {
  if (layout.length === 0) { return null }
  const lineRanges: LineRange[] = layout.map((l: LineLayout): LineRange => ({
    start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind,
  }))
  const idx = resolveVisualLineIndex(lineRanges, { utf16Offset: offset, affinity })
  return layout[idx] ?? null
}

/** 取 offset 在一行布局里的 x（vp）：用 caretStops 找最近的前一个停止点。 */
function xAtOffset(layout: LineLayout[], offset: number, fallback: number,
  affinity: CaretAffinity = CaretAffinity.Downstream): number {
  const line = lineForOffset(layout, offset, affinity)
  if (line === null) {
    return fallback
  }
  let x = line.left
  for (const stop of line.caretStops) {
    if (stop.utf16Offset <= offset) {
      x = stop.x
    }
  }
  return x
}

/** 取 offset 所在行的 y（vp）。 */
function yAtOffset(layout: LineLayout[], offset: number, fallback: number,
  affinity: CaretAffinity = CaretAffinity.Downstream): number {
  const line = lineForOffset(layout, offset, affinity)
  return line === null ? fallback : line.y
}

/**
 * 计算 run 里此刻在屏可见的全部字形段（可能有多段不连续的可见区间）。
 *
 * @returns 所有可见段数组；没有任何字形在屏可见时返回空数组。
 */
export function visibleGlyphPieces(run: RunGeometry, displayed: DisplayedContext): VisibleRunPiece[] {
  const boundaries = clusterBoundaries(run.ownText, run.ownUtf16Start, run.ownUtf16End)
  const clusterCount = boundaries.length - 1
  if (clusterCount <= 0 || clusterCount !== run.glyphIds.length) {
    // 簇数与身份数对不上（范围不是整簇边界）——不猜几何，交给调用方用兜底起点。
    return []
  }

  const frozenByGlyphId = frozenIndex(displayed)

  const pieces: VisibleRunPiece[] = []
  let currentFirst = -1
  let currentFirstVisible: OnScreenInterval | null = null

  for (let i = 0; i < clusterCount; i++) {
    const visible = clusterVisible(displayed, frozenByGlyphId, run.glyphIds[i])
    if (visible !== null) {
      if (currentFirst < 0) {
        currentFirst = i
        currentFirstVisible = visible
      }
    } else {
      if (currentFirst >= 0) {
        // 结束当前段
        const lastIdx = i - 1
        const piece = buildPiece(run, boundaries, currentFirst, lastIdx, currentFirstVisible, displayed, frozenByGlyphId)
        if (piece !== null) {
          pieces.push(piece)
        }
        currentFirst = -1
        currentFirstVisible = null
      }
    }
  }
  // 处理最后一段
  if (currentFirst >= 0) {
    const lastIdx = clusterCount - 1
    const piece = buildPiece(run, boundaries, currentFirst, lastIdx, currentFirstVisible, displayed, frozenByGlyphId)
    if (piece !== null) {
      pieces.push(piece)
    }
  }

  return pieces
}

/**
 * 构建一个可见段（piece）。
 *
 * ownLeft/ownRight 需要精确反映部分裁切后的可见宽度，而非字形的完整边界。
 *
 * Issue #879 复核评论6078682695 问题4：
 * 源窗口的裁切区间（冻结源布局的 vp 边界）不能直接与 run.ownLayout 的 ownFullLeft/Right
 * 做 max/min——源窗口和新目标 run 一旦因文字变更产生不同前缀宽度、折行或位置，
 * 二者不是同一局部坐标系，简单比较 vp 会导致首尾裁切错位。
 *
 * 修复：先计算首/尾字形在**源布局**中的完整边界 [sourceGlyphX0, sourceGlyphX1] 和
 * 裁切后的可见区间 [visiblePixelLeft, visiblePixelRight]（都在源布局坐标系里），
 * 再按可见比例投影到**目标 run 布局**的 [ownFullLeft, ownFullRight]：
 *   ratio = (visiblePixelLeft - sourceGlyphX0) / (sourceGlyphX1 - sourceGlyphX0)
 *   ownLeft = ownFullLeft + ratio * (ownFullRight - ownFullLeft)
 * 跨行必须分别处理（每个 piece 只在一行内），不能以原始全局 x 当新布局 x。
 */
/**
 * 缺口2修复：从 OnScreenInterval 的 rects 数组中取最左边的 x（vp，屏幕坐标）。
 */
function onScreenLeftOf(interval: OnScreenInterval): number {
  let min = Infinity
  for (const r of interval.rects) {
    if (r.x < min) { min = r.x }
  }
  return min
}

/**
 * 缺口2修复：从 OnScreenInterval 的 rects 数组中取最右边的 x+width（vp，屏幕坐标）。
 */
function onScreenRightOf(interval: OnScreenInterval): number {
  let max = -Infinity
  for (const r of interval.rects) {
    if (r.x + r.width > max) { max = r.x + r.width }
  }
  return max
}

/**
 * 缺口2修复：从 OnScreenInterval 的 rects 数组中取最上边的 y（vp，屏幕坐标）。
 */
function onScreenTopOf(interval: OnScreenInterval): number {
  let min = Infinity
  for (const r of interval.rects) {
    if (r.y < min) { min = r.y }
  }
  return min
}

function buildPiece(
  run: RunGeometry,
  boundaries: number[],
  firstIndex: number,
  lastIndex: number,
  firstVisible: OnScreenInterval,
  displayed: DisplayedContext,
  frozenByGlyphId: Map<string, FrozenWindowSpan>
): VisibleRunPiece | null {
  const firstBoundary = boundaries[firstIndex]
  const lastBoundary = boundaries[lastIndex + 1]

  // 字形在 run 自己布局中的完整边界（目标局部坐标系）
  // Issue #879 复核评论6082616112 问题4：start 用 Downstream、end 用 Upstream
  const ownFullLeft = xAtOffset(run.ownLayout, firstBoundary, run.ownRect.x)
  const ownFullRight = xAtOffset(run.ownLayout, lastBoundary, run.ownRect.x + run.ownRect.width,
    CaretAffinity.Upstream)

  // Issue #879 复核评论6078682695 问题4：
  // 首字形和尾字形各自在目标布局中的完整边界——投影时必须按各自字形的目标边界，
  // 不能用整个 piece 的 [ownFullLeft, ownFullRight]。
  // 否则多字形 piece 在源＝目标时会得到非恒等结果（首字形可见比例被映射到整段宽度）。
  const firstGlyphTargetLeft = ownFullLeft
  const firstGlyphTargetRight = xAtOffset(run.ownLayout, boundaries[firstIndex + 1], run.ownRect.x + run.ownRect.width,
    CaretAffinity.Upstream)
  const lastGlyphTargetLeft = xAtOffset(run.ownLayout, boundaries[lastIndex], run.ownRect.x)
  const lastGlyphTargetRight = ownFullRight

  const firstGlyphId = run.glyphIds[firstIndex]
  const lastGlyphId = run.glyphIds[lastIndex]
  const lastVisible = clusterVisible(displayed, frozenByGlyphId, lastGlyphId)

  // Issue #879 复核评论6100665268 问题3：同时收集 clipRect 和对应的源字形完整矩形。
  // 每段 clipRect 对应一个字形在 run 自己布局中的完整矩形（sourceGlyphRect），
  // buildFrame retained 用此精确映射每段裁切矩形到目标 glyph 对应边界。
  interface ClipWithSource { clip: RectLike; sourceRect: RectLike }

  // ── 首字形 ──
  const firstSpan = frozenByGlyphId.get(firstGlyphId)
  let ownLeft: number
  let onScreenLeft: number
  let sourceGlyphX0: number
  let sourceGlyphX1: number
  let visiblePixelLeft: number
  let visiblePixelRight: number
  // Issue #879 复核评论6099070438 问题4：首字形逐段交集投影收集器。
  const firstClipRects: RectLike[] = []
  const firstSourceGlyphRects: RectLike[] = []
  // 首字形在 run 自己布局中的完整矩形
  const firstGlyphFullRect: RectLike = {
    x: firstGlyphTargetLeft, y: run.ownRect.y,
    width: firstGlyphTargetRight - firstGlyphTargetLeft, height: run.ownRect.height,
  }

  if (firstSpan !== undefined) {
    // 被窗口覆盖：计算源字形几何，然后投影到目标布局
    const sourceGeom = sourceGlyphGeometry(displayed, frozenByGlyphId, firstGlyphId)
    if (sourceGeom !== null) {
      sourceGlyphX0 = sourceGeom.x0
      sourceGlyphX1 = sourceGeom.x1
      // 缺口2修复：逐段投影——对 firstVisible.rects 中每个 rect 分别投影，
      // 不能用 min/max 合并成连续区间。ownLeft = 最左边 rect 的投影左边界。
      let minProjectedLeft = Infinity
      let minVisLeft = Infinity
      let maxVisRight = -Infinity
      for (const r of firstVisible.rects) {
        const visLeftInSource = Math.max(sourceGlyphX0, r.x - firstSpan.offsetX)
        const visRightInSource = Math.min(sourceGlyphX1, r.x + r.width - firstSpan.offsetX)
        if (visRightInSource - visLeftInSource > VISIBLE_EPSILON) {
          const projectedLeft = projectToTarget(
            visLeftInSource, sourceGlyphX0, sourceGlyphX1,
            firstGlyphTargetLeft, firstGlyphTargetRight
          )
          // Issue #879 复核评论6099070438 问题4：逐段投影保留为 clipRect，
          // 不只用 min/max 压成连续区间——同一字形左右两块露出、中间有空洞时
          // 避免画出不可见的中间区域。
          const projectedRight = projectToTarget(
            visRightInSource, sourceGlyphX0, sourceGlyphX1,
            firstGlyphTargetLeft, firstGlyphTargetRight
          )
          firstClipRects.push({
            x: projectedLeft, y: run.ownRect.y,
            width: projectedRight - projectedLeft, height: run.ownRect.height,
          })
          firstSourceGlyphRects.push(firstGlyphFullRect)
          if (projectedLeft < minProjectedLeft) {
            minProjectedLeft = projectedLeft
          }
          if (visLeftInSource < minVisLeft) {
            minVisLeft = visLeftInSource
          }
          if (visRightInSource > maxVisRight) {
            maxVisRight = visRightInSource
          }
        }
      }
      if (minProjectedLeft === Infinity) {
        ownLeft = ownFullLeft
        visiblePixelLeft = sourceGlyphX0
        visiblePixelRight = sourceGlyphX0
      } else {
        ownLeft = minProjectedLeft
        visiblePixelLeft = minVisLeft
        visiblePixelRight = maxVisRight
      }
    } else {
      // 无法获取源字形几何——退回完整边界
      sourceGlyphX0 = ownFullLeft
      sourceGlyphX1 = ownFullRight
      visiblePixelLeft = ownFullLeft
      visiblePixelRight = ownFullRight
      ownLeft = ownFullLeft
      firstClipRects.push({
        x: ownFullLeft, y: run.ownRect.y,
        width: firstGlyphTargetRight - firstGlyphTargetLeft, height: run.ownRect.height,
      })
      firstSourceGlyphRects.push(firstGlyphFullRect)
    }
    // 缺口2修复：onScreenLeft 从 firstVisible.rects 中取最左边的 x
    onScreenLeft = onScreenLeftOf(firstVisible)
  } else {
    // 静态可见：完整边界（源 = 目标）
    sourceGlyphX0 = ownFullLeft
    sourceGlyphX1 = ownFullRight
    visiblePixelLeft = ownFullLeft
    visiblePixelRight = ownFullRight
    ownLeft = ownFullLeft
    onScreenLeft = onScreenLeftOf(firstVisible)
    firstClipRects.push({
      x: firstGlyphTargetLeft, y: run.ownRect.y,
      width: firstGlyphTargetRight - firstGlyphTargetLeft, height: run.ownRect.height,
    })
    firstSourceGlyphRects.push(firstGlyphFullRect)
  }

  // ── 尾字形 ──
  let ownRight: number
  let onScreenRight: number
  // Issue #879 复核评论6099070438 问题4：尾字形逐段交集投影收集器。
  const lastClipRects: RectLike[] = []
  const lastSourceGlyphRects: RectLike[] = []
  // 尾字形在 run 自己布局中的完整矩形
  const lastGlyphFullRect: RectLike = {
    x: lastGlyphTargetLeft, y: run.ownRect.y,
    width: lastGlyphTargetRight - lastGlyphTargetLeft, height: run.ownRect.height,
  }
  if (lastVisible !== null) {
    const lastSpan = frozenByGlyphId.get(lastGlyphId)
    if (lastSpan !== undefined) {
      // 被窗口覆盖：计算源字形几何，然后投影到目标布局
      const lastSourceGeom = sourceGlyphGeometry(displayed, frozenByGlyphId, lastGlyphId)
      if (lastSourceGeom !== null) {
        const lastSourceX0 = lastSourceGeom.x0
        const lastSourceX1 = lastSourceGeom.x1
        // 缺口2修复：逐段投影——对 lastVisible.rects 中每个 rect 分别投影，
        // ownRight = 最右边 rect 的投影右边界。
        let maxProjectedRight = -Infinity
        let minVisLeft = Infinity
        let maxVisRight = -Infinity
        for (const r of lastVisible.rects) {
          const visLeftInSource = Math.max(lastSourceX0, r.x - lastSpan.offsetX)
          const visRightInSource = Math.min(lastSourceX1, r.x + r.width - lastSpan.offsetX)
          if (visRightInSource - visLeftInSource > VISIBLE_EPSILON) {
            const projectedLeft = projectToTarget(
              visLeftInSource, lastSourceX0, lastSourceX1,
              lastGlyphTargetLeft, lastGlyphTargetRight
            )
            const projectedRight = projectToTarget(
              visRightInSource, lastSourceX0, lastSourceX1,
              lastGlyphTargetLeft, lastGlyphTargetRight
            )
            // Issue #879 复核评论6099070438 问题4：逐段投影保留为 clipRect。
            lastClipRects.push({
              x: projectedLeft, y: run.ownRect.y,
              width: projectedRight - projectedLeft, height: run.ownRect.height,
            })
            lastSourceGlyphRects.push(lastGlyphFullRect)
            if (projectedRight > maxProjectedRight) {
              maxProjectedRight = projectedRight
            }
            if (visLeftInSource < minVisLeft) {
              minVisLeft = visLeftInSource
            }
            if (visRightInSource > maxVisRight) {
              maxVisRight = visRightInSource
            }
          }
        }
        if (maxProjectedRight === -Infinity) {
          ownRight = ownFullRight
        } else {
          ownRight = maxProjectedRight
          sourceGlyphX1 = lastSourceX1
          visiblePixelRight = maxVisRight
        }
      } else {
        ownRight = ownFullRight
        lastClipRects.push({
          x: lastGlyphTargetLeft, y: run.ownRect.y,
          width: lastGlyphTargetRight - lastGlyphTargetLeft, height: run.ownRect.height,
        })
        lastSourceGlyphRects.push(lastGlyphFullRect)
      }
      // 缺口2修复：onScreenRight 从 lastVisible.rects 中取最右边的 x+width
      onScreenRight = onScreenRightOf(lastVisible)
    } else {
      // 静态可见：完整边界
      ownRight = ownFullRight
      onScreenRight = onScreenRightOf(lastVisible)
      lastClipRects.push({
        x: lastGlyphTargetLeft, y: run.ownRect.y,
        width: lastGlyphTargetRight - lastGlyphTargetLeft, height: run.ownRect.height,
      })
      lastSourceGlyphRects.push(lastGlyphFullRect)
    }
  } else {
    // lastVisible 为 null 不应发生（lastIndex 是可见段的最后一个）
    ownRight = ownFullRight
    onScreenRight = onScreenLeft
  }

  // Issue #879 复核评论6100665268 问题4：合并首/尾 clipRects 为该可见段的多段裁切子矩形。
  // - 首尾同字形（单字形 piece）：只用 firstClipRects（首尾是同一字形的可见区间）。
  // - 首尾不同字形：首字形可见段 + 中间逐字形可见段 + 尾字形可见段。
  //
  // 旧实现把 firstIndex..lastIndex 之间的中间字形直接画一个完整矩形
  // (midLeft=firstGlyphTargetRight → midRight=lastGlyphTargetLeft)，
  // 假设中间字形完整可见。实际上连续可见的中间字形也可能只是部分显示
  // （例如三个字形都可见，但中间的"乙"仅显示左半）。
  // 因为 visibleGlyphPieces 只按"是否有非空交集"分段，buildPiece 直接画满乙，
  // 会重新露出乙的隐藏部分。
  //
  // 修复：遍历 firstIndex..lastIndex 的每个 glyphId，从 clusterVisible(...).rects
  // 获得所有真实二维可见子矩形，逐 glyph 投影到 run.ownLayout（用 projectToTarget），
  // 再组装 clipRects。不能假设中间字形完整可见。
  const pieceClipRects: RectLike[] = []
  const pieceSourceGlyphRects: RectLike[] = []
  if (firstIndex === lastIndex) {
    pieceClipRects.push(...firstClipRects)
    pieceSourceGlyphRects.push(...firstSourceGlyphRects)
  } else {
    pieceClipRects.push(...firstClipRects)
    pieceSourceGlyphRects.push(...firstSourceGlyphRects)
    // 中间字形逐个处理——不能假设完整可见
    for (let i = firstIndex + 1; i < lastIndex; i++) {
      const midGlyphId = run.glyphIds[i]
      const midVisible = clusterVisible(displayed, frozenByGlyphId, midGlyphId)
      if (midVisible === null) {
        // 中间字形不可见——跳过，不产生 clipRect（不填补空洞）
        continue
      }
      // 该字形在 run 自己布局中的完整边界
      const midGlyphTargetLeft = xAtOffset(run.ownLayout, boundaries[i], run.ownRect.x)
      const midGlyphTargetRight = xAtOffset(run.ownLayout, boundaries[i + 1],
        run.ownRect.x + run.ownRect.width, CaretAffinity.Upstream)
      const midGlyphFullRect: RectLike = {
        x: midGlyphTargetLeft, y: run.ownRect.y,
        width: midGlyphTargetRight - midGlyphTargetLeft, height: run.ownRect.height,
      }
      const midSpan = frozenByGlyphId.get(midGlyphId)
      if (midSpan !== undefined) {
        // 被窗口覆盖：逐段投影
        const midSourceGeom = sourceGlyphGeometry(displayed, frozenByGlyphId, midGlyphId)
        if (midSourceGeom !== null) {
          for (const r of midVisible.rects) {
            const visLeftInSource = Math.max(midSourceGeom.x0, r.x - midSpan.offsetX)
            const visRightInSource = Math.min(midSourceGeom.x1, r.x + r.width - midSpan.offsetX)
            if (visRightInSource - visLeftInSource > VISIBLE_EPSILON) {
              const projectedLeft = projectToTarget(
                visLeftInSource, midSourceGeom.x0, midSourceGeom.x1,
                midGlyphTargetLeft, midGlyphTargetRight
              )
              const projectedRight = projectToTarget(
                visRightInSource, midSourceGeom.x0, midSourceGeom.x1,
                midGlyphTargetLeft, midGlyphTargetRight
              )
              pieceClipRects.push({
                x: projectedLeft, y: run.ownRect.y,
                width: projectedRight - projectedLeft, height: run.ownRect.height,
              })
              pieceSourceGlyphRects.push(midGlyphFullRect)
            }
          }
        } else {
          // 无法获取源字形几何——退回完整边界
          pieceClipRects.push({
            x: midGlyphTargetLeft, y: run.ownRect.y,
            width: midGlyphTargetRight - midGlyphTargetLeft, height: run.ownRect.height,
          })
          pieceSourceGlyphRects.push(midGlyphFullRect)
        }
      } else {
        // 静态可见：完整边界（源 = 目标）
        pieceClipRects.push({
          x: midGlyphTargetLeft, y: run.ownRect.y,
          width: midGlyphTargetRight - midGlyphTargetLeft, height: run.ownRect.height,
        })
        pieceSourceGlyphRects.push(midGlyphFullRect)
      }
    }
    pieceClipRects.push(...lastClipRects)
    pieceSourceGlyphRects.push(...lastSourceGlyphRects)
  }

  return {
    firstIndex: firstIndex,
    lastIndex: lastIndex,
    ownLeft: ownLeft,
    ownRight: ownRight,
    onScreenLeft: onScreenLeft,
    onScreenTop: onScreenTopOf(firstVisible),
    onScreenRight: onScreenRight,
    sourceGlyphX0: sourceGlyphX0,
    sourceGlyphX1: sourceGlyphX1,
    visiblePixelLeft: visiblePixelLeft,
    visiblePixelRight: visiblePixelRight,
    clipRects: pieceClipRects.length > 0 ? pieceClipRects : undefined,
    sourceGlyphRects: pieceSourceGlyphRects.length > 0 ? pieceSourceGlyphRects : undefined,
  }
}

/**
 * Issue #879 复核评论6078682695 问题4：把源布局坐标系中的可见边界投影到目标布局。
 *
 * 当源和目标布局不同（不同前缀宽度、折行或位置）时，不能直接套原坐标。
 * 按可见比例投影：
 *   ratio = (sourceVisible - sourceX0) / (sourceX1 - sourceX0)
 *   target = targetX0 + ratio * (targetX1 - targetX0)
 *
 * 源宽度为零时退回 targetX0（退化情况）。
 */
function projectToTarget(
  sourceVisible: number,
  sourceX0: number,
  sourceX1: number,
  targetX0: number,
  targetX1: number
): number {
  const sourceWidth = sourceX1 - sourceX0
  if (sourceWidth <= VISIBLE_EPSILON) {
    return targetX0
  }
  const ratio = (sourceVisible - sourceX0) / sourceWidth
  return targetX0 + ratio * (targetX1 - targetX0)
}

/**
 * Issue #879 复核评论6078682695 问题4：获取一个字形在**源布局**中的完整几何。
 *
 * 两个来源：
 * 1. 字形在 displayed 正文身份表中 → 用 displayed.layout 计算（源 = displayed 布局）
 * 2. ghost 字形（不在 displayed 身份表，但冻结窗口仍在绘制）→ 用窗口的 sourceLayout 计算
 *
 * 返回的 x0/x1 是源布局坐标系里的值（不含窗口 offset），用于投影计算。
 */
function sourceGlyphGeometry(
  displayed: DisplayedContext,
  frozenByGlyphId: Map<string, FrozenWindowSpan>,
  glyphId: string
): { x0: number, x1: number, y0: number, height: number } | null {
  // Issue #879 复核评论6083352210 问题4：用 glyphRectForRange 取得精确矩形，
  // 确保 end offset 用 Upstream affinity 选行，并获取真实行高。
  // 1. 先查 displayed.identities
  const entry = displayed.identities === null
    ? null
    : displayed.identities.entryByGlyphId(glyphId)
  if (entry !== null) {
    const rect = glyphRectForRange(displayed.layout, entry.utf16Start, entry.utf16End)
    if (rect !== null) {
      return { x0: rect.x, x1: rect.x + rect.width, y0: rect.y, height: rect.height }
    }
    return null
  }

  // 2. ghost 字形：用窗口的 sourceLayout
  const span = frozenByGlyphId.get(glyphId)
  if (span !== undefined && span.glyphUtf16Ranges !== null && span.sourceLayout !== null) {
    const utf16Range = span.glyphUtf16Ranges.get(glyphId)
    if (utf16Range !== undefined) {
      const rect = glyphRectForRange(span.sourceLayout, utf16Range[0], utf16Range[1])
      if (rect !== null) {
        return { x0: rect.x, x1: rect.x + rect.width, y0: rect.y, height: rect.height }
      }
    }
  }

  return null
}

/**
 * 出字 run 的起始状态：每个可见段各有一个独立的起始裁切区间。
 *
 * Issue #879 复核评论6078682695 问题1：
 * 多段可见片段不再合并成连续矩形——每个 piece 各有独立 startClipLeft/Right，
 * buildFrame 按 piece 分别采样，每个 piece 生成独立的 MotionGlyphWindow。
 * 区间之间的空洞绝不填补（丙被旧动画吞没时，新 plan 第一帧不会让丙冒出来）。
 *
 * 全不可见时返回单个零宽度 piece（从 run 左边界开始）。
 */
export function insertRunStartState(run: RunGeometry, displayed: DisplayedContext): RunStartPiece[] {
  const pieces = visibleGlyphPieces(run, displayed)
  if (pieces.length === 0) {
    // 无旧可见字形（一次插入的所有字都不在旧画面里——粘贴多字、一次 IME 上屏多个字）：
    // 产生覆盖整段新 run 的合法 piece，从零宽度逐步吐出到这段最终的完整 clip。
    return [{
      firstIndex: 0,
      lastIndex: run.glyphIds.length - 1,
      startClipLeft: run.ownRect.x,
      startClipRight: run.ownRect.x,
      startPositionX: run.ownRect.x,
      startPositionY: run.ownRect.y,
      glyphIds: [...run.glyphIds],
    }]
  }

  // 混合 run（部分旧可见+真正新插入）：必须同时包含原可见片段与尚未出现的新片段。
  // visibleGlyphPieces 返回的片段列表可能有间隙（如 piece1 覆盖 index 0-1，piece2 覆盖 index 3-4，
  // 但 index 2 是新插入的字没有被任何 piece 覆盖），这些间隙作为 0 宽度的新 piece 加入。
  //
  // Issue #879 复核评论6080604353 问题2：
  // 间隙 gap piece 的 startClipLeft/Right/startPositionX 必须用 gap 自身在 ownLayout 中的
  // 真正左边界（通过 clusterBoundaries + xAtOffset 计算），而不是整 run 的 ownRect.x。
  // 否则混合插入中间/尾部的零宽度 gap piece 从整 run 左端启动，会短暂露出别的字。
  const boundaries = clusterBoundaries(run.ownText, run.ownUtf16Start, run.ownUtf16End)
  const result: RunStartPiece[] = []
  let prevLastIndex = -1

  for (const p of pieces) {
    // 间隙区域（新插入的字）作为 0 宽度 piece 加入
    if (p.firstIndex > prevLastIndex + 1) {
      const gapFirst = prevLastIndex + 1
      const gapLast = p.firstIndex - 1
      // gap 自身在 run 布局中的真正左边界，而不是整 run 的 ownRect.x
      const gapLeft = boundaries.length > 0
        ? xAtOffset(run.ownLayout, boundaries[gapFirst], run.ownRect.x)
        : run.ownRect.x
      result.push({
        firstIndex: gapFirst,
        lastIndex: gapLast,
        startClipLeft: gapLeft,
        startClipRight: gapLeft,
        startPositionX: gapLeft,
        startPositionY: run.ownRect.y,
        glyphIds: run.glyphIds.slice(gapFirst, gapLast + 1),
      })
    }
    // 添加当前可见 piece
    result.push({
      firstIndex: p.firstIndex,
      lastIndex: p.lastIndex,
      startClipLeft: p.ownLeft,
      startClipRight: p.ownRight,
      startPositionX: run.ownRect.x,
      startPositionY: run.ownRect.y,
      glyphIds: run.glyphIds.slice(p.firstIndex, p.lastIndex + 1),
    })
    prevLastIndex = p.lastIndex
  }

  // 尾部间隙：如果最后一个 piece 的 lastIndex < glyphIds.length - 1
  if (prevLastIndex < run.glyphIds.length - 1) {
    const gapFirst = prevLastIndex + 1
    const gapLast = run.glyphIds.length - 1
    // 尾部 gap 同样用自身在 run 布局中的真正左边界
    const gapLeft = boundaries.length > 0
      ? xAtOffset(run.ownLayout, boundaries[gapFirst], run.ownRect.x)
      : run.ownRect.x
    result.push({
      firstIndex: gapFirst,
      lastIndex: gapLast,
      startClipLeft: gapLeft,
      startClipRight: gapLeft,
      startPositionX: gapLeft,
      startPositionY: run.ownRect.y,
      glyphIds: run.glyphIds.slice(gapFirst, gapLast + 1),
    })
  }

  return result
}

/**
 * 吞字 run 的起始状态：每个仍可见的字形段各有独立的起始裁切区间。
 *
 * Issue #879 复核评论6078682695 问题1：
 * 多段可见片段不再合并成连续矩形——每个 piece 各有独立 startClipLeft/Right。
 *
 * 已经全不可见（上一笔动画已经把它吞掉）时塌到 collapseX——
 * 起点＝终点，这一笔不再产生二次运动。
 */
export function deletedRunStartState(
  run: RunGeometry,
  displayed: DisplayedContext,
  collapseX: number
): RunStartPiece[] {
  const pieces = visibleGlyphPieces(run, displayed)
  if (pieces.length === 0) {
    // 全不可见（上一笔动画已经把它吞掉）时塌到 collapseX——
    // 起点＝终点，这一笔不再产生二次运动。
    // 覆盖整段 run 的 glyphIds，而不是只取第一个字形。
    return [{
      firstIndex: 0,
      lastIndex: run.glyphIds.length - 1,
      startClipLeft: collapseX,
      startClipRight: collapseX,
      startPositionX: run.ownRect.x,
      startPositionY: run.ownRect.y,
      glyphIds: [...run.glyphIds],
    }]
  }
  return pieces.map((p: VisibleRunPiece): RunStartPiece => ({
    firstIndex: p.firstIndex,
    lastIndex: p.lastIndex,
    startClipLeft: p.ownLeft,
    startClipRight: p.ownRight,
    startPositionX: run.ownRect.x,
    startPositionY: run.ownRect.y,
    glyphIds: run.glyphIds.slice(p.firstIndex, p.lastIndex + 1),
  }))
}

/**
 * 保留字平移 run 的起始状态：每个仍可见的字形段各有独立的起始位置。
 *
 * Issue #879 复核评论6078682695 问题1：
 * 多段可见片段不再合并成连续矩形——每个 piece 各有独立 startClipLeft/Right 和 startPosition。
 * 让每个 piece 的第一个仍可见的字形停在此刻在屏的位置上。
 *
 * 一个共享字形都不在屏上（旧帧已静态化且全部消失）→ 返回 fallback。
 */
export function retainedMoveStartState(
  run: RunGeometry,
  displayed: DisplayedContext,
  fallback: RunStartState
): RunStartPiece[] {
  // Issue #879 复核评论6100665268 问题1：retained 的固定物理窗口分区必须覆盖
  // run.glyphIds 的全部字符簇，与 prepare 时可不可见无关。
  //
  // 旧实现用 visibleGlyphPieces 只返回当时可见的字形段，不可见字形没有 interval。
  // 如果 prepare 时整段不可见，回退成覆盖全 run、没有 intervals 的 piece。
  // 此时 plannedWindowKeys 按整 run glyphIds 规划 1 个 key；若等待窗口期间旧动画
  // 露出局部字形，下一次 retargetRunStarts(kind='retained') 因 piece.intervals
  // 不存在走 allVisibleIslands，生成若干小 interval.glyphIds，随后 plannedWindowKeys
  // 和 buildFrame 就变成另一组 key——物理 Text 节点身份不稳定。
  //
  // 修复：只有一个 piece 覆盖整个 run（firstIndex=0, lastIndex=glyphIds.length-1），
  // 其 intervals 覆盖所有字形——按完整 [0..glyphIds.length-1] 每个 glyph 固定一个
  // interval（firstIndex=lastIndex=i）。可见字形用 intervalForIsland 计算正确的
  // clip/position/clipRects；不可见字形使用显式零宽/空裁切（startClipLeft=startClipRight），
  // 但 interval 分区保留。retargetRunStarts(kind='retained') 永远只刷新这份已冻结分区
  // 内的 startClip/startPosition/clipRects，不增删 glyphIds key。
  const boundaries = clusterBoundaries(run.ownText, run.ownUtf16Start, run.ownUtf16End)
  const frozenByGlyphId = frozenIndex(displayed)
  const intervals: RunStartInterval[] = []
  let legacyStartClipLeft = fallback.startClipLeft
  let legacyStartClipRight = fallback.startClipRight
  let legacyStartPositionX = fallback.startPositionX
  let legacyStartPositionY = fallback.startPositionY
  let legacyResolved = false

  for (let i = 0; i < run.glyphIds.length; i++) {
    const interval = intervalForIsland(
      run, boundaries, i, i, 'retained', displayed, frozenByGlyphId
    )
    if (interval !== null) {
      intervals.push(interval)
      // legacy 单段起始取第一个可见 interval 的值
      if (!legacyResolved) {
        legacyStartClipLeft = interval.startClipLeft
        legacyStartClipRight = interval.startClipRight
        legacyStartPositionX = interval.startPositionX
        legacyStartPositionY = interval.startPositionY
        legacyResolved = true
      }
    } else {
      // 不可见字形：保留分区，使用显式零宽/空裁切
      const glyphX = boundaries.length > i
        ? xAtOffset(run.ownLayout, boundaries[i], run.ownRect.x)
        : run.ownRect.x
      // Issue #879 复核评论6100665268 问题3：不可见字形的 sourceGlyphRects 保留完整字形矩形，
      // clipRects 设为零宽度。buildFrame retained 多矩形算法据此在字形重新可见时
      // 精确映射裁切矩形到目标 glyph 对应边界。
      const glyphFullWidth = boundaries.length > i + 1
        ? xAtOffset(run.ownLayout, boundaries[i + 1], run.ownRect.x + run.ownRect.width,
          CaretAffinity.Upstream) - glyphX
        : 0
      intervals.push({
        firstIndex: i,
        lastIndex: i,
        glyphIds: [run.glyphIds[i]],
        startClipLeft: glyphX,
        startClipRight: glyphX,
        startPositionX: glyphX,
        startPositionY: run.ownRect.y,
        utf16Start: boundaries.length > i ? boundaries[i] : run.ownUtf16Start,
        utf16End: boundaries.length > i + 1 ? boundaries[i + 1] : run.ownUtf16End,
        clipRects: [{
          x: glyphX, y: run.ownRect.y,
          width: 0, height: run.ownRect.height,
        }],
        sourceGlyphRects: [{
          x: glyphX, y: run.ownRect.y,
          width: glyphFullWidth, height: run.ownRect.height,
        }],
      })
    }
  }

  return [{
    firstIndex: 0,
    lastIndex: run.glyphIds.length - 1,
    startClipLeft: legacyStartClipLeft,
    startClipRight: legacyStartClipRight,
    startPositionX: legacyStartPositionX,
    startPositionY: legacyStartPositionY,
    glyphIds: [...run.glyphIds],
    intervals: intervals.length > 0 ? intervals : undefined,
  }]
}

/**
 * Issue #879 复核评论6088787701 问题4：piece 分区在 retarget 之间保持不变，只重算起始数据。
 *
 * 物理 Text 节点的身份由 piece 的 glyphIds 序列决定（WindowInstanceIdAllocator）。
 * 旧 retarget 用提交瞬间的可见性**重新拆分** piece，于是同一个字形在两个时刻可能落到
 * 不同的 glyphIds 序列上 → 分配出不同 renderNodeKey → 交棒前刚准备完的节点作废，
 * 旧 animator 已 cancel 而新节点还没布局，形成「一直等、一直不交棒」。
 *
 * 修复：分区（firstIndex/lastIndex/glyphIds）在 prepare 时由可见性确定一次，
 * retarget 只重算该 piece 的起始 clip/position 数据。同一个字形在两次 retarget 之间
 * 不会因为临时可见分组改变而更换物理 Text key。
 *
 * 数据语义：
 * - piece 内仍有可见字形：为该 piece 内**所有不连续可见段**各算一段 RunStartInterval
 *   （intervals 字段），每段独立起始裁切；legacy 单段字段（startClipLeft/Right/Position）
 *   回落取最长一段，供未读 intervals 的代码兼容。piece 之间的空洞依旧不填补——
 *   每条 clip 只覆盖一段连续区间，但一个 piece 可以有多条。
 * - piece 内字形已全部不可见：insert 用该 piece 自身左边界零宽度、deleted 塌到 collapseX、
 *   retained 用 fallback——与 prepare 时同语义，不猜几何。
 *
 * @param run 该 piece 所属的 run 几何
 * @param pieces prepare 时确定的分区（边界与 glyphIds 保持不变）
 * @param displayed 提交瞬间的在屏上下文
 * @param kind 通道类别，决定全不可见时的兜底语义
 * @param collapseX deleted 通道全不可见时的塌缩位置（vp）
 * @param fallback retained 通道全不可见时的兜底起始状态
 * @returns 与入参 piece 一一对应的新 piece 数组（同 firstIndex/lastIndex/glyphIds）
 */
export function retargetRunStarts(
  run: RunGeometry,
  pieces: RunStartPiece[],
  displayed: DisplayedContext,
  kind: RunStartKind,
  collapseX: number,
  fallback: RunStartState
): RunStartPiece[] {
  const boundaries = clusterBoundaries(run.ownText, run.ownUtf16Start, run.ownUtf16End)
  if (boundaries.length !== run.glyphIds.length + 1) {
    // 簇数与身份数对不上（范围不是整簇边界）——不猜几何，保留原起始数据。
    return clonePieces(pieces)
  }
  const frozenByGlyphId = frozenIndex(displayed)

  const result: RunStartPiece[] = []
  for (const piece of pieces) {
    const pieceOwnLeft = xAtOffset(run.ownLayout, boundaries[piece.firstIndex], run.ownRect.x)
    const intervals: RunStartInterval[] = []
    // Issue #879 复核评论6099070438 问题1：retained 通道保留 prepare 阶段的稳定 intervals 分区
    // （firstIndex/lastIndex/glyphIds 不变），只重算每个 interval 的 clip/position。
    // 不调 allVisibleIslands 重新拆分——同一字形在两次 retarget 之间不会因临时可见分组
    // 改变而更换物理 Text key。如果某个 interval 的字形在 retarget 时不可见，
    // 将其 clip 设为零宽度（startClipLeft=startClipRight），分区仍保留。
    if (kind === 'retained' && piece.intervals !== undefined && piece.intervals.length > 0) {
      for (const prevInterval of piece.intervals) {
        const reInterval = intervalForIsland(
          run, boundaries, prevInterval.firstIndex, prevInterval.lastIndex,
          'retained', displayed, frozenByGlyphId
        )
        if (reInterval !== null) {
          intervals.push(reInterval)
        } else {
          // 该 interval 的字形在 retarget 时不可见——保留分区，clip 设为零宽度
          const clipX = prevInterval.startClipLeft
          intervals.push({
            firstIndex: prevInterval.firstIndex,
            lastIndex: prevInterval.lastIndex,
            glyphIds: [...prevInterval.glyphIds],
            startClipLeft: clipX,
            startClipRight: clipX,
            startPositionX: prevInterval.startPositionX,
            startPositionY: prevInterval.startPositionY,
            utf16Start: prevInterval.utf16Start,
            utf16End: prevInterval.utf16End,
            // Issue #879 复核评论6100665268 问题3：不可见时 clipRects 设为零宽度，
            // 保留分区但不露出任何可见区域。
            clipRects: [{
              x: clipX, y: prevInterval.startPositionY,
              width: 0, height: run.ownRect.height,
            }],
            // 保留 prevInterval.sourceGlyphRects 的引用即可（不可见时不需要重新计算）。
            sourceGlyphRects: prevInterval.sourceGlyphRects,
          })
        }
      }
    } else {
      // insert/deleted 通道：计算该 piece 内所有不连续可见岛，
      // 每段各自独立起始裁切——保留字停在各自在屏位置、吞字各自缩到 collapseX、
      // 吐字各自从零宽度吐出。不丢任何可见字形（旧 longestVisibleIsland 只保留最长一段）。
      const islands = allVisibleIslands(
        run, boundaries, piece.firstIndex, piece.lastIndex, displayed, frozenByGlyphId
      )
      for (const island of islands) {
        const interval = intervalForIsland(
          run, boundaries, island.firstIndex, island.lastIndex, kind, displayed, frozenByGlyphId
        )
        if (interval !== null) {
          intervals.push(interval)
        }
      }

      // 缺口4：insert 通道为不可见连续子区间（空洞）建立零宽度起始 interval。
      // 空洞里的新字不从 run 左边界吐出，而是从前一个可见岛的右边界旁开始展开，
      // 避免空洞里的新字直到动画结束才出现。
      if (kind === 'insert') {
        const zeroWidthIntervals: RunStartInterval[] = []
        let cursor = piece.firstIndex
        let prevClipRight: number | null = null
        for (const island of islands) {
          if (island.firstIndex > cursor) {
            const gapFirst = cursor
            const gapLast = island.firstIndex - 1
            const gapClipX = prevClipRight !== null ? prevClipRight : pieceOwnLeft
            const gapStartX = prevClipRight !== null ? prevClipRight : piece.startPositionX
            // Issue #879 复核评论6100665268 问题3：空洞的 sourceGlyphRects 保留完整字形矩形，
            // clipRects 设为零宽度，保持数据结构一致性。
            const gapGlyphLeft = xAtOffset(run.ownLayout, boundaries[gapFirst], run.ownRect.x)
            const gapGlyphRight = xAtOffset(run.ownLayout, boundaries[gapLast + 1],
              run.ownRect.x + run.ownRect.width, CaretAffinity.Upstream)
            zeroWidthIntervals.push({
              firstIndex: gapFirst,
              lastIndex: gapLast,
              glyphIds: run.glyphIds.slice(gapFirst, gapLast + 1),
              startClipLeft: gapClipX,
              startClipRight: gapClipX,
              startPositionX: gapStartX,
              startPositionY: piece.startPositionY,
              utf16Start: boundaries[gapFirst],
              utf16End: boundaries[gapLast + 1],
              clipRects: [{
                x: gapClipX, y: piece.startPositionY,
                width: 0, height: run.ownRect.height,
              }],
              sourceGlyphRects: [{
                x: gapGlyphLeft, y: piece.startPositionY,
                width: gapGlyphRight - gapGlyphLeft, height: run.ownRect.height,
              }],
            })
          }
          const islandInterval = intervals.find(iv => iv.firstIndex === island.firstIndex)
          if (islandInterval !== undefined) {
            prevClipRight = islandInterval.startClipRight
          }
          cursor = island.lastIndex + 1
        }
        if (cursor <= piece.lastIndex) {
          const gapFirst = cursor
          const gapLast = piece.lastIndex
          const gapClipX = prevClipRight !== null ? prevClipRight : pieceOwnLeft
          const gapStartX = prevClipRight !== null ? prevClipRight : piece.startPositionX
          // Issue #879 复核评论6100665268 问题3：尾部空洞的 sourceGlyphRects 保留完整字形矩形，
          // clipRects 设为零宽度，保持数据结构一致性。
          const gapGlyphLeft = xAtOffset(run.ownLayout, boundaries[gapFirst], run.ownRect.x)
          const gapGlyphRight = xAtOffset(run.ownLayout, boundaries[gapLast + 1],
            run.ownRect.x + run.ownRect.width, CaretAffinity.Upstream)
          zeroWidthIntervals.push({
            firstIndex: gapFirst,
            lastIndex: gapLast,
            glyphIds: run.glyphIds.slice(gapFirst, gapLast + 1),
            startClipLeft: gapClipX,
            startClipRight: gapClipX,
            startPositionX: gapStartX,
            startPositionY: piece.startPositionY,
            utf16Start: boundaries[gapFirst],
            utf16End: boundaries[gapLast + 1],
            clipRects: [{
              x: gapClipX, y: piece.startPositionY,
              width: 0, height: run.ownRect.height,
            }],
            sourceGlyphRects: [{
              x: gapGlyphLeft, y: piece.startPositionY,
              width: gapGlyphRight - gapGlyphLeft, height: run.ownRect.height,
            }],
          })
        }
        if (zeroWidthIntervals.length > 0) {
          intervals.push(...zeroWidthIntervals)
          intervals.sort((a, b) => a.firstIndex - b.firstIndex)
        }
      }
    }

    // legacy 单段起始（最长岛）保持原语义，供未读 intervals 的代码兼容。
    // 缺口4：零宽度 interval（空洞）不参与 legacy 最长岛选取——
    // 它们只为动画层提供独立 clip，不应影响兼容单段 clip 的语义。
    let startClipLeft = pieceOwnLeft
    let startClipRight = pieceOwnLeft
    let startPositionX = piece.startPositionX
    let startPositionY = piece.startPositionY
    let resolved = false
    if (intervals.length > 0) {
      let longest: RunStartInterval | null = null
      for (const iv of intervals) {
        if (iv.startClipRight - iv.startClipLeft <= VISIBLE_EPSILON) {
          continue
        }
        if (longest === null) {
          longest = iv
          continue
        }
        const len = iv.lastIndex - iv.firstIndex
        const bestLen = longest.lastIndex - longest.firstIndex
        if (len > bestLen || (len === bestLen && iv.firstIndex < longest.firstIndex)) {
          longest = iv
        }
      }
      if (longest !== null) {
        startClipLeft = longest.startClipLeft
        startClipRight = longest.startClipRight
        startPositionX = longest.startPositionX
        startPositionY = longest.startPositionY
        resolved = true
      }
    }
    if (!resolved) {
      // 该 piece 的字形此刻全部不可见——按通道语义塌缩，不猜几何。
      if (kind === 'deleted') {
        startClipLeft = collapseX
        startClipRight = collapseX
      } else if (kind === 'retained') {
        startClipLeft = fallback.startClipLeft
        startClipRight = fallback.startClipRight
        startPositionX = fallback.startPositionX
        startPositionY = fallback.startPositionY
      }
    }
    result.push({
      firstIndex: piece.firstIndex,
      lastIndex: piece.lastIndex,
      startClipLeft: startClipLeft,
      startClipRight: startClipRight,
      startPositionX: startPositionX,
      startPositionY: startPositionY,
      glyphIds: [...piece.glyphIds],
      intervals: intervals.length > 0 ? intervals : undefined,
    })
  }
  return result
}

/**
 * Issue #879 本轮复核评论（高优先级）：在 piece 的字母下标区间内找**所有**连续可见段。
 *
 * 与已删除的 longestVisibleIsland 不同——这里返回区间内每一段连续可见的下标对，
 * 段与段之间的空洞（被冻结窗口局部裁切断开）各自作为独立岛返回，
 * 交棒时每段都生成一段独立 clip，不丢任何可见字形。
 */
function allVisibleIslands(
  run: RunGeometry,
  boundaries: number[],
  firstIndex: number,
  lastIndex: number,
  displayed: DisplayedContext,
  frozenByGlyphId: Map<string, FrozenWindowSpan>
): Array<{ firstIndex: number, lastIndex: number }> {
  const islands: Array<{ firstIndex: number, lastIndex: number }> = []
  let currentFirst = -1
  for (let i = firstIndex; i <= lastIndex; i++) {
    const visible = clusterVisible(displayed, frozenByGlyphId, run.glyphIds[i])
    if (visible !== null) {
      if (currentFirst < 0) {
        currentFirst = i
      }
    } else if (currentFirst >= 0) {
      islands.push({ firstIndex: currentFirst, lastIndex: i - 1 })
      currentFirst = -1
    }
  }
  if (currentFirst >= 0) {
    islands.push({ firstIndex: currentFirst, lastIndex: lastIndex })
  }
  return islands
}

/**
 * Issue #879 本轮复核评论（高优先级）：为一段连续可见岛计算其起始裁切区间。
 *
 * 复用 buildPiece 计算该岛在 run 自己布局里的精确可见边界 ownLeft/ownRight，
 * 以及（保留字）在屏起始位置。insert/deleted 通道不平移，起始位置落到 run 左/上边界；
 * retained 通道让该岛第一个仍可见字形停在此刻在屏位置。
 *
 * @returns 该岛的 RunStartInterval；岛内无任何字形可见时返回 null。
 */
function intervalForIsland(
  run: RunGeometry,
  boundaries: number[],
  firstIndex: number,
  lastIndex: number,
  kind: RunStartKind,
  displayed: DisplayedContext,
  frozenByGlyphId: Map<string, FrozenWindowSpan>
): RunStartInterval | null {
  const firstVisible = clusterVisible(displayed, frozenByGlyphId, run.glyphIds[firstIndex])
  if (firstVisible === null) {
    return null
  }
  const visible = buildPiece(run, boundaries, firstIndex, lastIndex, firstVisible, displayed, frozenByGlyphId)
  if (visible === null) {
    return null
  }
  let startPositionX = run.ownRect.x
  let startPositionY = run.ownRect.y
  if (kind === 'retained') {
    // 让该岛第一个仍可见字形停在此刻在屏的位置上。
    startPositionX = visible.onScreenLeft - (visible.ownLeft - run.ownRect.x)
    startPositionY = visible.onScreenTop
  }
  return {
    firstIndex: firstIndex,
    lastIndex: lastIndex,
    glyphIds: run.glyphIds.slice(firstIndex, lastIndex + 1),
    startClipLeft: visible.ownLeft,
    startClipRight: visible.ownRight,
    startPositionX: startPositionX,
    startPositionY: startPositionY,
    // 该岛在 run 自己正文里的 UTF-16 区间——供 computeStaticRangesMultiChannel 精确扣除。
    utf16Start: boundaries[firstIndex],
    utf16End: boundaries[lastIndex + 1],
    // Issue #879 复核评论6099070438 问题4：传递 buildPiece 收集的多段裁切子矩形，
    // buildFrame 据此逐段插值，避免把不连续可见压成连续区间。
    clipRects: visible.clipRects,
    // Issue #879 复核评论6100665268 问题3：传递每段 clipRect 对应的源字形完整矩形，
    // buildFrame retained 多矩形算法用此精确映射每段裁切矩形到目标 glyph 对应边界。
    sourceGlyphRects: visible.sourceGlyphRects,
  }
}

/** piece 起始状态的通道类别。 */
export type RunStartKind = 'insert' | 'deleted' | 'retained'

/** 复制 pieces（分区不变时的兜底返回）。 */
function clonePieces(pieces: RunStartPiece[]): RunStartPiece[] {
  return pieces.map((p: RunStartPiece): RunStartPiece => ({
    firstIndex: p.firstIndex,
    lastIndex: p.lastIndex,
    startClipLeft: p.startClipLeft,
    startClipRight: p.startClipRight,
    startPositionX: p.startPositionX,
    startPositionY: p.startPositionY,
    glyphIds: [...p.glyphIds],
    intervals: p.intervals === undefined
      ? undefined
      : p.intervals.map((iv: RunStartInterval): RunStartInterval => ({
        firstIndex: iv.firstIndex,
        lastIndex: iv.lastIndex,
        glyphIds: [...iv.glyphIds],
        startClipLeft: iv.startClipLeft,
        startClipRight: iv.startClipRight,
        startPositionX: iv.startPositionX,
        startPositionY: iv.startPositionY,
        utf16Start: iv.utf16Start,
        utf16End: iv.utf16End,
        // Issue #879 复核评论6099070438 问题4：深拷贝 clipRects，避免共享引用。
        clipRects: iv.clipRects === undefined
          ? undefined
          : iv.clipRects.map((cr: RectLike): RectLike => ({
            x: cr.x, y: cr.y, width: cr.width, height: cr.height,
          })),
        // Issue #879 复核评论6100665268 问题3：深拷贝 sourceGlyphRects，避免共享引用。
        sourceGlyphRects: iv.sourceGlyphRects === undefined
          ? undefined
          : iv.sourceGlyphRects.map((sr: RectLike): RectLike => ({
            x: sr.x, y: sr.y, width: sr.width, height: sr.height,
          })),
      })),
  }))
}

// Issue #879 本轮复核评论（高优先级）：longestVisibleIsland 已被 allVisibleIslands +
// intervalForIsland 取代——固定 piece 分区下需返回该 piece 内所有不连续可见岛，
// 而非仅最长一段，否则其余可见段会被裁切层丢弃而永远无法归还静态层。

/**
 * 某个字形此刻在屏的精确可见区间（vp，屏幕坐标）。
 *
 * 缺口2修复：改为支持多个不连续的可见矩形。不同视觉行即使 x 相同，
 * 只要 y 完全不相交也不应被误认为可见；两个矩形裁中同一字形的不同不连续
 * 横向部分时，各自作为独立 rect 保留，不用 min/max 合并成连续区间。
 */
interface OnScreenInterval {
  /** 各段不连续可见矩形（vp，屏幕坐标） */
  rects: RectLike[]
}

/** 冻结窗口按字形身份索引：一个字形只可能落在一个窗口里（窗口区间互不重叠）。 */
function frozenIndex(displayed: DisplayedContext): Map<string, FrozenWindowSpan> {
  const index: Map<string, FrozenWindowSpan> = new Map()
  for (const span of displayed.frozenWindows) {
    for (const glyphId of span.glyphIds) {
      if (!index.has(glyphId)) {
        index.set(glyphId, span)
      }
    }
  }
  return index
}

/**
 * 判断一个字形此刻在屏是否可见，可见时给出它的精确可见区间。
 *
 * 三个可见来源，按优先级查询：
 * 1. 在 displayed 正文身份表中找到 → 按现有逻辑计算（静态或被窗口覆盖）
 * 2. 在 displayed 正文身份表中找不到 → 查冻结窗口是否包含该 glyphId → ghost字形
 * 3. 都找不到 → null（真正的新字）
 *
 * 对于被冻结窗口覆盖的字形：
 *   可见区间 = max(字形在屏left, clipLeft) 到 min(字形在屏right, clipRight)
 *   字形在屏位置 = 字形在 displayed 正文布局中的位置 + 窗口的 offsetX/offsetY
 *
 * 对于 ghost字形（已从 displayed 正文删除，但冻结窗口仍在绘制）：
 *   用窗口的 glyphUtf16Ranges + 窗口的 sourceLayout（displayed.layout 近似）计算字形几何
 *   可见区间 = max(字形在屏left, clipLeft) 到 min(字形在屏right, clipRight)
 *   字形在屏位置 = 字形在窗口源文本布局中的位置 + 窗口的 offsetX/offsetY
 */
function clusterVisible(
  displayed: DisplayedContext,
  frozenByGlyphId: Map<string, FrozenWindowSpan>,
  glyphId: string
): OnScreenInterval | null {
  // Issue #879 复核评论6083352210 问题4：用 glyphRectForRange 取得精确矩形，
  // 确保 end offset 用 Upstream affinity 选行，并获取真实行高（替代硬编码 20）。
  // 1. 先查 displayed.identities
  const entry = displayed.identities === null
    ? null
    : displayed.identities.entryByGlyphId(glyphId)
  if (entry !== null) {
    const rect = glyphRectForRange(displayed.layout, entry.utf16Start, entry.utf16End)
    if (rect === null) {
      return null
    }
    const x0 = rect.x
    const x1 = rect.x + rect.width
    const y0 = rect.y
    const glyphHeight = rect.height
    const span = frozenByGlyphId.get(glyphId)
    if (span === undefined) {
      // 没有被任何窗口覆盖：主文本静态绘制这一整段，字形完整可见。
      return { rects: [{ x: x0, y: y0, width: x1 - x0, height: glyphHeight }] }
    }
    // 被运动窗口接管：对每个 clipRect 求交（多岛并集）
    // Issue #879 评论6096421590 问题2：不能只看包围盒 clipLeft/clipRight，
    // 否则两个可见岛之间不可见的字会被误判为可见。
    // 缺口2修复：同时检查 x 和 y 交集，不合并不连续区间——
    // 每个 clipRect 与字形矩形的交集作为独立的 rect 保留。
    const glyphOnScreenLeft = x0 + span.offsetX
    const glyphOnScreenRight = x1 + span.offsetX
    const glyphOnScreenTop = y0 + span.offsetY
    const glyphOnScreenBottom = y0 + span.offsetY + glyphHeight

    const rects: RectLike[] = []
    for (const cr of span.clipRects) {
      const intersectLeft = Math.max(glyphOnScreenLeft, cr.x)
      const intersectRight = Math.min(glyphOnScreenRight, cr.x + cr.width)
      const intersectTop = Math.max(glyphOnScreenTop, cr.y)
      const intersectBottom = Math.min(glyphOnScreenBottom, cr.y + cr.height)
      if (intersectRight - intersectLeft > VISIBLE_EPSILON &&
          intersectBottom - intersectTop > VISIBLE_EPSILON) {
        rects.push({
          x: intersectLeft,
          y: intersectTop,
          width: intersectRight - intersectLeft,
          height: intersectBottom - intersectTop,
        })
      }
    }
    if (rects.length === 0) {
      return null
    }
    return { rects }
  }

  // 2. displayed 正文身份表中找不到 → 查冻结窗口（ghost字形）
  const span = frozenByGlyphId.get(glyphId)
  if (span !== undefined && span.glyphUtf16Ranges !== null && span.sourceLayout !== null) {
    // ghost字形：该 glyphId 已从 displayed 正文删除，但冻结窗口仍在绘制它
    const utf16Range = span.glyphUtf16Ranges.get(glyphId)
    if (utf16Range !== undefined) {
      const rect = glyphRectForRange(span.sourceLayout, utf16Range[0], utf16Range[1])
      if (rect === null) {
        return null
      }
      const x0 = rect.x
      const x1 = rect.x + rect.width
      const y0 = rect.y
      const glyphHeight = rect.height
      // 缺口2修复：ghost 分支也按 clipRects 求交（不再只用 clipLeft/clipRight），
      // 同时检查 x 和 y 交集，不合并不连续区间。
      const glyphOnScreenLeft = x0 + span.offsetX
      const glyphOnScreenRight = x1 + span.offsetX
      const glyphOnScreenTop = y0 + span.offsetY
      const glyphOnScreenBottom = y0 + span.offsetY + glyphHeight

      const rects: RectLike[] = []
      for (const cr of span.clipRects) {
        const intersectLeft = Math.max(glyphOnScreenLeft, cr.x)
        const intersectRight = Math.min(glyphOnScreenRight, cr.x + cr.width)
        const intersectTop = Math.max(glyphOnScreenTop, cr.y)
        const intersectBottom = Math.min(glyphOnScreenBottom, cr.y + cr.height)
        if (intersectRight - intersectLeft > VISIBLE_EPSILON &&
            intersectBottom - intersectTop > VISIBLE_EPSILON) {
          rects.push({
            x: intersectLeft,
            y: intersectTop,
            width: intersectRight - intersectLeft,
            height: intersectBottom - intersectTop,
          })
        }
      }
      if (rects.length === 0) {
        return null
      }
      return { rects }
    }
  }

  // 3. 都找不到 → null（真正的新字）
  return null
}
