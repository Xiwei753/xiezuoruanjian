// editor_render_geometry.ts — 编辑器静态渲染几何的纯数学模块。
//
// Issue #629 评论18：统一 layout source + soft-wrap affinity。
// 选区矩形 / 光标矩形 / composition 下划线矩形 / 行布局的计算。
// 类型从 editor_layout_math.ts 导入（import type，解决 Node ESM 模块解析）。
// 函数（buildLineCaretStops / resolveVisualLineIndex）在本文件直接定义，
// 保证测试和生产代码走同一条路径。
//
// 所有 offset 是 UTF-16 code unit offset（ArkTS string.length 语义）。
// 坐标单位 px，相对组件左上角。

import type { LineRange, VisualCaretPosition, CaretStop } from './editor_layout_math.ts'
import { LineBreakKind, CaretAffinity } from './editor_layout_math.ts'

/** 选区矩形（px，相对组件左上）。 */
export interface SelectionRect {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

/** 光标矩形（px，相对组件左上）。width 通常 1-2px。 */
export interface CaretRect {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

/** composition 下划线矩形（px，相对组件左上）。height 通常 1-2px。 */
export interface CompositionUnderlineRect {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

/** 行布局（含 y/height/breakKind/caretStops，px，相对组件左上）。 */
export interface LineLayout {
  readonly startUtf16: number
  readonly endUtf16: number
  readonly y: number
  readonly height: number
  readonly breakKind: LineBreakKind
  readonly caretStops: CaretStop[]
}

/** 静态光标宽度（px）。 */
export const CARET_WIDTH_PX = 2
/** composition 下划线高度（px）。 */
export const UNDERLINE_HEIGHT_PX = 2

// ── 函数定义（与 editor_layout_math.ts 共享同一套算法，不重复实现）──

/**
 * 返回 code unit index i 后的下一个 code point 边界。
 * surrogate pair 推进 2，否则推进 1。
 */
function nextCodePointBoundary(text: string, i: number): number {
  const n = text.length
  if (i >= n) { return n }
  if (i < 0) { i = 0 }
  const code = text.charCodeAt(i)
  if (code >= 0xD800 && code <= 0xDBFF && i + 1 < n) {
    const low = text.charCodeAt(i + 1)
    if (low >= 0xDC00 && low <= 0xDFFF) { return i + 2 }
  }
  return i + 1
}

/** 给定一行文本，生成该行所有 code point 边界的 caret stops。 */
function buildLineCaretStops(
  text: string,
  line: { start: number; end: number },
  measureTextFn: (s: string) => number,
): CaretStop[] {
  const stops: CaretStop[] = []
  let i = line.start
  while (i <= line.end) {
    const x = measureTextFn(text.substring(line.start, i))
    stops.push({ utf16Offset: i, x })
    if (i >= line.end) { break }
    i = nextCodePointBoundary(text, i)
  }
  return stops
}

/** 根据 UTF-16 offset 和 lines 的 breakKind，返回 visual line index。 */
function resolveVisualLineIndex(
  lines: LineRange[],
  position: { utf16Offset: number; affinity: CaretAffinity }
): number {
  if (lines.length === 0) { return 0 }
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

// ── 导出函数 ──

/**
 * 把 LineRange[] 转成 LineLayout[]（补 y/height/breakKind/caretStops）。
 * lineSpacingPx <= 0 时按 0 处理。
 */
export function toLineLayouts(
  lines: LineRange[],
  lineSpacingPx: number,
  text: string,
  measureTextFn: (s: string) => number,
): LineLayout[] {
  const spacing = lineSpacingPx > 0 ? lineSpacingPx : 0
  const out: LineLayout[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const stops = buildLineCaretStops(text, line, measureTextFn)
    out.push({
      startUtf16: line.start,
      endUtf16: line.end,
      y: i * spacing,
      height: spacing,
      breakKind: line.breakKind,
      caretStops: stops,
    })
  }
  return out
}

/** 计算选区矩形列表。 */
export function computeSelectionRects(
  text: string,
  lines: LineRange[],
  lineSpacingPx: number,
  selStartUtf16: number,
  selEndUtf16: number,
  measureTextFn: (s: string) => number,
): SelectionRect[] {
  if (lines.length === 0) { return [] }
  if (selStartUtf16 === selEndUtf16) { return [] }
  const start = Math.min(selStartUtf16, selEndUtf16)
  const end = Math.max(selStartUtf16, selEndUtf16)
  const spacing = lineSpacingPx > 0 ? lineSpacingPx : 0
  const rects: SelectionRect[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const selStartInLine = Math.max(line.start, start)
    const selEndInLine = Math.min(line.end, end)
    if (selStartInLine >= selEndInLine) { continue }
    const x = measureTextFn(text.substring(line.start, selStartInLine))
    const w = measureTextFn(text.substring(selStartInLine, selEndInLine))
    rects.push({ x, y: i * spacing, width: w, height: spacing })
  }
  return rects
}

/**
 * 计算光标矩形。
 * Issue #629 评论18：使用 VisualCaretPosition + resolveVisualLineIndex，
 * 不再自己扫描 cursor <= line.end。
 */
export function computeCaretRect(
  text: string,
  lines: LineRange[],
  lineSpacingPx: number,
  cursorUtf16: number,
  measureTextFn: (s: string) => number,
  // 默认 Upstream：soft-wrap 边界放在上一行末尾（与旧行为一致）。
  // Downstream 用于命中测试明确指定了 affinity 的场景。
  affinity: CaretAffinity = CaretAffinity.Upstream,
): CaretRect | null {
  if (lines.length === 0) { return null }
  const n = text.length
  let cursor = cursorUtf16
  if (cursor < 0) { cursor = 0 }
  if (cursor > n) { cursor = n }
  const lineIndex = resolveVisualLineIndex(lines, { utf16Offset: cursor, affinity })
  const line = lines[lineIndex]
  const clampedCursor = Math.max(line.start, Math.min(line.end, cursor))
  const x = measureTextFn(text.substring(line.start, clampedCursor))
  const spacing = lineSpacingPx > 0 ? lineSpacingPx : 0
  return { x, y: lineIndex * spacing, width: CARET_WIDTH_PX, height: spacing }
}

/** 计算 composition 下划线矩形列表。 */
export function computeCompositionUnderlineRects(
  text: string,
  lines: LineRange[],
  lineSpacingPx: number,
  compStartUtf16: number | null,
  compEndUtf16: number | null,
  measureTextFn: (s: string) => number,
): CompositionUnderlineRect[] {
  if (lines.length === 0) { return [] }
  if (compStartUtf16 === null || compEndUtf16 === null) { return [] }
  if (compStartUtf16 === compEndUtf16) { return [] }
  const start = Math.min(compStartUtf16, compEndUtf16)
  const end = Math.max(compStartUtf16, compEndUtf16)
  const spacing = lineSpacingPx > 0 ? lineSpacingPx : 0
  const rects: CompositionUnderlineRect[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const compStartInLine = Math.max(line.start, start)
    const compEndInLine = Math.min(line.end, end)
    if (compStartInLine >= compEndInLine) { continue }
    const x = measureTextFn(text.substring(line.start, compStartInLine))
    const w = measureTextFn(text.substring(compStartInLine, compEndInLine))
    rects.push({
      x,
      y: i * spacing + spacing - UNDERLINE_HEIGHT_PX,
      width: w,
      height: UNDERLINE_HEIGHT_PX,
    })
  }
  return rects
}
