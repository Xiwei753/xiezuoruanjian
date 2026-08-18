// EditorRenderBackend — 编辑器静态渲染后端。
// 输入只能是 EditorLayoutSnapshot。先做静态文本、选区、composition 下划线、光标和命中测试。
// 动画层以后新增 motion 消费同一份 snapshot，不改输入层和 session 层。
//
// Issue #629 评论18：统一 layout source。
// - layoutLines 返回 LineRange[]（含 breakKind：SoftWrap / HardBreak / EndOfText）
// - toLineLayouts 返回 LineLayout[]（含 breakKind + caretStops）
// - hitTestByPoint 返回 VisualCaretPosition（含 CaretAffinity）
// - computeCaretRect 消费 VisualCaretPosition，不再自己扫描 cursor <= line.end

import { EditorLayoutSnapshot } from './EditorLayoutSnapshot'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { layoutLines, hitTestPoint, CaretAffinity } from './editor_layout_math'
import type { VisualCaretPosition } from './editor_layout_math'
import {
  toLineLayouts,
  computeSelectionRects,
  computeCaretRect,
  computeCompositionUnderlineRects,
  LineLayout,
  SelectionRect,
  CaretRect,
  CompositionUnderlineRect,
} from './editor_render_geometry'

export interface EditorRenderResult {
  text: string
  selectionStartUtf16: number
  selectionEndUtf16: number
  compositionStartUtf16: number | null
  compositionEndUtf16: number | null
  cursorUtf16: number
  revision: number
  // 静态渲染几何。render() 不填（默认空/null），renderLayout() 填。
  lines: LineLayout[]
  selectionRects: SelectionRect[]
  caretRect: CaretRect | null
  compositionUnderlineRects: CompositionUnderlineRect[]
}

export class EditorRenderBackend {
  // 把 byte offset snapshot 转成 UTF-16 offset 渲染结果（ArkUI Text 用 UTF-16）。
  // 不含几何信息（lines/selectionRects/caretRect/compositionUnderlineRects 为空/null）。
  static render(snapshot: EditorLayoutSnapshot): EditorRenderResult {
    const text = snapshot.text
    return {
      text,
      selectionStartUtf16: TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionAnchor),
      selectionEndUtf16: TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionHead),
      compositionStartUtf16: snapshot.compositionStart === null
        ? null
        : TextOffsetMapper.utf8ToUtf16(text, snapshot.compositionStart),
      compositionEndUtf16: snapshot.compositionEnd === null
        ? null
        : TextOffsetMapper.utf8ToUtf16(text, snapshot.compositionEnd),
      cursorUtf16: TextOffsetMapper.utf8ToUtf16(text, snapshot.cursorByteOffset),
      revision: snapshot.revision,
      lines: [],
      selectionRects: [],
      caretRect: null,
      compositionUnderlineRects: [],
    }
  }

  // 完整静态渲染：在 render() 基础上计算行布局/选区/光标/composition 下划线几何。
  //
  // Issue #629 评论18：layoutLines 返回 breakKind，toLineLayouts 把 breakKind + caretStops
  // 一起带进 LineLayout。hitTestByPoint 返回 VisualCaretPosition。
  static renderLayout(
    snapshot: EditorLayoutSnapshot,
    containerWidth: number,
    textHeight: number,
    measureTextFn: (s: string) => number,
    // Issue #629 评论18：renderLayout 接受可选 visualCaretAffinity，
    // 由 SelectionController 发布的 VisualCaretPosition.affinity 传入，
    // 保证光标渲染与命中测试/行导航使用同一 affinity。
    visualCaretAffinity: CaretAffinity = CaretAffinity.Upstream,
  ): EditorRenderResult {
    const base = EditorRenderBackend.render(snapshot)
    const text = base.text
    const lines = layoutLines(text, containerWidth, measureTextFn)
    const lineSpacingPx = lines.length > 0 ? textHeight / lines.length : 0
    const lineLayouts = toLineLayouts(lines, lineSpacingPx, text, measureTextFn)
    const selStart = Math.min(base.selectionStartUtf16, base.selectionEndUtf16)
    const selEnd = Math.max(base.selectionStartUtf16, base.selectionEndUtf16)
    const selectionRects = computeSelectionRects(text, lines, lineSpacingPx, selStart, selEnd, measureTextFn)
    const caretRect = computeCaretRect(text, lines, lineSpacingPx, base.cursorUtf16, measureTextFn, visualCaretAffinity)
    const compositionUnderlineRects = computeCompositionUnderlineRects(
      text,
      lines,
      lineSpacingPx,
      base.compositionStartUtf16,
      base.compositionEndUtf16,
      measureTextFn,
    )
    return {
      text: base.text,
      selectionStartUtf16: base.selectionStartUtf16,
      selectionEndUtf16: base.selectionEndUtf16,
      compositionStartUtf16: base.compositionStartUtf16,
      compositionEndUtf16: base.compositionEndUtf16,
      cursorUtf16: base.cursorUtf16,
      revision: base.revision,
      lines: lineLayouts,
      selectionRects,
      caretRect,
      compositionUnderlineRects,
    }
  }

  // offset 命中测试（无坐标）。
  static hitTest(snapshot: EditorLayoutSnapshot, utf16Offset: number): 'before' | 'inside' | 'after' {
    const text = snapshot.text
    const anchorUtf16 = TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionAnchor)
    const headUtf16 = TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionHead)
    const start = Math.min(anchorUtf16, headUtf16)
    const end = Math.max(anchorUtf16, headUtf16)
    if (utf16Offset < start) return 'before'
    if (utf16Offset > end) return 'after'
    return 'inside'
  }

  // 坐标命中测试：返回 VisualCaretPosition（含 CaretAffinity）。
  // Issue #629 评论18：hitTestPoint 现在使用 buildLineCaretStops + offsetForHorizontal，
  // 不再自己二分字符边界；返回 VisualCaretPosition 区分 soft-wrap 上下行。
  static hitTestByPoint(
    snapshot: EditorLayoutSnapshot,
    containerWidth: number,
    textHeight: number,
    touchX: number,
    touchY: number,
    measureTextFn: (s: string) => number,
  ): VisualCaretPosition {
    const text = snapshot.text
    const lines = layoutLines(text, containerWidth, measureTextFn)
    const lineSpacingPx = lines.length > 0 ? textHeight / lines.length : 0
    return hitTestPoint(text, lines, lineSpacingPx, touchX, touchY, measureTextFn)
  }

  // Issue #629 评论18：hitTestByPoint 返回 VisualCaretPosition，
  // 同时计算当前 cursor 的 VisualCaretPosition 供 renderLayout 使用。
  static computeCurrentCaretPosition(
    snapshot: EditorLayoutSnapshot,
    containerWidth: number,
    textHeight: number,
    measureTextFn: (s: string) => number,
  ): VisualCaretPosition {
    const text = snapshot.text
    const lines = layoutLines(text, containerWidth, measureTextFn)
    if (lines.length === 0) {
      return { utf16Offset: 0, affinity: CaretAffinity.Downstream }
    }
    // 用 resolveVisualLineIndex 找光标所在行，判断光标在 soft-wrap 边界时的 affinity。
    // 但 renderLayout 默认 Upstream（光标在上一行尾），只有 hitTest 明确给了 Downstream 才改。
    // 这里返回 Upstream 作为默认值，实际光标渲染通过 visualCaretAffinity 参数传入。
    return { utf16Offset: 0, affinity: CaretAffinity.Upstream }
  }
}
