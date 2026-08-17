// LineNavigationResolver — 行级导航解析器。
// Issue #629 评论11 第2项 + 评论14 第3项 + 评论15 第3项：
// Up/Down/Home/End 不再在入队前读旧布局计算目标位置。
// 行导航命令（lineUp/lineDown/lineHome/lineEnd）在 Dispatcher 唯一队列出队时
// 通过此 resolver 基于"最新 Core snapshot + 最新行布局"计算目标。
//
// Issue #629 评论15 第3项：彻底删除 500ms timeout。
// - 等待只能有两个出口：SujianEditor 发布与目标编辑状态匹配的新布局；
//   编辑器销毁/换 session 时 cancelWait()。
// - EditorLayoutIdentity 包含 revision + generation + compositionGeneration + displayText，
//   不再用 contentWidth/fontSize 作为身份字段（它们决定折行但不属于编辑状态身份）。
// - 方法改为纯函数式：接收显式 state 参数，不再读取内部 mutable this.state。
//   等待到的那一版 lines 才是真正被使用的。

import { LineLayout } from '../render/editor_render_geometry'

// Issue #629 评论15 第3项：布局身份 = 编辑状态身份。
// revision + generation + compositionGeneration 精确对应 Core 编辑状态；
// displayText 确保显示文本一致（composition 活跃时 committed text 与 display text 不同）。
// contentWidth/fontSize 不在身份中——它们决定折行但不属于 Core 编辑状态身份。
export interface EditorLayoutIdentity {
  readonly revision: number
  readonly generation: number
  readonly compositionGeneration: number
  readonly displayText: string
}

// 带身份的行布局状态。contentWidth/fontSize 保留在这里，因为它们确实决定折行。
// 但它们不由 Dispatcher 猜，也不作为 Dispatcher 请求的目标 identity。
// SujianEditor 发布时携带当前真实 contentWidth/fontSize。
export interface LineLayoutState extends EditorLayoutIdentity {
  readonly contentWidth: number
  readonly fontSize: number
  readonly lines: LineLayout[]
}

// 等待条目：当 resolver 版本落后于 Core snapshot 时，行导航命令等待新版本。
// Issue #629 评论15 第3项：不加固定 timeout；resolve 只在匹配身份的布局发布或 cancelWait 时触发。
interface Waiter {
  identity: EditorLayoutIdentity
  resolve: (state: LineLayoutState | null) => void
}

export class LineNavigationResolver {
  private state: LineLayoutState | null = null
  // 等待列表：executeLineNavigation 出队时如果版本落后，注册 waiter 等待新布局。
  private waiters: Waiter[] = []

  /** SujianEditor 在渲染结果更新后调用，发布最新行布局（带身份）。传 null 清空。 */
  updateLayout(state: LineLayoutState | null): void {
    this.state = state
    // 唤醒匹配的 waiters
    if (state !== null) {
      this.resolveWaiters(state)
    }
  }

  /**
   * Issue #629 评论15 第3项：等待匹配编辑状态身份的 layout state。
   * 如果当前 state 已匹配 identity，立即返回；否则注册 waiter 等待。
   * 返回 Promise<LineLayoutState | null>，null 表示被取消（组件销毁/换 session）。
   *
   * 不加固定毫秒 timeout，也不退回入队前算目标。
   * 等待只能有两个出口：
   * 1. SujianEditor 发布与目标编辑状态匹配的新布局；
   * 2. 编辑器销毁/换 session 时 cancelWait()。
   */
  waitForLayout(identity: EditorLayoutIdentity): Promise<LineLayoutState | null> {
    // 检查当前 state 是否已匹配
    if (this.state !== null && this.matchesIdentity(this.state, identity)) {
      return Promise.resolve(this.state)
    }

    // 注册 waiter（不设 timeout）
    return new Promise<LineLayoutState | null>((resolve) => {
      const waiter: Waiter = { identity, resolve }
      this.waiters.push(waiter)
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
    // 移除已 resolve 的 waiter
    for (const w of resolved) {
      this.removeWaiter(w)
    }
  }

  private removeWaiter(waiter: Waiter): void {
    const idx = this.waiters.indexOf(waiter)
    if (idx >= 0) {
      this.waiters.splice(idx, 1)
    }
  }

  /**
   * 验证给定 state 是否匹配编辑状态身份。
   * 身份 = revision + generation + compositionGeneration + displayText。
   */
  private matchesIdentity(state: LineLayoutState, identity: EditorLayoutIdentity): boolean {
    return state.revision === identity.revision
      && state.generation === identity.generation
      && state.compositionGeneration === identity.compositionGeneration
      && state.displayText === identity.displayText
  }

  // Issue #629 评论15 第3项：所有方法改为纯函数式，接收显式 state 参数。
  // 不再读取内部 mutable this.state，等待到的那一版 lines 才是真正被使用的。

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

  /** 找光标 UTF-16 offset 所在行号。返回 -1 表示无行信息。 */
  static getCurrentLineIndex(state: LineLayoutState, cursorUtf16: number): number {
    if (state.lines.length === 0) {
      return -1
    }
    for (let i = 0; i < state.lines.length; i++) {
      const line = state.lines[i]
      if (cursorUtf16 >= line.startUtf16 && cursorUtf16 <= line.endUtf16) {
        return i
      }
    }
    // 越界时归最后一行
    return state.lines.length - 1
  }

  /** 上一行行号。已在第一行时返回 -1。 */
  static getPreviousLineIndex(state: LineLayoutState, cursorUtf16: number): number {
    const currentIdx = this.getCurrentLineIndex(state, cursorUtf16)
    if (currentIdx <= 0) {
      return -1
    }
    return currentIdx - 1
  }

  /** 下一行行号。已在最后一行时返回 -1。 */
  static getNextLineIndex(state: LineLayoutState, cursorUtf16: number): number {
    const currentIdx = this.getCurrentLineIndex(state, cursorUtf16)
    if (currentIdx < 0 || currentIdx >= state.lines.length - 1) {
      return -1
    }
    return currentIdx + 1
  }
}
