// editor_render_geometry.ts — 编辑器静态渲染几何的纯数学模块。
//
// 选区矩形 / 光标矩形 / composition 下划线矩形 / 行布局的计算。
// 不依赖 ArkUI / EditorLayoutSnapshot，只依赖 string/number/Array 与 LineRange。
// 由 EditorRenderBackend.renderLayout 调用；测试在 editor_render_layout.test.mjs
// 注入确定性 mock measureFn 验证数学性质。
//
// 这些是编辑器静态显示能力，不是动画：不做闪烁/平滑移动/淡入淡出。
// 动画层以后新增 motion 消费同一份 snapshot，不改这里的纯函数签名。
//
// 所有 offset 是 UTF-16 code unit offset（ArkTS string.length 语义）。
// 坐标单位 px，相对组件左上角。

import type { LineRange } from './editor_layout_math'

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

/** composition 下划线矩形（px，相对组件左上）。height 通常 1-2px，位于行底部。 */
export interface CompositionUnderlineRect {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

/** 行布局（含 y/height，px，相对组件左上）。 */
export interface LineLayout {
  readonly startUtf16: number
  readonly endUtf16: number
  readonly y: number
  readonly height: number
}

/** 静态光标宽度（px）。 */
export const CARET_WIDTH_PX = 2
/** composition 下划线高度（px）。 */
export const UNDERLINE_HEIGHT_PX = 2

/**
 * 把 LineRange[] 转成 LineLayout[]（补 y/height）。
 * lineSpacingPx <= 0 时按 0 处理（退化但确定）。
 *
 * 性质：
 * - lines 为空返回 []。
 * - lineLayouts[i].y === i * lineSpacingPx。
 * - lineLayouts[i].height === lineSpacingPx（<=0 时为 0）。
 * - startUtf16/endUtf16 与输入 LineRange 一致。
 */
export function toLineLayouts(lines: LineRange[], lineSpacingPx: number): LineLayout[] {
  const spacing = lineSpacingPx > 0 ? lineSpacingPx : 0
  const out: LineLayout[] = []
  for (let i = 0; i < lines.length; i++) {
    out.push({
      startUtf16: lines[i].start,
      endUtf16: lines[i].end,
      y: i * spacing,
      height: spacing,
    })
  }
  return out
}

/**
 * 计算选区矩形列表。
 *
 * 性质：
 * - lines 为空返回 []。
 * - selStartUtf16 >= selEndUtf16（空选区）返回 []。
 * - 自动交换 selStartUtf16 > selEndUtf16。
 * - 选区跨越多行时每行一个 rect；行内选区起点/终点 clamp 到行边界。
 * - x = measureTextFn(text.substring(line.start, selStartInLine))
 * - width = measureTextFn(text.substring(selStartInLine, selEndInLine))
 * - y = lineIndex * lineSpacingPx, height = lineSpacingPx
 * - lineSpacingPx <= 0 时 y=0, height=0。
 */
export function computeSelectionRects(
  text: string,
  lines: LineRange[],
  lineSpacingPx: number,
  selStartUtf16: number,
  selEndUtf16: number,
  measureTextFn: (s: string) => number,
): SelectionRect[] {
  if (lines.length === 0) {
    return []
  }
  if (selStartUtf16 === selEndUtf16) {
    return []
  }
  const start = Math.min(selStartUtf16, selEndUtf16)
  const end = Math.max(selStartUtf16, selEndUtf16)
  const spacing = lineSpacingPx > 0 ? lineSpacingPx : 0
  const rects: SelectionRect[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const selStartInLine = Math.max(line.start, start)
    const selEndInLine = Math.min(line.end, end)
    if (selStartInLine >= selEndInLine) {
      continue
    }
    const x = measureTextFn(text.substring(line.start, selStartInLine))
    const w = measureTextFn(text.substring(selStartInLine, selEndInLine))
    rects.push({
      x,
      y: i * spacing,
      width: w,
      height: spacing,
    })
  }
  return rects
}

/**
 * 计算光标矩形。
 *
 * 性质：
 * - lines 为空返回 null（无文本时光标位置不确定）。
 * - cursorUtf16 clamp 到 [0, text.length]。
 * - 找第一个 line.end >= cursor 的行：cursor === line.end 归到该行（光标在行尾显示）。
 * - x = measureTextFn(text.substring(line.start, clampedCursor))
 * - y = lineIndex * lineSpacingPx, width = CARET_WIDTH_PX, height = lineSpacingPx
 * - lineSpacingPx <= 0 时 y=0, height=0。
 */
export function computeCaretRect(
  text: string,
  lines: LineRange[],
  lineSpacingPx: number,
  cursorUtf16: number,
  measureTextFn: (s: string) => number,
): CaretRect | null {
  if (lines.length === 0) {
    return null
  }
  const n = text.length
  let cursor = cursorUtf16
  if (cursor < 0) {
    cursor = 0
  }
  if (cursor > n) {
    cursor = n
  }
  // 找第一个 line.end >= cursor 的行；cursor === line.end 归到该行（行尾）。
  let lineIndex = lines.length - 1
  for (let i = 0; i < lines.length; i++) {
    if (cursor <= lines[i].end) {
      lineIndex = i
      break
    }
  }
  const line = lines[lineIndex]
  const clampedCursor = Math.max(line.start, Math.min(line.end, cursor))
  const x = measureTextFn(text.substring(line.start, clampedCursor))
  const spacing = lineSpacingPx > 0 ? lineSpacingPx : 0
  return {
    x,
    y: lineIndex * spacing,
    width: CARET_WIDTH_PX,
    height: spacing,
  }
}

/**
 * 计算 composition 下划线矩形列表。
 *
 * 性质：
 * - lines 为空返回 []。
 * - compStartUtf16 === null 或 compEndUtf16 === null 返回 []。
 * - compStartUtf16 >= compEndUtf16 返回 []。
 * - 自动交换 compStartUtf16 > compEndUtf16。
 * - composition 跨越多行时每行一个下划线 rect。
 * - x = measureTextFn(text.substring(line.start, compStartInLine))
 * - width = measureTextFn(text.substring(compStartInLine, compEndInLine))
 * - y = lineIndex * lineSpacingPx + lineSpacingPx - UNDERLINE_HEIGHT_PX（行底部）
 * - height = UNDERLINE_HEIGHT_PX
 * - lineSpacingPx <= 0 时 y = -UNDERLINE_HEIGHT_PX（退化但确定）。
 */
export function computeCompositionUnderlineRects(
  text: string,
  lines: LineRange[],
  lineSpacingPx: number,
  compStartUtf16: number | null,
  compEndUtf16: number | null,
  measureTextFn: (s: string) => number,
): CompositionUnderlineRect[] {
  if (lines.length === 0) {
    return []
  }
  if (compStartUtf16 === null || compEndUtf16 === null) {
    return []
  }
  if (compStartUtf16 === compEndUtf16) {
    return []
  }
  const start = Math.min(compStartUtf16, compEndUtf16)
  const end = Math.max(compStartUtf16, compEndUtf16)
  const spacing = lineSpacingPx > 0 ? lineSpacingPx : 0
  const rects: CompositionUnderlineRect[] = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const compStartInLine = Math.max(line.start, start)
    const compEndInLine = Math.min(line.end, end)
    if (compStartInLine >= compEndInLine) {
      continue
    }
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
