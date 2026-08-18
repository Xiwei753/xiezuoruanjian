// LineNavigationResolver — 行级导航解析器。
// Issue #629 评论11 第2项 + 评论14 第3项 + 评论15 第3项：
// Up/Down/Home/End 不再在入队前读旧布局计算目标位置。
// 行导航命令在 Dispatcher 唯一队列出队时通过此 resolver 基于最新布局计算。
//
// Issue #629 评论18：统一 layout source。
// - 删除自有 LineCaretStop 类型定义，改用 editor_layout_types.CaretStop。
// - getCaretX / getNearestOffsetAtX 直接调用 editor_layout_math.horizontalForOffset / offsetForHorizontal，
//   不再各自实现二分。hitTestPoint 也用同一套算法。
// - getCurrentLineIndex 使用 resolveVisualLineIndex（统一 soft-wrap 处理）。

import { LineLayout } from '../render/editor_render_geometry'
import {
  CaretAffinity,
  LineBreakKind,
  horizontalForOffset,
  offsetForHorizontal,
  positionForHorizontalArrival,
  resolveVisualLineIndex,
} from '../render/editor_layout_math'
import type { CaretStop, LineRange, VisualCaretPosition } from '../render/editor_layout_math'

// Issue #629 评论15 第3项：布局身份 = 编辑状态身份。
// revision + generation + compositionGeneration 精确对应 Core 编辑状态；
// displayText 确保显示文本一致。
export interface EditorLayoutIdentity {
  readonly revision: number
  readonly generation: number
  readonly compositionGeneration: number
  readonly displayText: string
}

// Issue #629 评论18：NavigationLine 直接复用 LineLayout（已含 breakKind + caretStops）。
// 不再自己定义 LineCaretStop / NavigationLine。
export type NavigationLine = LineLayout

// 带身份的行布局状态。
export interface LineLayoutState extends EditorLayoutIdentity {
  readonly contentWidth: number
  readonly fontSize: number
  readonly lines: NavigationLine[]
}

// 等待条目。
interface Waiter {
  identity: EditorLayoutIdentity
  resolve: (state: LineLayoutState | null) => void
}

export class LineNavigationResolver {
  private state: LineLayoutState | null = null
  private waiters: Waiter[] = []

  /** SujianEditor 在渲染结果更新后调用，发布最新行布局（带身份）。传 null 清空。 */
  updateLayout(state: LineLayoutState | null): void {
    this.state = state
    if (state !== null) {
      this.resolveWaiters(state)
    }
  }

  /**
   * 等待匹配编辑状态身份的 layout state。
   * 不加固定 timeout；只有匹配布局发布或 cancelWait 两个出口。
   */
  waitForLayout(identity: EditorLayoutIdentity): Promise<LineLayoutState | null> {
    if (this.state !== null && this.matchesIdentity(this.state, identity)) {
      return Promise.resolve(this.state)
    }
    return new Promise<LineLayoutState | null>((resolve) => {
      this.waiters.push({ identity, resolve })
    })
  }

  /** 取消所有等待（组件销毁/换 session 时调用）。 */
  cancelWait(): void {
    for (const waiter of this.waiters) {
      waiter.resolve(null)
    }
    this.waiters = []
  }

  private resolveWaiters(state: LineLayoutState): void {
    const resolved: Waiter[] = []
    for (const waiter of this.waiters) {
      if (this.matchesIdentity(state, waiter.identity)) {
        waiter.resolve(state)
        resolved.push(waiter)
      }
    }
    for (const w of resolved) {
      const idx = this.waiters.indexOf(w)
      if (idx >= 0) {
        this.waiters.splice(idx, 1)
      }
    }
  }

  private matchesIdentity(state: LineLayoutState, identity: EditorLayoutIdentity): boolean {
    return state.revision === identity.revision
      && state.generation === identity.generation
      && state.compositionGeneration === identity.compositionGeneration
      && state.displayText === identity.displayText
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
   * soft-wrap 边界上同一 offset 出现在两行时用 affinity 区分：
   *   Upstream → 归上一行（行末），Downstream → 归下一行（行首）。
   */
  static getCurrentLineIndex(state: LineLayoutState, position: VisualCaretPosition): number {
    if (state.lines.length === 0) {
      return -1
    }
    const lineRanges = state.lines.map(l => ({
      start: l.startUtf16,
      end: l.endUtf16,
      breakKind: l.breakKind,
    }))
    return resolveVisualLineIndex(lineRanges, position)
  }

  /** 上一行行号。已在第一行时返回 -1。 */
  static getPreviousLineIndex(state: LineLayoutState, position: VisualCaretPosition): number {
    const currentIdx = this.getCurrentLineIndex(state, position)
    if (currentIdx <= 0) {
      return -1
    }
    return currentIdx - 1
  }

  /** 下一行行号。已在最后一行时返回 -1。 */
  static getNextLineIndex(state: LineLayoutState, position: VisualCaretPosition): number {
    const currentIdx = this.getCurrentLineIndex(state, position)
    if (currentIdx < 0 || currentIdx >= state.lines.length - 1) {
      return -1
    }
    return currentIdx + 1
  }

  /**
   * Issue #629 R7 任务B：纯函数 — 根据 offset 在指定行的 soft-wrap 边界位置，
   * 返回正确的 VisualCaretPosition。
   * 
   * SoftWrap 起点（行首，来自上一行软折）：Downstream（光标在此行）。
   * SoftWrap 终点（行末，折到下一行）：Upstream（光标归上一行末尾）。
   * HardBreak / EndOfText 行末：Downstream。
   */
  static positionForOffsetInLine(state: LineLayoutState, lineIndex: number, utf16Offset: number): VisualCaretPosition {
    if (lineIndex < 0 || lineIndex >= state.lines.length) {
      return { utf16Offset, affinity: CaretAffinity.Downstream }
    }
    const line = state.lines[lineIndex]
    // 行首：SoftWrap 起点 → Downstream（光标在此行开始处）。
    if (utf16Offset === line.startUtf16) {
      return { utf16Offset, affinity: CaretAffinity.Downstream }
    }
    // 行末：SoftWrap 终点 → Upstream（光标归上一行末尾）。
    if (utf16Offset === line.endUtf16) {
      if (line.breakKind === LineBreakKind.SoftWrap) {
        return { utf16Offset, affinity: CaretAffinity.Upstream }
      }
      return { utf16Offset, affinity: CaretAffinity.Downstream }
    }
    // 其他位置：Downstream。
    return { utf16Offset, affinity: CaretAffinity.Downstream }
  }

  /**
   * Issue #629 R11 评论5329310563 第1项：水平方向 grapheme 移动到达目标 offset 时，
   * 根据到达方向决定 soft-wrap 边界的 affinity。
   *
   * 与 positionForOffsetInLine 互补：positionForOffsetInLine 已知目标行号（行导航场景），
   * 本方法只看目标 offset 是否落在 SoftWrap 行尾，按移动方向决定 affinity：
   *   - Right 到达 SoftWrap 行尾 -> Upstream（光标归上一行末尾）
   *   - Left 到达 SoftWrap 行尾  -> Downstream（光标归下一行行首）
   *   - 其他位置 -> Downstream
   *
   * 内部委托 editor_layout_math.positionForHorizontalArrival 纯函数（可被 Node 测试直接导入）。
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
    const lineIdx = this.getCurrentLineIndex(state, position)
    if (lineIdx < 0 || lineIdx >= state.lines.length) {
      return 0
    }
    const stops = state.lines[lineIdx].caretStops
    return horizontalForOffset(stops, position.utf16Offset)
  }

  /**
   * Issue #629 评论18：getNearestOffsetAtX 直接调用 offsetForHorizontal（统一算法）。
   * 不再各自实现二分。
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
}
