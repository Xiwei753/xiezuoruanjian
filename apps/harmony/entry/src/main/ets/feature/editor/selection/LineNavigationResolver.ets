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
} from '../render/editor_layout_math'
import { CaretStop, LineRange, VisualCaretPosition } from '../render/editor_layout_math'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'

import { LineLayoutStore } from './line_layout_store'
import { EditorLayoutIdentity } from './editor_layout_identity'

// Issue #629 评论18：NavigationLine 直接复用 LineLayout（已含 breakKind + caretStops）。
export type NavigationLine = LineLayout

// 带身份的行布局状态。
export interface LineLayoutState extends EditorLayoutIdentity {
  readonly contentWidth: number
  readonly fontSize: number
  readonly lines: NavigationLine[]
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
    const stops = state.lines[lineIdx].caretStops
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
    const stops = line.caretStops
    if (stops.length === 0) {
      return line.startUtf16
    }
    return offsetForHorizontal(stops, x)
  }

  /**
   * Issue #768 任务10：从系统 LayoutManager 数据构建 LineLayoutState。
   *
   * 从系统 LineMetrics 构建 LineLayout[]（含 y/height/breakKind），
   * 然后构造 LineLayoutState。系统行的 caretStops 为空数组
   *（因为系统行的 caret stops 由 LayoutManager 管理，不需要自计算）。
   */
  static buildLayoutStateFromSystem(
    layout: LayoutManager,
    systemLineCount: number,
    snapshot: EditorLayoutSnapshot,
    contentWidth: number,
  ): LineLayoutState {
    const text = snapshot.text
    const lineMetricsData: Array<{ startIndex: number; endIndex: number; isLastLine: boolean; text: string }> = []
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
      const y = metrics.topHeight
      const height = metrics.height
      systemLines.push({
        startUtf16,
        endUtf16,
        y,
        height,
        breakKind: LineBreakKind.EndOfText, // 占位，下面用 buildLineRangesFromMetrics 覆盖
        caretStops: [],
      })
    }

    const systemLineRanges = buildLineRangesFromMetrics(lineMetricsData)
    // 用推导出的 breakKind 更新 systemLines
    for (let i = 0; i < systemLines.length; i++) {
      systemLines[i] = {
        ...systemLines[i],
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
    }
  }
}
