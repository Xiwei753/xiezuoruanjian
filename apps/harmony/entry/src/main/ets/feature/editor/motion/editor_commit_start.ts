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

  // ── 首字形 ──
  const firstSpan = frozenByGlyphId.get(firstGlyphId)
  let ownLeft: number
  let onScreenLeft: number
  let sourceGlyphX0: number
  let sourceGlyphX1: number
  let visiblePixelLeft: number
  let visiblePixelRight: number

  if (firstSpan !== undefined) {
    // 被窗口覆盖：计算源字形几何，然后投影到目标布局
    const sourceGeom = sourceGlyphGeometry(displayed, frozenByGlyphId, firstGlyphId)
    if (sourceGeom !== null) {
      sourceGlyphX0 = sourceGeom.x0
      sourceGlyphX1 = sourceGeom.x1
      // 源布局坐标系中的可见区间（裁切后，不含 offset）
      const clipLeftInSource = firstSpan.clipLeft - firstSpan.offsetX
      const clipRightInSource = firstSpan.clipRight - firstSpan.offsetX
      visiblePixelLeft = Math.max(sourceGlyphX0, clipLeftInSource)
      visiblePixelRight = Math.min(sourceGlyphX1, clipRightInSource)
      // 投影到目标布局（问题4）——按首字形的目标边界投影
      ownLeft = projectToTarget(visiblePixelLeft, sourceGlyphX0, sourceGlyphX1, firstGlyphTargetLeft, firstGlyphTargetRight)
    } else {
      // 无法获取源字形几何——退回完整边界
      sourceGlyphX0 = ownFullLeft
      sourceGlyphX1 = ownFullRight
      visiblePixelLeft = ownFullLeft
      visiblePixelRight = ownFullRight
      ownLeft = ownFullLeft
    }
    onScreenLeft = Math.max(firstVisible.left, firstSpan.clipLeft)
  } else {
    // 静态可见：完整边界（源 = 目标）
    sourceGlyphX0 = ownFullLeft
    sourceGlyphX1 = ownFullRight
    visiblePixelLeft = ownFullLeft
    visiblePixelRight = ownFullRight
    ownLeft = ownFullLeft
    onScreenLeft = firstVisible.left
  }

  // ── 尾字形 ──
  let ownRight: number
  let onScreenRight: number
  if (lastVisible !== null) {
    const lastSpan = frozenByGlyphId.get(lastGlyphId)
    if (lastSpan !== undefined) {
      // 被窗口覆盖：计算源字形几何，然后投影到目标布局
      const lastSourceGeom = sourceGlyphGeometry(displayed, frozenByGlyphId, lastGlyphId)
      if (lastSourceGeom !== null) {
        // 尾字形的源几何
        const lastSourceX0 = lastSourceGeom.x0
        const lastSourceX1 = lastSourceGeom.x1
        const lastClipLeftInSource = lastSpan.clipLeft - lastSpan.offsetX
        const lastClipRightInSource = lastSpan.clipRight - lastSpan.offsetX
        const lastVisiblePixelLeft = Math.max(lastSourceX0, lastClipLeftInSource)
        const lastVisiblePixelRight = Math.min(lastSourceX1, lastClipRightInSource)
        // 投影尾字形的可见右边界到目标布局——按尾字形的目标边界投影
        ownRight = projectToTarget(lastVisiblePixelRight, lastSourceX0, lastSourceX1, lastGlyphTargetLeft, lastGlyphTargetRight)
        // 更新 sourceGlyphX1 为尾字形的源右边界
        sourceGlyphX1 = lastSourceX1
        visiblePixelRight = lastVisiblePixelRight
      } else {
        ownRight = ownFullRight
      }
      onScreenRight = Math.min(lastVisible.right, lastSpan.clipRight)
    } else {
      // 静态可见：完整边界
      ownRight = ownFullRight
      onScreenRight = lastVisible.right
    }
  } else {
    // lastVisible 为 null 不应发生（lastIndex 是可见段的最后一个）
    ownRight = ownFullRight
    onScreenRight = onScreenLeft
  }

  return {
    firstIndex: firstIndex,
    lastIndex: lastIndex,
    ownLeft: ownLeft,
    ownRight: ownRight,
    onScreenLeft: onScreenLeft,
    onScreenTop: firstVisible.top,
    onScreenRight: onScreenRight,
    sourceGlyphX0: sourceGlyphX0,
    sourceGlyphX1: sourceGlyphX1,
    visiblePixelLeft: visiblePixelLeft,
    visiblePixelRight: visiblePixelRight,
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
  const pieces = visibleGlyphPieces(run, displayed)
  if (pieces.length === 0) {
    // 一个共享字形都不在屏上（旧帧已静态化且全部消失）→ 返回 fallback。
    // 覆盖整段 run 的 glyphIds，而不是只取第一个字形。
    return [{
      firstIndex: 0,
      lastIndex: run.glyphIds.length - 1,
      startClipLeft: fallback.startClipLeft,
      startClipRight: fallback.startClipRight,
      startPositionX: fallback.startPositionX,
      startPositionY: fallback.startPositionY,
      glyphIds: [...run.glyphIds],
    }]
  }
  return pieces.map((p: VisibleRunPiece): RunStartPiece => ({
    firstIndex: p.firstIndex,
    lastIndex: p.lastIndex,
    startClipLeft: p.ownLeft,
    startClipRight: p.ownRight,
    // 每个 piece 的 startPosition 让该 piece 的第一个可见字形停在此刻在屏的位置上
    startPositionX: p.onScreenLeft - (p.ownLeft - run.ownRect.x),
    startPositionY: p.onScreenTop,
    glyphIds: run.glyphIds.slice(p.firstIndex, p.lastIndex + 1),
  }))
}

/** 某个字形此刻在屏的精确可见区间（vp）。 */
interface OnScreenInterval {
  left: number
  right: number
  top: number
  bottom: number
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
      return { left: x0, right: x1, top: y0, bottom: y0 + glyphHeight }
    }
    // 被运动窗口接管：可见区间 = 字形在屏区间 ∩ 窗口 clip
    const onScreenLeft = Math.max(x0 + span.offsetX, span.clipLeft)
    const onScreenRight = Math.min(x1 + span.offsetX, span.clipRight)
    if (onScreenRight - onScreenLeft <= VISIBLE_EPSILON) {
      return null
    }
    return {
      left: onScreenLeft,
      right: onScreenRight,
      top: y0 + span.offsetY,
      bottom: y0 + span.offsetY + glyphHeight,
    }
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
      const onScreenLeft = Math.max(x0 + span.offsetX, span.clipLeft)
      const onScreenRight = Math.min(x1 + span.offsetX, span.clipRight)
      if (onScreenRight - onScreenLeft <= VISIBLE_EPSILON) {
        return null
      }
      return {
        left: onScreenLeft,
        right: onScreenRight,
        top: y0 + span.offsetY,
        bottom: y0 + span.offsetY + glyphHeight,
      }
    }
  }

  // 3. 都找不到 → null（真正的新字）
  return null
}
