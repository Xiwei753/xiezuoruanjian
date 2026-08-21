// editor_layout_math.ts — 编辑器折行、命中测试与布局类型的纯数学模块。
//
// Issue #629 评论18：统一 layout source。
// 所有 offset↔x 算法、行导航、命中测试、光标渲染共享同一套类型和纯函数。
// 不依赖 ArkUI / EditorLayoutSnapshot，只依赖 string/number/Array。
//
// 三个核心概念：
// 1. LineRange + LineBreakKind：一行的 UTF-16 code unit 范围 + 折行类型。
// 2. CaretStop：行内 code point 边界 + x 坐标。由 buildLineCaretStops() 生成。
// 3. VisualCaretPosition + CaretAffinity：soft-wrap 边界上的视觉光标位置。
//
// 所有 offset 是 UTF-16 code unit offset（ArkTS string.length 语义）。

// ── 类型定义 ──

/** 行结束类型。 */
export const LineBreakKind = {
  SoftWrap: 'softWrap',
  HardBreak: 'hardBreak',
  EndOfText: 'endOfText'
} as const
export type LineBreakKind = typeof LineBreakKind[keyof typeof LineBreakKind]

/** soft-wrap 边界上的光标亲和性。 */
export const CaretAffinity = {
  Upstream: 'upstream',
  Downstream: 'downstream'
} as const
export type CaretAffinity = typeof CaretAffinity[keyof typeof CaretAffinity]

/** 带亲和性的光标位置。 */
export interface VisualCaretPosition {
  readonly utf16Offset: number
  readonly affinity: CaretAffinity
}

/** 一行的 UTF-16 code unit 范围 + 折行类型。 */
export interface LineRange {
  readonly start: number
  readonly end: number
  readonly breakKind: LineBreakKind
}

/** 行内光标停靠点：code point 边界 + 对应 x 坐标（px）。 */
export interface CaretStop {
  readonly utf16Offset: number
  readonly x: number
}

// ── 纯函数 ──

/**
 * 返回 code unit index i 后的下一个 code point 边界。
 * surrogate pair 推进 2，否则推进 1。i >= text.length 时返回 text.length。
 */
export function nextCodePointBoundary(text: string, i: number): number {
  const n = text.length
  if (i >= n) {
    return n
  }
  if (i < 0) {
    i = 0
  }
  const code = text.charCodeAt(i)
  if (code >= 0xD800 && code <= 0xDBFF && i + 1 < n) {
    const low = text.charCodeAt(i + 1)
    if (low >= 0xDC00 && low <= 0xDFFF) {
      return i + 2
    }
  }
  return i + 1
}

/**
 * Issue #629 R7-C item5: 为 [start, end) 区间生成 code point 边界列表。
 * 每个 code point 只扫描一次，不再先构造全文 bounds 再 filter。
 */
function buildSegmentBounds(text: string, start: number, end: number): number[] {
  const bounds: number[] = [start]
  let i = start
  while (i < end) {
    i = Math.min(nextCodePointBoundary(text, i), end)
    bounds.push(i)
  }
  return bounds
}

/**
 * 折行：先按 `\n` 拆硬换行 segment，再对每个 segment 做软折行。
 * `\n` 本身不进可见 line range；空行产生空行。
 *
 * - 空文本返回 [{ start: 0, end: 0, breakKind: EndOfText }]
 * - 硬换行时不共享 offset：上一行 end 在 `\n` 前，下一行 start = end + 1。
 * - 软折行时共享 offset：{ start: a, end: b, SoftWrap } / { start: b, end: c, EndOfText }
 * - surrogate pair 不切断。
 * - containerWidth <= 0 时每个 code point 一行（退化但确定）。
 */
export function layoutLines(
  text: string,
  containerWidth: number,
  measureTextFn: (s: string) => number,
): LineRange[] {
  const n = text.length
  if (n === 0) {
    return [{ start: 0, end: 0, breakKind: LineBreakKind.EndOfText }]
  }

  // 按 \n 拆硬换行 segment
  const segments: { start: number; end: number }[] = []
  let segStart = 0
  for (let i = 0; i < n; i++) {
    if (text.charCodeAt(i) === 0x0A) {
      segments.push({ start: segStart, end: i })
      segStart = i + 1
    }
  }
  segments.push({ start: segStart, end: n })

  // Issue #629 R7-C item5: 按 segment 就地生成 bounds，扫描 \n 时即时处理 [segStart,i)，
  // 最后处理尾段。避免换行越多重复全文扫描的 allBounds + filter 结构。
  const lines: LineRange[] = []

  for (let s = 0; s < segments.length; s++) {
    const seg = segments[s]
    const isLastSegment = s === segments.length - 1

    if (seg.start === seg.end) {
      lines.push({ start: seg.start, end: seg.end, breakKind: isLastSegment ? LineBreakKind.EndOfText : LineBreakKind.HardBreak })
      continue
    }

    const segBounds = buildSegmentBounds(text, seg.start, seg.end)
    if (segBounds.length === 0) {
      continue
    }

    let pos = 0
    const lastPos = segBounds.length - 1

    while (pos < lastPos) {
      const lineStart = segBounds[pos]
      let lo = pos + 1
      let hi = lastPos
      let bestIdx = pos + 1
      while (lo <= hi) {
        const mid = Math.floor((lo + hi) / 2)
        const w = measureTextFn(text.substring(lineStart, segBounds[mid]))
        if (w <= containerWidth) {
          bestIdx = mid
          lo = mid + 1
        } else {
          hi = mid - 1
        }
      }
      const lineEnd = segBounds[bestIdx]
      const isLastLineInSeg = bestIdx === lastPos
      const breakKind: LineBreakKind = isLastLineInSeg
        ? (isLastSegment ? LineBreakKind.EndOfText : LineBreakKind.HardBreak)
        : LineBreakKind.SoftWrap
      lines.push({ start: lineStart, end: lineEnd, breakKind })
      pos = bestIdx
    }
  }

  return lines
}

/**
 * 给定一行文本和 measureTextFn，生成该行所有 code point 边界的 caret stops。
 */
export function buildLineCaretStops(
  text: string,
  line: { start: number; end: number },
  measureTextFn: (s: string) => number,
): CaretStop[] {
  const stops: CaretStop[] = []
  let i = line.start
  while (i <= line.end) {
    const x = measureTextFn(text.substring(line.start, i))
    stops.push({ utf16Offset: i, x })
    if (i >= line.end) {
      break
    }
    i = nextCodePointBoundary(text, i)
  }
  return stops
}

/** 纯函数：给定 caret stops 和 UTF-16 offset，返回该 offset 的 x 坐标（px）。 */
export function horizontalForOffset(stops: CaretStop[], utf16Offset: number): number {
  if (stops.length === 0) {
    return 0
  }
  let lo = 0, hi = stops.length - 1, bestIdx = 0
  let bestDist = Math.abs(stops[0].utf16Offset - utf16Offset)
  while (lo <= hi) {
    const mid = Math.floor((lo + hi) / 2)
    const dist = Math.abs(stops[mid].utf16Offset - utf16Offset)
    if (dist < bestDist || (dist === bestDist && stops[mid].utf16Offset <= utf16Offset)) {
      bestDist = dist
      bestIdx = mid
    }
    if (stops[mid].utf16Offset < utf16Offset) { lo = mid + 1 }
    else if (stops[mid].utf16Offset > utf16Offset) { hi = mid - 1 }
    else { break }
  }
  return stops[bestIdx].x
}

/** 纯函数：给定 caret stops 和 x 坐标，返回最近的 UTF-16 code unit offset。 */
export function offsetForHorizontal(stops: CaretStop[], x: number): number {
  if (stops.length === 0) {
    return 0
  }
  let lo = 0, hi = stops.length - 1, bestIdx = 0
  while (lo <= hi) {
    const mid = Math.floor((lo + hi) / 2)
    if (stops[mid].x <= x) { bestIdx = mid; lo = mid + 1 }
    else { hi = mid - 1 }
  }
  if (bestIdx + 1 < stops.length) {
    const leftX = stops[bestIdx].x
    const rightX = stops[bestIdx + 1].x
    if (x - leftX <= rightX - x) {
      return stops[bestIdx].utf16Offset
    }
    return stops[bestIdx + 1].utf16Offset
  }
  return stops[bestIdx].utf16Offset
}

/** 纯函数：根据 UTF-16 offset 和 lines 的 breakKind，返回 visual line index。 */
export function resolveVisualLineIndex(
  lines: LineRange[],
  position: { utf16Offset: number; affinity: CaretAffinity }
): number {
  if (lines.length === 0) {
    return 0
  }
  const { utf16Offset: offset, affinity } = position
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    if (offset >= line.start && offset < line.end) { return i }
    if (offset === line.end) {
      if (line.breakKind === LineBreakKind.SoftWrap) {
        return affinity === CaretAffinity.Upstream ? i : i + 1
      }
      return i
    }
  }
  return lines.length - 1
}

/** 命中测试：根据 touchX/touchY 找命中的 UTF-16 offset + CaretAffinity。 */
export function hitTestPoint(
  text: string,
  lines: LineRange[],
  lineSpacingPx: number,
  touchX: number,
  touchY: number,
  measureTextFn: (s: string) => number,
): VisualCaretPosition {
  if (lines.length === 0) {
    return { utf16Offset: 0, affinity: CaretAffinity.Downstream }
  }
  let lineIndex = lineSpacingPx > 0 ? Math.floor(touchY / lineSpacingPx) : 0
  if (lineIndex < 0) { lineIndex = 0 }
  if (lineIndex > lines.length - 1) { lineIndex = lines.length - 1 }

  const line = lines[lineIndex]
  const stops = buildLineCaretStops(text, line, measureTextFn)
  const offset = offsetForHorizontal(stops, touchX)

  // Issue #629 R7-C item4: touchY 已选定 lineIndex，命中该行 soft-wrap 末尾恒为 Upstream；
  // 下一行 start 自然保持 Downstream。不再按同一行上/下半区分。
  let affinity = CaretAffinity.Downstream
  if (offset === line.end && line.breakKind === LineBreakKind.SoftWrap) {
    affinity = CaretAffinity.Upstream
  }
  return { utf16Offset: offset, affinity }
}


/**
 * Issue #629 R11 评论5329310563 第1项：水平方向 grapheme 移动到达目标 offset 时，
 * 根据到达方向决定 soft-wrap 边界的 affinity。
 *
 * 规则：
 *   目标 offset 是 SoftWrap 行尾（line.end === utf16Offset && breakKind === SoftWrap）：
 *     - Right 到达 -> Upstream（光标归上一行末尾，下次 Right 才进到下一行行首）
 *     - Left 到达  -> Downstream（光标归下一行行首，下次 Left 才退到上一行行尾）
 *   其他位置 -> Downstream
 *
 * 这是 "这次移动刚好到达 soft-wrap 边界" 的出口，与 positionForOffsetInLine
 * （行导航场景，已知目标行号）互补。纯函数，可被 Node .mjs 测试直接导入。
 */
export function positionForHorizontalArrival(
  lines: LineRange[],
  direction: 'left' | 'right',
  utf16Offset: number,
): VisualCaretPosition {
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    if (line.breakKind === LineBreakKind.SoftWrap && line.end === utf16Offset) {
      const affinity = direction === 'right' ? CaretAffinity.Upstream : CaretAffinity.Downstream
      return { utf16Offset, affinity }
    }
  }
  return { utf16Offset, affinity: CaretAffinity.Downstream }
}

/**
 * 纯函数：根据 UTF-16 offset 在指定行的 soft-wrap 边界位置，返回正确 affinity。
 *
 * SoftWrap 起点（行首，来自上一行软折）：Downstream（光标在此行）。
 * SoftWrap 终点（行末，折到下一行）：Upstream（光标归上一行末尾）。
 * HardBreak / EndOfText 行末：Downstream。
 *
 * Issue #629 评论5358224312 第2项：从 LineNavigationResolver.ets 下沉到纯 .ts，
 * 可被 Node .mjs 测试直接 import。与 positionForHorizontalArrival 互补：
 * positionForHorizontalArrival 已知移动方向，本函数已知目标行号（行导航场景）。
 */
export function positionForOffsetInLine(
  lines: LineRange[],
  lineIndex: number,
  utf16Offset: number,
): VisualCaretPosition {
  if (lineIndex < 0 || lineIndex >= lines.length) {
    return { utf16Offset, affinity: CaretAffinity.Downstream }
  }
  const line = lines[lineIndex]
  // 行首：SoftWrap 起点 → Downstream（光标在此行开始处）。
  if (utf16Offset === line.start) {
    return { utf16Offset, affinity: CaretAffinity.Downstream }
  }
  // 行末：SoftWrap 终点 → Upstream（光标归上一行末尾）。
  if (utf16Offset === line.end) {
    if (line.breakKind === LineBreakKind.SoftWrap) {
      return { utf16Offset, affinity: CaretAffinity.Upstream }
    }
    return { utf16Offset, affinity: CaretAffinity.Downstream }
  }
  // 其他位置：Downstream。
  return { utf16Offset, affinity: CaretAffinity.Downstream }
}
