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
import { LineBreakKind, CaretAffinity, nextCodePointBoundary, buildLineCaretStops, horizontalForOffset, offsetForHorizontal, resolveVisualLineIndex } from './editor_layout_math.ts'

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

// Issue #629 评论18：所有 offset↔x 算法统一来自 editor_layout_math.ts。
// 不再重复实现 nextCodePointBoundary / buildLineCaretStops / resolveVisualLineIndex。

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

/**
 * 计算选区矩形列表。
 *
 * Issue #629 评论5324447292 item4: HardBreak 行的 LF 被 selection 覆盖时，
 * 从当前行文字末端绘制到 contentWidth；空 hard line 的 LF 被选中时整行
 * x=0,width=contentWidth；普通可见字符选区仍按 caret stops/measure。
 */
export function computeSelectionRects(
  text: string,
  lines: LineRange[],
  lineSpacingPx: number,
  contentWidth: number,
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
    const hasVisibleText = selStartInLine < selEndInLine

    if (line.breakKind === LineBreakKind.HardBreak) {
      // 仍有可见文本被选中时画标准选区 rect（先画，保持从左到右渲染顺序）
      if (hasVisibleText) {
        const x = measureTextFn(text.substring(line.start, selStartInLine))
        const w = measureTextFn(text.substring(selStartInLine, selEndInLine))
        rects.push({ x, y: i * spacing, width: w, height: spacing })
      }
      // LF 位于 line.end（= line.start for empty hard line）。
      // 条件: selection 区间覆盖 line.end → selStart <= line.end && selEnd > line.end
      const lfCovered = start <= line.end && end > line.end
      if (lfCovered) {
        const isEmptyLine = line.start >= line.end
        if (isEmptyLine) {
          // 空 hard line: LF 被选中 → 整行 x=0, width=contentWidth
          rects.push({ x: 0, y: i * spacing, width: contentWidth, height: spacing })
        } else {
          // 非空 hard line: LF 被选中 → 从文字末端画到 contentWidth
          // Issue #629 R9：极端单 glyph 宽于容器时避免负数 width
          const x = measureTextFn(text.substring(line.start, line.end))
          rects.push({ x, y: i * spacing, width: Math.max(0, contentWidth - x), height: spacing })
        }
      }
      continue
    }

    // SoftWrap / EndOfText: 标准选区 rect
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
