// LineNavigationResolver — 行级导航解析器。
// Issue #629 评论21：删除自有 state/waiters/resolveWaiters/matchesIdentity，
// 委托 LineLayoutStore 管理状态和等待；EditorLayoutIdentity 来自共享 .ts。
// 行导航纯函数继续委托 editor_layout_math.ts。
// Issue #629 评论5358224312 第2项：positionForOffsetInLine 也委托 editor_layout_math 纯函数，
// LineNavigationResolver.ets 只保留 waiter/state 薄委托，不保留纯算法实现。

import { LineLayout } from '../render/editor_render_geometry'
import {
  CaretAffinity,
  LineBreakKind,
  horizontalForOffset,
  offsetForHorizontal,
  positionForHorizontalArrival,
  positionForOffsetInLine,
  resolveVisualLineIndex,
  buildLineRangesFromMetrics,
  buildLineCaretStops,
  SystemLineMetricData,
} from '../render/editor_layout_math'
import { CaretStop, LineRange, VisualCaretPosition } from '../render/editor_layout_math'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { text } from '@kit.ArkGraphics2D'

import { LineLayoutStore } from './line_layout_store'
import { EditorLayoutIdentity } from './editor_layout_identity'

// Issue #629 评论18：NavigationLine 直接复用 LineLayout（已含 breakKind + caretStops）。
export type NavigationLine = LineLayout

/**
 * Issue #1026：系统行的 caretStops 延迟到真正需要行内 offset↔x 映射时再生成。
 * 缓存绑定当前布局身份（revision/text/font/contentWidth/line 边界）；
 * 同一份稳定布局上的重复行导航查询复用已生成的行，不再每次逐 code point measure。
 */
export class LineCaretStopsCache {
  private readonly revision: number
  private readonly displayText: string
  private readonly fontSize: number
  private readonly contentWidth: number
  private readonly lines: NavigationLine[]
  private readonly stopsByLine: Map<number, CaretStop[]> = new Map<number, CaretStop[]>()

  constructor(
    revision: number,
    displayText: string,
    fontSize: number,
    contentWidth: number,
    lines: NavigationLine[],
  ) {
    this.revision = revision
    this.displayText = displayText
    this.fontSize = fontSize
    this.contentWidth = contentWidth
    this.lines = lines
  }

  matches(state: LineLayoutState): boolean {
    return this.revision === state.revision
      && this.displayText === state.displayText
      && this.fontSize === state.fontSize
      && this.contentWidth === state.contentWidth
      && this.lines === state.lines
  }

  get(lineIndex: number): CaretStop[] | undefined {
    return this.stopsByLine.get(lineIndex)
  }

  set(lineIndex: number, stops: CaretStop[]): void {
    this.stopsByLine.set(lineIndex, stops)
  }
}

// 带身份的行布局状态。
// Issue #768 评论5835171266 第4项：新增 systemLayoutManager 字段。
// 当此字段非 null 时，Up/Down 行导航用系统 LayoutManager API 取真实 x/offset，
// 不依赖 caretStops（系统行的 caretStops 为空数组）。
export interface LineLayoutState extends EditorLayoutIdentity {
  readonly contentWidth: number
  readonly fontSize: number
  readonly lines: NavigationLine[]
  // 系统 LayoutManager 引用。非 null 时行导航用系统 API；null 时回退到 caretStops 方案。
  readonly systemLayoutManager: LayoutManager | null
  // Issue #776 评论5849108212 问题2：px↔vp 转换函数，用于系统 LayoutManager 路径的单位统一。
  // 系统行导航（getCaretXFromSystem/getNearestOffsetFromSystem）需要用这两个函数
  // 把系统 API 的 px 坐标和编辑器的 vp 坐标互相转换。
  readonly pxToVp?: (value: number) => number
  readonly vpToPx?: (value: number) => number
  // Issue #1026：系统行 caretStops 按需生成时使用的测量函数和行级缓存。
  readonly measureTextFn?: (s: string) => number
  readonly caretStopCache?: LineCaretStopsCache
}

export class LineNavigationResolver {
  private store: LineLayoutStore<LineLayoutState> = new LineLayoutStore()

  /** SujianEditor 在渲染结果更新后调用，发布最新行布局（带身份）。传 null 清空。 */
  updateLayout(state: LineLayoutState | null): void {
    this.store.update(state)
  }

  /**
   * 等待匹配编辑状态身份的 layout state。
   * 不加固定 timeout；只有匹配布局发布或 cancelWait 两个出口。
   */
  waitForLayout(identity: EditorLayoutIdentity): Promise<LineLayoutState | null> {
    return this.store.waitFor(identity)
  }

  /** 取消所有等待（组件销毁/换 session 时调用）。 */
  cancelWait(): void {
    this.store.cancelAll()
  }

  // ── 纯函数式静态方法 ──

  /** 指定行的行首 UTF-16 offset。行号越界时返回 0。 */
  static getLineStart(state: LineLayoutState, lineIndex: number): number {
    if (lineIndex < 0 || lineIndex >= state.lines.length) {
      return 0
    }
    return state.lines[lineIndex].startUtf16
  }

  /** 指定行的行末 UTF-16 offset。行号越界时返回 0。 */
  static getLineEnd(state: LineLayoutState, lineIndex: number): number {
    if (lineIndex < 0 || lineIndex >= state.lines.length) {
      return 0
    }
    return state.lines[lineIndex].endUtf16
  }

  /**
   * Issue #629 评论19 + R7 任务B：用 resolveVisualLineIndex 找光标所在行。
   * 接受 VisualCaretPosition（含 affinity），不再写死 Downstream。
   */
  static getCurrentLineIndex(state: LineLayoutState, position: VisualCaretPosition): number {
    if (state.lines.length === 0) {
      return -1
    }
    const lineRanges: LineRange[] = state.lines.map((l: NavigationLine): LineRange => ({
      start: l.startUtf16,
      end: l.endUtf16,
      breakKind: l.breakKind,
    }))
    return resolveVisualLineIndex(lineRanges, position)
  }

  /** 上一行行号。已在第一行时返回 -1。 */
  static getPreviousLineIndex(state: LineLayoutState, position: VisualCaretPosition): number {
    const currentIdx = LineNavigationResolver.getCurrentLineIndex(state, position)
    if (currentIdx <= 0) {
      return -1
    }
    return currentIdx - 1
  }

  /** 下一行行号。已在最后一行时返回 -1。 */
  static getNextLineIndex(state: LineLayoutState, position: VisualCaretPosition): number {
    const currentIdx = LineNavigationResolver.getCurrentLineIndex(state, position)
    if (currentIdx < 0 || currentIdx >= state.lines.length - 1) {
      return -1
    }
    return currentIdx + 1
  }

  /**
   * Issue #629 R7 任务B：纯函数 — 根据 offset 在指定行的 soft-wrap 边界位置，
   * 返回正确的 VisualCaretPosition。
   */
  // Issue #629 评论5358224312 第2项：委托 editor_layout_math.positionForOffsetInLine 纯函数。
  // LineNavigationResolver.ets 只保留 waiter/state 薄委托，不保留纯算法实现。
  static positionForOffsetInLine(state: LineLayoutState, lineIndex: number, utf16Offset: number): VisualCaretPosition {
    const lineRanges: LineRange[] = state.lines.map((l: NavigationLine): LineRange => ({
      start: l.startUtf16,
      end: l.endUtf16,
      breakKind: l.breakKind,
    }))
    return positionForOffsetInLine(lineRanges, lineIndex, utf16Offset)
  }

  /**
   * Issue #629 R11 评论5329310563 第1项：水平方向 grapheme 移动到达目标 offset 时，
   * 根据到达方向决定 soft-wrap 边界的 affinity。
   */
  static positionForHorizontalArrival(
    state: LineLayoutState,
    direction: 'left' | 'right',
    utf16Offset: number,
  ): VisualCaretPosition {
    const lineRanges: LineRange[] = state.lines.map((l: NavigationLine): LineRange => ({
      start: l.startUtf16,
      end: l.endUtf16,
      breakKind: l.breakKind,
    }))
    return positionForHorizontalArrival(lineRanges, direction, utf16Offset)
  }

  /**
   * Issue #629 R7 任务B：getCaretX 接受 VisualCaretPosition。
   * 调用 horizontalForOffset（统一算法），不再各自实现二分。
   */
  static getCaretX(state: LineLayoutState, position: VisualCaretPosition): number {
    const lineIdx = LineNavigationResolver.getCurrentLineIndex(state, position)
    if (lineIdx < 0 || lineIdx >= state.lines.length) {
      return 0
    }
    const stops = LineNavigationResolver.ensureCaretStops(state, lineIdx)
    return horizontalForOffset(stops, position.utf16Offset)
  }

  /**
   * Issue #629 评论18：getNearestOffsetAtX 直接调用 offsetForHorizontal（统一算法）。
   */
  static getNearestOffsetAtX(state: LineLayoutState, lineIndex: number, x: number): number {
    if (lineIndex < 0 || lineIndex >= state.lines.length) {
      return 0
    }
    const line = state.lines[lineIndex]
    const stops = LineNavigationResolver.ensureCaretStops(state, lineIndex)
    if (stops.length === 0) {
      return line.startUtf16
    }
    return offsetForHorizontal(stops, x)
  }

  /**
   * Issue #1026：系统行只在真正需要时生成 caretStops。
   * 同一 LineLayoutState 上按行缓存；缓存身份由 revision/text/font/contentWidth/行边界共同决定。
   */
  private static ensureCaretStops(state: LineLayoutState, lineIndex: number): CaretStop[] {
    const line = state.lines[lineIndex]
    if (line.caretStops.length > 0) {
      return line.caretStops
    }
    const cache = state.caretStopCache
    const measureTextFn = state.measureTextFn
    if (cache === undefined || measureTextFn === undefined || !cache.matches(state)) {
      return []
    }
    const cached = cache.get(lineIndex)
    if (cached !== undefined) {
      return cached
    }
    const rawStops = buildLineCaretStops(
      state.displayText,
      { start: line.startUtf16, end: line.endUtf16 },
      measureTextFn,
    )
    const stops = rawStops.map((stop: CaretStop): CaretStop => ({
      utf16Offset: stop.utf16Offset,
      x: line.left + stop.x,
    }))
    cache.set(lineIndex, stops)
    return stops
  }

  // ── Issue #768 评论5835171266 第4项：系统 LayoutManager 行导航 API ──
  // 系统行的 caretStops 为空数组，Up/Down 行导航不能依赖 caretStops。
  // 当 state.systemLayoutManager 非 null 时，用系统 API 取真实 x/offset。

  /**
   * 从系统 LayoutManager 获取当前光标的 x 坐标。
   * Issue #768 评论5835958606：修正 API 签名和文末光标处理。
   * - cursor < textLength：取当前字符 box 的 left
   * - cursor === textLength 且 cursor > 0：取前一个字符 box 的 right
   * - 空文本（cursor === 0 且 textLength === 0）：返回 0
   * Issue #768 评论5836390597 第3项：接收 affinity 参数，正确处理 soft-wrap 边界。
   * - Upstream → 上一行末尾，取 [offset-1, offset] 的 right
   * - Downstream → 下一行开头，取 [offset, offset+1] 的 left
   * 与 EditorRenderBackend.buildCaretRectFromSystem() 保持同一语义。
   */
  static getCaretXFromSystem(layout: LayoutManager, cursorUtf16: number, textLength: number, affinity: CaretAffinity, pxToVp?: (value: number) => number): number {
    try {
      // soft-wrap 边界 Upstream：取 [cursor-1, cursor] 的 right
      if (affinity === CaretAffinity.Upstream && cursorUtf16 > 0 && cursorUtf16 < textLength) {
        const range: TextRange = { start: cursorUtf16 - 1, end: cursorUtf16 }
        const boxes = layout.getRectsForRange(range, text.RectWidthStyle.TIGHT, text.RectHeightStyle.TIGHT)
        if (boxes.length > 0) {
          // Issue #776 评论5849108212 问题2：系统返回 px，需要转成 vp。
          const right = boxes[boxes.length - 1].rect.right
          return pxToVp ? pxToVp(right) : right
        }
        return 0
      }
      // Downstream 或文末：原有逻辑
      let range: TextRange
      if (cursorUtf16 < textLength) {
        range = { start: cursorUtf16, end: cursorUtf16 + 1 }
      } else if (cursorUtf16 > 0) {
        range = { start: cursorUtf16 - 1, end: cursorUtf16 }
      } else {
        return 0 // 空文本
      }
      const boxes = layout.getRectsForRange(range, text.RectWidthStyle.TIGHT, text.RectHeightStyle.TIGHT)
      if (boxes.length > 0) {
        if (cursorUtf16 < textLength) {
          // Issue #776 评论5849108212 问题2：系统返回 px，需要转成 vp。
          const left = boxes[0].rect.left
          return pxToVp ? pxToVp(left) : left
        } else {
          // Issue #776 评论5849108212 问题2：系统返回 px，需要转成 vp。
          const right = boxes[boxes.length - 1].rect.right
          return pxToVp ? pxToVp(right) : right
        }
      }
    } catch (_e) {
      // getRectsForRange 失败时回退到 0
    }
    return 0
  }

  /**
   * 从系统 LayoutManager 获取目标行最接近 targetX 的 offset。
   * Issue #768 评论5835958606：修正 API 签名，返回 PositionWithAffinity.position。
   */
  static getNearestOffsetFromSystem(layout: LayoutManager, targetX: number, targetLineY: number, vpToPx?: (value: number) => number): number {
    try {
      // Issue #776 评论5849108212 问题2：targetX/targetLineY 是 vp 坐标，
      // getGlyphPositionAtCoordinate 接收 px，需要先转回 px。
      const pxX = vpToPx ? vpToPx(targetX) : targetX
      const pxY = vpToPx ? vpToPx(targetLineY) : targetLineY
      const pos = layout.getGlyphPositionAtCoordinate(pxX, pxY)
      return pos.position
    } catch (_e) {
      // getGlyphPositionAtCoordinate 失败时回退到行首
    }
    return 0
  }

  /**
   * Issue #768 任务10：从系统 LayoutManager 数据构建 LineLayoutState。
   *
   * 从系统 LineMetrics 构建 LineLayout[]（含 left/y/height/breakKind/caretStops），
   * 然后构造 LineLayoutState。
   * Issue #768 评论5835171266 第4项：把 layout 引用写入 systemLayoutManager，
   * 让行导航可以直接用系统 API。
   * Issue #768 评论5836390597 第2项：API12~13 的 Up/Down 行导航需要 caretStops。
   * Issue #1026：改为真正查询时才按行生成并缓存，不再在每次整章布局时预生成。
   * 同时把系统行的真实 left（首行缩进时 > 0）放进 LineLayout，
   * API12~13 的 caret/selection/composition 都使用每行自己的 left/y/height。
   */
  static buildLayoutStateFromSystem(
    layout: LayoutManager,
    systemLineCount: number,
    snapshot: EditorLayoutSnapshot,
    contentWidth: number,
    measureTextFn?: (s: string) => number,
    pxToVp?: (value: number) => number,
    vpToPx?: (value: number) => number,
  ): LineLayoutState {
    const text = snapshot.text
    const lineMetricsData: SystemLineMetricData[] = []
    const systemLines: NavigationLine[] = []

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
      // Issue #776 评论5849108212 问题2：LineMetrics 的 topHeight/height/left 是 px，转成 vp。
      const y = pxToVp ? pxToVp(metrics.topHeight) : metrics.topHeight
      const height = pxToVp ? pxToVp(metrics.height) : metrics.height
      const left = pxToVp ? pxToVp(metrics.left) : metrics.left

      // Issue #1026：系统行的 caretStops 不再在整章布局时预生成。
      // 只有在 API12~13 行导航真正需要行内 offset↔x 映射时，
      // 才由 LineNavigationResolver.ensureCaretStops 按行延迟生成并缓存。
      const caretStops: CaretStop[] = []

      systemLines.push({
        startUtf16,
        endUtf16,
        left,
        y,
        height,
        breakKind: LineBreakKind.EndOfText, // 占位，下面用 buildLineRangesFromMetrics 覆盖
        caretStops,
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

    return {
      revision: snapshot.revision,
      generation: snapshot.generation,
      compositionGeneration: snapshot.compositionGeneration,
      compositionSessionId: snapshot.compositionSessionId,
      displayText: snapshot.text,
      contentWidth,
      fontSize: snapshot.fontSize,
      lines: systemLines,
      systemLayoutManager: layout,
      // Issue #776 评论5849108212 问题2：把 pxToVp/vpToPx 写入 state，供行导航调用系统 API 时使用。
      pxToVp,
      vpToPx,
      // Issue #1026：按需生成系统行 caretStops 所需的测量函数和行级缓存。
      measureTextFn,
      caretStopCache: measureTextFn !== undefined
        ? new LineCaretStopsCache(snapshot.revision, text, snapshot.fontSize, contentWidth, systemLines)
        : undefined,
    }
  }
}
