// editor_render_geometry.ts — 编辑器静态渲染几何的纯数学模块。
//
// Issue #629 评论18：统一 layout source + soft-wrap affinity。
// 选区矩形 / 光标矩形 / composition 下划线矩形 / 行布局的计算。
// 类型从 editor_layout_math.ts 导入（interface 用 import type，解决 Node ESM 运行时解析）。
// 函数（buildLineCaretStops / resolveVisualLineIndex）在本文件直接定义，
// 保证测试和生产代码走同一条路径。
//
// 所有 offset 是 UTF-16 code unit offset（ArkTS string.length 语义）。
// 坐标单位 vp，相对组件左上角。
// Issue #776 评论5848626733 第6项：系统 LayoutManager 返回的几何是 px，
// 调用方（EditorRenderBackend）在传入前用 pxToVp 转成 vp；
// fallback 路径的 measureTextFn 契约也返回 vp。

import type { LineRange, VisualCaretPosition, CaretStop } from './editor_layout_math.ts'
import { LineBreakKind, CaretAffinity, nextCodePointBoundary, buildLineCaretStops, horizontalForOffset, offsetForHorizontal, resolveVisualLineIndex } from './editor_layout_math.ts'

/** 选区矩形（vp，相对组件左上）。 */
export interface SelectionRect {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

/** 光标矩形（vp，相对组件左上）。width 通常 2vp。 */
export interface CaretRect {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

/** composition 下划线矩形（vp，相对组件左上）。height 通常 2vp。 */
export interface CompositionUnderlineRect {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

/** 行布局（含 left/y/height/breakKind/caretStops，vp，相对组件左上）。 */
// Issue #768 评论5836390597 第2项：新增 left 字段。
// 系统行的 left 来自 LineMetrics.left（首行缩进等场景 left > 0）；
// fallback 自计算行的 left 来自 toLineLayouts 的 firstLineIndentVp 参数（段落首行 > 0）。
export interface LineLayout {
  readonly startUtf16: number
  readonly endUtf16: number
  readonly left: number
  readonly y: number
  readonly height: number
  readonly breakKind: LineBreakKind
  readonly caretStops: CaretStop[]
}

/** 静态光标宽度（vp）。 */
export const CARET_WIDTH_VP = 2
/** composition 下划线高度（vp）。 */
export const UNDERLINE_HEIGHT_VP = 2

// Issue #629 评论18：所有 offset↔x 算法统一来自 editor_layout_math.ts。
// 不再重复实现 nextCodePointBoundary / buildLineCaretStops / resolveVisualLineIndex。

// ── 导出函数 ──

/**
 * 把 LineRange[] 转成 LineLayout[]（补 y/height/breakKind/caretStops）。
 * lineHeightVp <= 0 时按 0 处理。
 * Issue #776 评论5848626733 第7项：firstLineIndentVp 用于 fallback 路径的首行缩进。
 * 段落首行（i===0 或前一个字符是 \n）的 left = firstLineIndentVp；其余行 left = 0。
 */
export function toLineLayouts(
  lines: LineRange[],
  lineHeightVp: number,
  text: string,
  measureTextFn: (s: string) => number,
  firstLineIndentVp: number = 0,
): LineLayout[] {
  const spacing = lineHeightVp > 0 ? lineHeightVp : 0
  const out: LineLayout[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    // Issue #776 评论5849108212 问题3：caretStops 的 x 加上 line.left，
    // 使 caretStops 反映真实行左边界（首行缩进时 left > 0）。
    const rawStops = buildLineCaretStops(text, line, measureTextFn)
    const isParagraphFirstLine = i === 0 || (line.start > 0 && text.charAt(line.start - 1) === '\n')
    const left = isParagraphFirstLine ? firstLineIndentVp : 0
    const stops = rawStops.map((stop: CaretStop): CaretStop => ({
      utf16Offset: stop.utf16Offset,
      x: left + stop.x,
    }))
    out.push({
      startUtf16: line.start,
      endUtf16: line.end,
      left,
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
  lineHeightVp: number,
  contentWidth: number,
  selStartUtf16: number,
  selEndUtf16: number,
  measureTextFn: (s: string) => number,
  firstLineIndentVp: number = 0,
): SelectionRect[] {
  if (lines.length === 0) { return [] }
  if (selStartUtf16 === selEndUtf16) { return [] }
  const start = Math.min(selStartUtf16, selEndUtf16)
  const end = Math.max(selStartUtf16, selEndUtf16)
  const spacing = lineHeightVp > 0 ? lineHeightVp : 0
  const rects: SelectionRect[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const selStartInLine = Math.max(line.start, start)
    const selEndInLine = Math.min(line.end, end)
    const hasVisibleText = selStartInLine < selEndInLine
    // Issue #776 评论5848626733 第7项：fallback 路径首行缩进。
    const isParagraphFirstLine = i === 0 || (line.start > 0 && text.charAt(line.start - 1) === '\n')
    const lineLeft = isParagraphFirstLine ? firstLineIndentVp : 0

    if (line.breakKind === LineBreakKind.HardBreak) {
      // 仍有可见文本被选中时画标准选区 rect（先画，保持从左到右渲染顺序）
      if (hasVisibleText) {
        const x = lineLeft + measureTextFn(text.substring(line.start, selStartInLine))
        const w = measureTextFn(text.substring(selStartInLine, selEndInLine))
        rects.push({ x, y: i * spacing, width: w, height: spacing })
      }
      // LF 位于 line.end（= line.start for empty hard line）。
      // 条件: selection 区间覆盖 line.end → selStart <= line.end && selEnd > line.end
      const lfCovered = start <= line.end && end > line.end
      if (lfCovered) {
        const isEmptyLine = line.start >= line.end
        if (isEmptyLine) {
          // 空 hard line: LF 被选中 → 整行从 lineLeft 起, width 不超过内容右边界
          rects.push({ x: lineLeft, y: i * spacing, width: Math.max(0, contentWidth - lineLeft), height: spacing })
        } else {
          // 非空 hard line: LF 被选中 → 从文字末端画到 contentWidth
          // Issue #629 R9：极端单 glyph 宽于容器时避免负数 width
          const x = lineLeft + measureTextFn(text.substring(line.start, line.end))
          rects.push({ x, y: i * spacing, width: Math.max(0, contentWidth - x), height: spacing })
        }
      }
      continue
    }

    // SoftWrap / EndOfText: 标准选区 rect
    if (selStartInLine >= selEndInLine) { continue }
    const x = lineLeft + measureTextFn(text.substring(line.start, selStartInLine))
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
  lineHeightVp: number,
  cursorUtf16: number,
  measureTextFn: (s: string) => number,
  // 默认 Upstream：soft-wrap 边界放在上一行末尾（与旧行为一致）。
  // Downstream 用于命中测试明确指定了 affinity 的场景。
  affinity: CaretAffinity = CaretAffinity.Upstream,
  firstLineIndentVp: number = 0,
): CaretRect | null {
  if (lines.length === 0) { return null }
  const n = text.length
  let cursor = cursorUtf16
  if (cursor < 0) { cursor = 0 }
  if (cursor > n) { cursor = n }
  const lineIndex = resolveVisualLineIndex(lines, { utf16Offset: cursor, affinity })
  const line = lines[lineIndex]
  const clampedCursor = Math.max(line.start, Math.min(line.end, cursor))
  // Issue #776 评论5848626733 第7项：fallback 路径首行缩进。
  const isParagraphFirstLine = lineIndex === 0 || (line.start > 0 && text.charAt(line.start - 1) === '\n')
  const lineLeft = isParagraphFirstLine ? firstLineIndentVp : 0
  const x = lineLeft + measureTextFn(text.substring(line.start, clampedCursor))
  const spacing = lineHeightVp > 0 ? lineHeightVp : 0
  return { x, y: lineIndex * spacing, width: CARET_WIDTH_VP, height: spacing }
}

/** 计算 composition 下划线矩形列表。 */
export function computeCompositionUnderlineRects(
  text: string,
  lines: LineRange[],
  lineHeightVp: number,
  compStartUtf16: number | null,
  compEndUtf16: number | null,
  measureTextFn: (s: string) => number,
  firstLineIndentVp: number = 0,
): CompositionUnderlineRect[] {
  if (lines.length === 0) { return [] }
  if (compStartUtf16 === null || compEndUtf16 === null) { return [] }
  if (compStartUtf16 === compEndUtf16) { return [] }
  const start = Math.min(compStartUtf16, compEndUtf16)
  const end = Math.max(compStartUtf16, compEndUtf16)
  const spacing = lineHeightVp > 0 ? lineHeightVp : 0
  const rects: CompositionUnderlineRect[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const compStartInLine = Math.max(line.start, start)
    const compEndInLine = Math.min(line.end, end)
    if (compStartInLine >= compEndInLine) { continue }
    // Issue #776 评论5848626733 第7项：fallback 路径首行缩进。
    const isParagraphFirstLine = i === 0 || (line.start > 0 && text.charAt(line.start - 1) === '\n')
    const lineLeft = isParagraphFirstLine ? firstLineIndentVp : 0
    const x = lineLeft + measureTextFn(text.substring(line.start, compStartInLine))
    const w = measureTextFn(text.substring(compStartInLine, compEndInLine))
    rects.push({
      x,
      y: i * spacing + spacing - UNDERLINE_HEIGHT_VP,
      width: w,
      height: UNDERLINE_HEIGHT_VP,
    })
  }
  return rects
}

// ── Issue #768 评论5836390597 第2项：系统行几何函数 ──
// API12~13 系统行的 caret/selection/composition 需要使用每行自己的 left/y/height，
// 不能再用 "x=0 + 第一行高度当所有行高度"。
// 这些函数接收 LineLayout[]（含 left/y/height），不依赖 lineHeightVp。

/**
 * 计算光标矩形（使用 LineLayout[] 的 left/y/height）。
 * 系统行的 left 来自 LineMetrics.left（首行缩进时 > 0）。
 */
export function computeCaretRectFromLineLayouts(
  text: string,
  lines: LineLayout[],
  cursorUtf16: number,
  measureTextFn: (s: string) => number,
  affinity: CaretAffinity = CaretAffinity.Upstream,
): CaretRect | null {
  if (lines.length === 0) { return null }
  const n = text.length
  let cursor = cursorUtf16
  if (cursor < 0) { cursor = 0 }
  if (cursor > n) { cursor = n }
  const lineRanges: LineRange[] = lines.map((l: LineLayout): LineRange => ({
    start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind,
  }))
  const lineIndex = resolveVisualLineIndex(lineRanges, { utf16Offset: cursor, affinity })
  const line = lines[lineIndex]
  const clampedCursor = Math.max(line.startUtf16, Math.min(line.endUtf16, cursor))
  const x = line.left + measureTextFn(text.substring(line.startUtf16, clampedCursor))
  return { x, y: line.y, width: CARET_WIDTH_VP, height: line.height }
}

/**
 * 计算选区矩形列表（使用 LineLayout[] 的 left/y/height）。
 */
export function computeSelectionRectsFromLineLayouts(
  text: string,
  lines: LineLayout[],
  contentWidth: number,
  selStartUtf16: number,
  selEndUtf16: number,
  measureTextFn: (s: string) => number,
): SelectionRect[] {
  if (lines.length === 0) { return [] }
  if (selStartUtf16 === selEndUtf16) { return [] }
  const start = Math.min(selStartUtf16, selEndUtf16)
  const end = Math.max(selStartUtf16, selEndUtf16)
  const rects: SelectionRect[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const selStartInLine = Math.max(line.startUtf16, start)
    const selEndInLine = Math.min(line.endUtf16, end)
    const hasVisibleText = selStartInLine < selEndInLine

    if (line.breakKind === LineBreakKind.HardBreak) {
      if (hasVisibleText) {
        const x = line.left + measureTextFn(text.substring(line.startUtf16, selStartInLine))
        const w = measureTextFn(text.substring(selStartInLine, selEndInLine))
        rects.push({ x, y: line.y, width: w, height: line.height })
      }
      const lfCovered = start <= line.endUtf16 && end > line.endUtf16
      if (lfCovered) {
        const isEmptyLine = line.startUtf16 >= line.endUtf16
        if (isEmptyLine) {
          // Issue #768 评论5836931927 第2项：空 hard line 首行缩进时 line.left > 0，
          // width 需要减去 line.left，否则矩形右边会越过内容右边界。
          rects.push({ x: line.left, y: line.y, width: Math.max(0, contentWidth - line.left), height: line.height })
        } else {
          const x = line.left + measureTextFn(text.substring(line.startUtf16, line.endUtf16))
          rects.push({ x, y: line.y, width: Math.max(0, contentWidth - x), height: line.height })
        }
      }
      continue
    }

    if (selStartInLine >= selEndInLine) { continue }
    const x = line.left + measureTextFn(text.substring(line.startUtf16, selStartInLine))
    const w = measureTextFn(text.substring(selStartInLine, selEndInLine))
    rects.push({ x, y: line.y, width: w, height: line.height })
  }
  return rects
}

/**
 * 计算 composition 下划线矩形列表（使用 LineLayout[] 的 left/y/height）。
 */
export function computeCompositionUnderlineRectsFromLineLayouts(
  text: string,
  lines: LineLayout[],
  compStartUtf16: number | null,
  compEndUtf16: number | null,
  measureTextFn: (s: string) => number,
): CompositionUnderlineRect[] {
  if (lines.length === 0) { return [] }
  if (compStartUtf16 === null || compEndUtf16 === null) { return [] }
  if (compStartUtf16 === compEndUtf16) { return [] }
  const start = Math.min(compStartUtf16, compEndUtf16)
  const end = Math.max(compStartUtf16, compEndUtf16)
  const rects: CompositionUnderlineRect[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const compStartInLine = Math.max(line.startUtf16, start)
    const compEndInLine = Math.min(line.endUtf16, end)
    if (compStartInLine >= compEndInLine) { continue }
    const x = line.left + measureTextFn(text.substring(line.startUtf16, compStartInLine))
    const w = measureTextFn(text.substring(compStartInLine, compEndInLine))
    rects.push({
      x,
      y: line.y + line.height - UNDERLINE_HEIGHT_VP,
      width: w,
      height: UNDERLINE_HEIGHT_VP,
    })
  }
  return rects
}

/**
 * Issue #879 复核评论6081596024 问题2：统一的字形矩形几何入口。
 *
 * 用 resolveVisualLineIndex + Downstream affinity 选行——
 * 软换行边界 offset 属于下一行（右侧视觉行），不属于上一行。
 * 返回 [startUtf16, endUtf16) 范围内字形的完整矩形 {x, y, width, height}。
 *
 * @param layout LineLayout[]（含 left/y/height/caretStops）
 * @param startUtf16 字形起始 UTF-16 offset（用 Downstream affinity 选行）
 * @param endUtf16 字形结束 UTF-16 offset（用 Upstream affinity 选行，即结束位置属于当前行）
 * @returns {x, y, width, height} 或 null（范围不在任何行内）
 */
export function glyphRectForRange(
  layout: LineLayout[],
  startUtf16: number,
  endUtf16: number
): { x: number; y: number; width: number; height: number } | null {
  if (layout.length === 0) { return null }
  // 起始位置用 Downstream affinity：软换行边界属于下一行
  const lineRanges: LineRange[] = layout.map((l: LineLayout): LineRange => ({
    start: l.startUtf16, end: l.endUtf16, breakKind: l.breakKind,
  }))
  const startLineIdx = resolveVisualLineIndex(lineRanges, { utf16Offset: startUtf16, affinity: CaretAffinity.Downstream })
  const startLine = layout[startLineIdx]
  if (startLine === undefined) { return null }
  // 起始 x：在起始行中找 startUtf16 对应的 x
  let x = startLine.left
  for (const stop of startLine.caretStops) {
    if (stop.utf16Offset <= startUtf16) {
      x = stop.x
    }
  }
  // 结束位置用 Upstream affinity：结束 offset 属于当前行末
  const endLineIdx = resolveVisualLineIndex(lineRanges, { utf16Offset: endUtf16, affinity: CaretAffinity.Upstream })
  const endLine = layout[endLineIdx]
  if (endLine === undefined) { return null }
  // 结束 x：在结束行中找 endUtf16 对应的 x
  let endX = endLine.left
  for (const stop of endLine.caretStops) {
    if (stop.utf16Offset <= endUtf16) {
      endX = stop.x
    }
  }
  // 如果跨行，宽度就是从 startUtf16 到起始行末
  // 但动画代码中 piece 通常不跨行（跨行的 move 已被拆成多个 piece）
  // 所以简单返回起始行的矩形即可
  const width = startLineIdx === endLineIdx ? endX - x : startLine.caretStops[startLine.caretStops.length - 1].x - x
  return {
    x,
    y: startLine.y,
    width: Math.max(0, width),
    height: startLine.height,
  }
}
