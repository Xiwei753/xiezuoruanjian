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
import { layoutLines, hitTestPoint, CaretAffinity, LineBreakKind, buildLineRangesFromMetrics, SystemLineMetricData } from './editor_layout_math'
import { VisualCaretPosition, LineRange } from './editor_layout_math'
import {
  toLineLayouts,
  computeSelectionRects,
  computeCaretRect,
  computeCompositionUnderlineRects,
  computeCaretRectFromLineLayouts,
  computeSelectionRectsFromLineLayouts,
  computeCompositionUnderlineRectsFromLineLayouts,
  LineLayout,
  SelectionRect,
  CaretRect,
  CompositionUnderlineRect,
  CARET_WIDTH_PX,
  UNDERLINE_HEIGHT_PX,
} from './editor_render_geometry'
import { text } from '@kit.ArkGraphics2D'

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
    const selectionRects = computeSelectionRects(text, lines, lineSpacingPx, containerWidth, selStart, selEnd, measureTextFn)
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

  // ── Issue #768 任务10：系统 LayoutManager 路径 ──

  /**
   * 从系统 LayoutManager 构建渲染结果。
   *
   * layout: 系统 LayoutManager（从 TextController.getLayoutManager() 获取）
   * systemLineCount: layout.getLineCount()
   * snapshot: 编辑器布局快照
   * contentWidth: 内容区域宽度（px）
   * visualCaretAffinity: 光标亲和性
   * isApi14Plus: 是否 API14+（决定是否可用 getRectsForRange）
   * measureTextFn: measureText 函数（API12-13 fallback 用）
   */
  static renderLayoutFromSystem(
    layout: LayoutManager,
    systemLineCount: number,
    snapshot: EditorLayoutSnapshot,
    contentWidth: number,
    visualCaretAffinity: CaretAffinity,
    isApi14Plus: boolean,
    measureTextFn: (s: string) => number,
  ): EditorRenderResult {
    const base = EditorRenderBackend.render(snapshot)
    const text = base.text
    const cursorUtf16 = base.cursorUtf16
    const selStart = Math.min(base.selectionStartUtf16, base.selectionEndUtf16)
    const selEnd = Math.max(base.selectionStartUtf16, base.selectionEndUtf16)

    // 1. 从 LineMetrics 构建 LineLayout[] 和 LineRange[]
    const systemLines: LineLayout[] = []
    const lineMetricsData: SystemLineMetricData[] = []
    for (let i = 0; i < systemLineCount; i++) {
      const metrics = layout.getLineMetrics(i)
      const startUtf16 = metrics.startIndex
      const endUtf16 = metrics.endIndex
      lineMetricsData.push({
        startIndex: startUtf16,
        endIndex: endUtf16,
        isLastLine: i === systemLineCount - 1,
        text,
      })
      // y 坐标：topHeight 是从段落顶部到当前行的距离，用作行顶部 y
      const y = metrics.topHeight
      const height = metrics.height
      const left = metrics.left
      systemLines.push({
        startUtf16,
        endUtf16,
        left,
        y,
        height,
        breakKind: LineBreakKind.EndOfText, // 占位，下面用 buildLineRangesFromMetrics 覆盖
        caretStops: [],
      })
    }
    const systemLineRanges = buildLineRangesFromMetrics(lineMetricsData)
    // 用推导出的 breakKind 更新 systemLines（显式复制，不用 object spread）
    for (let i = 0; i < systemLines.length; i++) {
      systemLines[i] = {
        startUtf16: systemLines[i].startUtf16,
        endUtf16: systemLines[i].endUtf16,
        left: systemLines[i].left,
        y: systemLines[i].y,
        height: systemLines[i].height,
        caretStops: systemLines[i].caretStops,
        breakKind: systemLineRanges[i].breakKind,
      }
    }

    // 2. 计算选区/光标/composition 矩形
    let selectionRects: SelectionRect[] = []
    let caretRect: CaretRect | null = null
    let compositionUnderlineRects: CompositionUnderlineRect[] = []

    if (isApi14Plus) {
      // API14+：用 getRectsForRange 获取真实矩形
      selectionRects = EditorRenderBackend.buildSelectionRectsFromSystem(layout, selStart, selEnd)
      // Issue #768 评论5836390597 第3项：把 affinity 传给 buildCaretRectFromSystem。
      caretRect = EditorRenderBackend.buildCaretRectFromSystem(layout, systemLines, cursorUtf16, text, visualCaretAffinity)
      compositionUnderlineRects = EditorRenderBackend.buildCompositionRectsFromSystem(
        layout, base.compositionStartUtf16, base.compositionEndUtf16,
      )
    } else {
      // API12-13：用 LineMetrics 行边界 + measureText 算行内 x 坐标
      // Issue #768 评论5836390597 第2项：使用 LineLayout[]（含 left/y/height）的新几何函数，
      // 不再用 "x=0 + 第一行高度当所有行高度" 的旧函数。
      selectionRects = computeSelectionRectsFromLineLayouts(
        text, systemLines, contentWidth,
        selStart, selEnd, measureTextFn,
      )
      caretRect = computeCaretRectFromLineLayouts(
        text, systemLines, cursorUtf16,
        measureTextFn, visualCaretAffinity,
      )
      compositionUnderlineRects = computeCompositionUnderlineRectsFromLineLayouts(
        text, systemLines,
        base.compositionStartUtf16, base.compositionEndUtf16,
        measureTextFn,
      )
    }

    return {
      text: base.text,
      selectionStartUtf16: base.selectionStartUtf16,
      selectionEndUtf16: base.selectionEndUtf16,
      compositionStartUtf16: base.compositionStartUtf16,
      compositionEndUtf16: base.compositionEndUtf16,
      cursorUtf16: base.cursorUtf16,
      revision: base.revision,
      lines: systemLines,
      selectionRects,
      caretRect,
      compositionUnderlineRects,
    }
  }

  /**
   * 从系统 LayoutManager 做命中测试。
   * 用 layout.getGlyphPositionAtCoordinate(x, y) 做命中测试，
   * 将系统 Affinity（UPSTREAM=0/DOWNSTREAM=1）映射到 CaretAffinity。
   */
  static hitTestByLayoutManager(
    layout: LayoutManager,
    touchX: number,
    touchY: number,
  ): VisualCaretPosition {
    const result = layout.getGlyphPositionAtCoordinate(touchX, touchY)
    // PositionWithAffinity.affinity: UPSTREAM=0, DOWNSTREAM=1
    // CaretAffinity: Upstream='upstream', Downstream='downstream'
    const affinity: CaretAffinity = result.affinity === text.Affinity.UPSTREAM
      ? CaretAffinity.Upstream
      : CaretAffinity.Downstream
    return { utf16Offset: result.position, affinity }
  }

  // ── Issue #768 任务10：API14+ getRectsForRange 辅助方法 ──

  /** API14+ 用 getRectsForRange 获取选区矩形。 */
  private static buildSelectionRectsFromSystem(
    layout: LayoutManager,
    selStart: number,
    selEnd: number,
  ): SelectionRect[] {
    if (selStart === selEnd) {
      return []
    }
    try {
      const range: TextRange = { start: selStart, end: selEnd }
      const textBoxes: TextBox[] = layout.getRectsForRange(
        range, text.RectWidthStyle.TIGHT, text.RectHeightStyle.TIGHT,
      )
      const rects: SelectionRect[] = []
      for (const box of textBoxes) {
        const r = box.rect
        rects.push({
          x: r.left,
          y: r.top,
          width: r.right - r.left,
          height: r.bottom - r.top,
        })
      }
      return rects
    } catch (_e) {
      return []
    }
  }

  /**
   * API14+ 用 getRectsForRange 获取光标矩形。
   * Issue #768 评论5836390597 第3项：接收 affinity 参数，正确处理 soft-wrap 边界。
   *
   * soft-wrap 边界规则：
   * - Upstream → 上一视觉行末，取 [offset-1, offset] 的 right，y 用上一行
   * - Downstream → 下一视觉行首，取 [offset, offset+1] 的 left，y 用下一行
   * 不允许 x 和 y 分别猜两次。
   */
  private static buildCaretRectFromSystem(
    layout: LayoutManager,
    systemLines: LineLayout[],
    cursorUtf16: number,
    text: string,
    affinity: CaretAffinity,
  ): CaretRect | null {
    if (systemLines.length === 0) {
      return null
    }

    // 找到光标所在行，考虑 affinity。
    // soft-wrap 边界：同一个 UTF-16 offset 同时是上一行末尾和下一行开头。
    // Upstream → 上一行（endUtf16 === cursor 且 breakKind === SoftWrap）
    // Downstream → 下一行（startUtf16 === cursor）
    let lineIndex = 0
    for (let i = 0; i < systemLines.length; i++) {
      if (cursorUtf16 < systemLines[i].endUtf16) {
        lineIndex = i
        break
      }
      if (cursorUtf16 === systemLines[i].endUtf16) {
        // soft-wrap 边界：检查 affinity
        if (systemLines[i].breakKind === LineBreakKind.SoftWrap && affinity === CaretAffinity.Downstream) {
          // Downstream → 下一行开头
          lineIndex = Math.min(i + 1, systemLines.length - 1)
        } else {
          // Upstream 或非 soft-wrap → 当前行末尾
          lineIndex = i
        }
        break
      }
      if (i === systemLines.length - 1) {
        lineIndex = i
      }
    }
    const line = systemLines[lineIndex]

    try {
      let x: number
      if (cursorUtf16 < text.length) {
        // 光标在文本中间
        // Issue #768 评论5836931927 第1项：lineIndex 已经是按 affinity 选定的视觉行，
        // 直接判断 line 本身是否 soft-wrap Upstream，不要回头找 lineIndex - 1。
        const isSoftWrapUpstream =
          affinity === CaretAffinity.Upstream
          && cursorUtf16 > 0
          && cursorUtf16 === line.endUtf16
          && line.breakKind === LineBreakKind.SoftWrap

        if (isSoftWrapUpstream) {
          // soft-wrap 边界 Upstream：取 [cursor-1, cursor] 的 right（当前行末尾）
          const range: TextRange = { start: cursorUtf16 - 1, end: cursorUtf16 }
          const textBoxes: TextBox[] = layout.getRectsForRange(
            range, text.RectWidthStyle.TIGHT, text.RectHeightStyle.TIGHT,
          )
          if (textBoxes.length > 0) {
            x = textBoxes[textBoxes.length - 1].rect.right
          } else {
            x = line.left
          }
        } else {
          // 正常情况或 Downstream：取 [cursor, cursor+1] 的 left
          const range: TextRange = { start: cursorUtf16, end: cursorUtf16 + 1 }
          const textBoxes: TextBox[] = layout.getRectsForRange(
            range, text.RectWidthStyle.TIGHT, text.RectHeightStyle.TIGHT,
          )
          if (textBoxes.length > 0) {
            x = textBoxes[0].rect.left
          } else {
            x = line.left
          }
        }
      } else {
        // 光标在文本末尾：取 [cursor-1, cursor] 的 right
        if (cursorUtf16 > 0) {
          const range: TextRange = { start: cursorUtf16 - 1, end: cursorUtf16 }
          const textBoxes: TextBox[] = layout.getRectsForRange(
            range, text.RectWidthStyle.TIGHT, text.RectHeightStyle.TIGHT,
          )
          if (textBoxes.length > 0) {
            x = textBoxes[textBoxes.length - 1].rect.right
          } else {
            x = 0
          }
        } else {
          // 空文本
          x = 0
        }
      }
      return {
        x,
        y: line.y,
        width: CARET_WIDTH_PX,
        height: line.height,
      }
    } catch (_e) {
      return null
    }
  }

  /** API14+ 用 getRectsForRange 获取 composition 下划线矩形。 */
  private static buildCompositionRectsFromSystem(
    layout: LayoutManager,
    compStart: number | null,
    compEnd: number | null,
  ): CompositionUnderlineRect[] {
    if (compStart === null || compEnd === null || compStart === compEnd) {
      return []
    }
    try {
      const range: TextRange = { start: compStart, end: compEnd }
      const textBoxes: TextBox[] = layout.getRectsForRange(
        range, text.RectWidthStyle.TIGHT, text.RectHeightStyle.TIGHT,
      )
      const rects: CompositionUnderlineRect[] = []
      for (const box of textBoxes) {
        const r = box.rect
        rects.push({
          x: r.left,
          y: r.bottom - UNDERLINE_HEIGHT_PX,
          width: r.right - r.left,
          height: UNDERLINE_HEIGHT_PX,
        })
      }
      return rects
    } catch (_e) {
      return []
    }
  }
}
