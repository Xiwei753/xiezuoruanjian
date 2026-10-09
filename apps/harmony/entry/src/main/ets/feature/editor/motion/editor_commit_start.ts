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
 * 对于被冻结窗口覆盖的字形：
 *   在屏可见区间 = max(字形在屏left, clipLeft) 到 min(字形在屏right, clipRight)
 *   换算回 run 坐标系 = max(字形在run布局left, clipLeft - offsetX) 到 min(字形在run布局right, clipRight - offsetX)
 * 对于静态可见的字形：ownLeft/ownRight = 字形在 run 自己布局中的完整边界。
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

  // 字形在 run 自己布局中的完整边界
  const ownFullLeft = xAtOffset(run.ownLayout, firstBoundary, run.ownRect.x)
  const ownFullRight = xAtOffset(run.ownLayout, lastBoundary, run.ownRect.x + run.ownRect.width)

  // 计算精确可见区间（含部分裁切）
  const firstGlyphId = run.glyphIds[firstIndex]
  const lastGlyphId = run.glyphIds[lastIndex]
  const lastVisible = clusterVisible(displayed, frozenByGlyphId, lastGlyphId)

  // 对于首字形的 ownLeft：如果被窗口覆盖，需要精确裁切
  const firstSpan = frozenByGlyphId.get(firstGlyphId)
  let ownLeft: number
  let onScreenLeft: number
  if (firstSpan !== undefined) {
    // 被窗口覆盖：精确裁切
    ownLeft = Math.max(ownFullLeft, firstSpan.clipLeft - firstSpan.offsetX)
    onScreenLeft = Math.max(firstVisible.left, firstSpan.clipLeft)
  } else {
    // 静态可见：完整边界
    ownLeft = ownFullLeft
    onScreenLeft = firstVisible.left
  }

  // 对于尾字形的 ownRight：如果被窗口覆盖，需要精确裁切
  let ownRight: number
  let onScreenRight: number
  if (lastVisible !== null) {
    const lastSpan = frozenByGlyphId.get(lastGlyphId)
    if (lastSpan !== undefined) {
      // 被窗口覆盖：精确裁切
      ownRight = Math.min(ownFullRight, lastSpan.clipRight - lastSpan.offsetX)
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
  }
}

/**
 * 出字 run 的起始状态：可见段在 run 自己布局里的区间；全不可见时从 run 左边界零宽度开始。
 *
 * 多 piece 时取所有 piece 的并集（最小 ownLeft 到最大 ownRight）。
 */
export function insertRunStartState(run: RunGeometry, displayed: DisplayedContext): RunStartState {
  const pieces = visibleGlyphPieces(run, displayed)
  if (pieces.length === 0) {
    return {
      startClipLeft: run.ownRect.x,
      startClipRight: run.ownRect.x,
      startPositionX: run.ownRect.x,
      startPositionY: run.ownRect.y,
    }
  }
  // 取所有 piece 的并集
  const left = Math.min(...pieces.map((p) => p.ownLeft))
  const right = Math.max(...pieces.map((p) => p.ownRight))
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
 * 多 piece 时取所有 piece 的并集（最小 ownLeft 到最大 ownRight）。
 */
export function deletedRunStartState(
  run: RunGeometry,
  displayed: DisplayedContext,
  collapseX: number
): RunStartState {
  const pieces = visibleGlyphPieces(run, displayed)
  if (pieces.length === 0) {
    return {
      startClipLeft: collapseX,
      startClipRight: collapseX,
      startPositionX: run.ownRect.x,
      startPositionY: run.ownRect.y,
    }
  }
  // 取所有 piece 的并集
  const left = Math.min(...pieces.map((p) => p.ownLeft))
  const right = Math.max(...pieces.map((p) => p.ownRight))
  return {
    startClipLeft: left,
    startClipRight: right,
    startPositionX: run.ownRect.x,
    startPositionY: run.ownRect.y,
  }
}

/**
 * 保留字平移 run 的起始状态：让第一个仍可见的字形停在此刻在屏的位置上。
 *
 * 多 piece 时取所有 piece 的并集作为可见区间，用第一个 piece 的在屏位置做平移校正。
 */
export function retainedMoveStartState(
  run: RunGeometry,
  displayed: DisplayedContext,
  fallback: RunStartState
): RunStartState {
  const pieces = visibleGlyphPieces(run, displayed)
  if (pieces.length === 0) {
    return fallback
  }
  // 取所有 piece 的并集作为可见区间
  const ownLeft = Math.min(...pieces.map((p) => p.ownLeft))
  const ownRight = Math.max(...pieces.map((p) => p.ownRight))
  // 用第一个 piece 的在屏位置做平移校正
  const firstPiece = pieces[0]
  return {
    startClipLeft: ownLeft,
    startClipRight: ownRight,
    startPositionX: firstPiece.onScreenLeft - (firstPiece.ownLeft - run.ownRect.x),
    startPositionY: firstPiece.onScreenTop,
  }
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
  // 1. 先查 displayed.identities
  const entry = displayed.identities === null
    ? null
    : displayed.identities.entryByGlyphId(glyphId)
  if (entry !== null) {
    const x0 = xAtOffset(displayed.layout, entry.utf16Start, 0)
    const x1 = xAtOffset(displayed.layout, entry.utf16End, x0)
    const y0 = yAtOffset(displayed.layout, entry.utf16Start, 0)
    const span = frozenByGlyphId.get(glyphId)
    if (span === undefined) {
      // 没有被任何窗口覆盖：主文本静态绘制这一整段，字形完整可见。
      return { left: x0, right: x1, top: y0, bottom: y0 + 20 }
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
      bottom: y0 + span.offsetY + 20,
    }
  }

  // 2. displayed 正文身份表中找不到 → 查冻结窗口（ghost字形）
  const span = frozenByGlyphId.get(glyphId)
  if (span !== undefined && span.glyphUtf16Ranges !== null && span.sourceLayout !== null) {
    // ghost字形：该 glyphId 已从 displayed 正文删除，但冻结窗口仍在绘制它
    const utf16Range = span.glyphUtf16Ranges.get(glyphId)
    if (utf16Range !== undefined) {
      // 用窗口的 sourceLayout 来获取字形在源修订正文中的像素位置
      // （ghost字形不在 displayed 正文中，不能用 displayed.layout；
      //   窗口的 sourceLayout 是窗口源文本的行布局，与 glyphUtf16Ranges 配合使用）
      const x0 = xAtOffset(span.sourceLayout, utf16Range[0], 0)
      const x1 = xAtOffset(span.sourceLayout, utf16Range[1], x0)
      const y0 = yAtOffset(span.sourceLayout, utf16Range[0], 0)
      const onScreenLeft = Math.max(x0 + span.offsetX, span.clipLeft)
      const onScreenRight = Math.min(x1 + span.offsetX, span.clipRight)
      if (onScreenRight - onScreenLeft <= VISIBLE_EPSILON) {
        return null
      }
      return {
        left: onScreenLeft,
        right: onScreenRight,
        top: y0 + span.offsetY,
        bottom: y0 + span.offsetY + 20,
      }
    }
  }

  // 3. 都找不到 → null（真正的新字）
  return null
}
