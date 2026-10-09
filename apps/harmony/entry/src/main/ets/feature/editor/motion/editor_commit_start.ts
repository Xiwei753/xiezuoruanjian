// editor_commit_start.ts — 提交瞬间的运动起始状态计算（纯逻辑）。
//
// Issue #879 复核评论6075662695问题3：
// 旧实现找到一个「交集最多的旧窗口」后直接沿用它的 currentClipLeft/Right（绝对 vp 坐标）
// 作为新 run 的裁切起点。这在三种情况下都不成立：
// 1) 旧窗口跨折行被拆成多窗口、只剩一部分字属于新窗口；
// 2) 旧窗口大、目标窗口短——整段绝对 clip 会让新窗口瞬间多露字；
// 3) 旧动画在候选排版期间自然结束（冻结帧没有运动窗口），
//    起点却仍是 prepareCandidate 时刻保存的半裁切值。
//
// 正确做法是**按字形算**：逐个 glyphId 判断它此刻在屏上可见不可见、在哪里，
// 再把「可见的字形区间」按 run 自己的布局（caretStops）换算成 run 坐标系里的
// startClip / startPosition。可见性来源有两处，同一套逻辑覆盖：
// - 被冻结运动窗口覆盖的字形：可见区间 = 字形在屏区间 ∩ 窗口 clip；
// - 没有被任何运动窗口覆盖的字形：displayed 主文本静态绘制 → 完整可见
//   （旧动画结束后所有字形都属于这一类，自然解决情况 3）。
//
// 纯逻辑：不依赖 ArkUI、不 import .ets，生产由 Planner/Coordinator 调用，Node 单测直接 import。

import type { LineLayout } from '../render/editor_render_geometry.ts'
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

/** 可见字形段——run 簇序里的最长连续可见区间。 */
export interface VisibleGlyphRange {
  /** run 簇序里第一个可见簇的下标 */
  firstIndex: number
  /** run 簇序里最后一个可见簇的下标（含） */
  lastIndex: number
  /** 第一个可见簇在 run 自己布局里的左边界（vp） */
  ownLeft: number
  /** 最后一个可见簇在 run 自己布局里的右边界（vp） */
  ownRight: number
  /** 第一个可见簇此刻在屏的左边界（vp，含窗口平移） */
  onScreenLeft: number
  /** 第一个可见簇此刻在屏的上边界（vp） */
  onScreenTop: number
}

/** 可见性判定用的微小容差（vp）——避免浮点误差把零宽度可见判成不可见。 */
const VISIBLE_EPSILON = 0.01

/** 找 offset 所在的行（行区间 [startUtf16, endUtf16] 闭区间，与光标定位一致）。 */
function lineForOffset(layout: LineLayout[], offset: number): LineLayout | null {
  for (const line of layout) {
    if (offset >= line.startUtf16 && offset <= line.endUtf16) {
      return line
    }
  }
  return null
}

/** 取 offset 在一行布局里的 x（vp）：用 caretStops 找最近的前一个停止点。 */
function xAtOffset(layout: LineLayout[], offset: number, fallback: number): number {
  const line = lineForOffset(layout, offset)
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
function yAtOffset(layout: LineLayout[], offset: number, fallback: number): number {
  const line = lineForOffset(layout, offset)
  return line === null ? fallback : line.y
}

/**
 * 计算 run 里此刻在屏可见的字形段。
 *
 * @returns 最长的连续可见段；没有任何字形在屏可见时返回 null。
 */
export function visibleGlyphRange(run: RunGeometry, displayed: DisplayedContext): VisibleGlyphRange | null {
  const boundaries = clusterBoundaries(run.ownText, run.ownUtf16Start, run.ownUtf16End)
  const clusterCount = boundaries.length - 1
  if (clusterCount <= 0 || clusterCount !== run.glyphIds.length) {
    // 簇数与身份数对不上（范围不是整簇边界）——不猜几何，交给调用方用兜底起点。
    return null
  }

  const frozenByGlyphId = frozenIndex(displayed)

  let bestFirst = -1
  let bestLast = -1
  let bestLength = 0
  let currentFirst = -1
  for (let i = 0; i < clusterCount; i++) {
    const visible = clusterVisible(displayed, frozenByGlyphId, run.glyphIds[i]) !== null
    if (visible) {
      if (currentFirst < 0) {
        currentFirst = i
      }
      const length = i - currentFirst + 1
      if (length > bestLength) {
        bestLength = length
        bestFirst = currentFirst
        bestLast = i
      }
    } else {
      currentFirst = -1
    }
  }
  if (bestFirst < 0) {
    return null
  }

  const firstBoundary = boundaries[bestFirst]
  const lastBoundary = boundaries[bestLast + 1]
  const ownLeft = xAtOffset(run.ownLayout, firstBoundary, run.ownRect.x)
  const ownRight = xAtOffset(run.ownLayout, lastBoundary, run.ownRect.x + run.ownRect.width)
  const onScreen = clusterVisible(displayed, frozenByGlyphId, run.glyphIds[bestFirst])
  return {
    firstIndex: bestFirst,
    lastIndex: bestLast,
    ownLeft: ownLeft,
    ownRight: ownRight,
    onScreenLeft: onScreen === null ? ownLeft : onScreen.left,
    onScreenTop: onScreen === null ? yAtOffset(run.ownLayout, firstBoundary, run.ownRect.y) : onScreen.top,
  }
}

/** 出字 run 的起始状态：可见段在 run 自己布局里的区间；全不可见时从 run 左边界零宽度开始。 */
export function insertRunStartState(run: RunGeometry, displayed: DisplayedContext): RunStartState {
  const visible = visibleGlyphRange(run, displayed)
  const left = visible === null ? run.ownRect.x : visible.ownLeft
  const right = visible === null ? run.ownRect.x : visible.ownRight
  return {
    startClipLeft: left,
    startClipRight: right,
    startPositionX: run.ownRect.x,
    startPositionY: run.ownRect.y,
  }
}

/**
 * 吞字 run 的起始状态：仍可见的字形段在 run 自己布局里的区间。
 *
 * 已经全不可见（上一笔动画已经把它吞掉）时塌到 collapseX——
 * 起点＝终点，这一笔不再产生二次运动。
 */
export function deletedRunStartState(
  run: RunGeometry,
  displayed: DisplayedContext,
  collapseX: number
): RunStartState {
  const visible = visibleGlyphRange(run, displayed)
  const left = visible === null ? collapseX : visible.ownLeft
  const right = visible === null ? collapseX : visible.ownRight
  return {
    startClipLeft: left,
    startClipRight: right,
    startPositionX: run.ownRect.x,
    startPositionY: run.ownRect.y,
  }
}

/** 保留字平移 run 的起始状态：让第一个仍可见的字形停在此刻在屏的位置上。 */
export function retainedMoveStartState(
  run: RunGeometry,
  displayed: DisplayedContext,
  fallback: RunStartState
): RunStartState {
  const visible = visibleGlyphRange(run, displayed)
  if (visible === null) {
    return fallback
  }
  return {
    startClipLeft: run.ownRect.x,
    startClipRight: run.ownRect.x + run.ownRect.width,
    startPositionX: visible.onScreenLeft - (visible.ownLeft - run.ownRect.x),
    startPositionY: visible.onScreenTop,
  }
}

/** 某个字形此刻在屏的可见区间（vp）。 */
interface OnScreenInterval {
  left: number
  top: number
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
 * 判断一个字形此刻在屏是否可见，可见时给出它在屏区间。
 *
 * - 不在在屏正文里（身份表查不到这个 glyphId）→ 不可见：属于真正的新字；
 * - 在在屏正文里但被冻结窗口覆盖 → 可见区间 = 字形在屏区间 ∩ 窗口 clip；
 * - 在在屏正文里且没有被任何窗口覆盖 → displayed 主文本静态绘制 → 完整可见。
 */
function clusterVisible(
  displayed: DisplayedContext,
  frozenByGlyphId: Map<string, FrozenWindowSpan>,
  glyphId: string
): OnScreenInterval | null {
  const entry = displayed.identities === null
    ? null
    : displayed.identities.entryByGlyphId(glyphId)
  if (entry === null) {
    return null
  }
  const x0 = xAtOffset(displayed.layout, entry.utf16Start, 0)
  const x1 = xAtOffset(displayed.layout, entry.utf16End, x0)
  const span = frozenByGlyphId.get(glyphId)
  if (span === undefined) {
    // 没有被任何窗口覆盖：主文本静态绘制这一整段，字形完整可见。
    return { left: x0, top: yAtOffset(displayed.layout, entry.utf16Start, 0) }
  }
  // 被运动窗口接管：主文本不画它，可见性完全由窗口的当前 clip 决定。
  // 字形此刻在屏的位置 = 它在窗口源文本布局里的位置 + 窗口自身的平移量。
  const left = Math.max(x0 + span.offsetX, span.clipLeft)
  const right = Math.min(x1 + span.offsetX, span.clipRight)
  if (right - left <= VISIBLE_EPSILON) {
    return null
  }
  return { left: left, top: yAtOffset(displayed.layout, entry.utf16Start, 0) + span.offsetY }
}
