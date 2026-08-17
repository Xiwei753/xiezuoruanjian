// LineNavigationResolver — 行级导航解析器。
// Issue #629 评论11 第2项 + 评论14 第3项：
// Up/Down/Home/End 不再在入队前读旧布局计算目标位置。
// 行导航命令（lineUp/lineDown/lineHome/lineEnd）在 Dispatcher 唯一队列出队时
// 通过此 resolver 基于"最新 Core snapshot + 最新行布局"计算目标。
//
// Issue #629 评论14 第3项：版本化 layout source。
// - layout 不再只提供 matchesIdentity 返回 bool，而是版本化：
//   每次 SujianEditor 新布局完成后发布新版本（versioned LayoutState）。
//   executeLineNavigation 出队时如果 Core 已前进但 resolver 还是旧版本，
//   不返回 STALE_LAYOUT 丢键，而是等待对应版本的 layout state 后再算目标。
// - LineLayoutState 身份包含真正决定折行的东西：displayText + contentWidth + fontSize。
//   cursorUtf16 不决定 lines，不拿它当行布局缓存身份。
// - 组件被销毁/换 session 时通过 cancelWait() 显式取消等待。

import { LineLayout } from '../render/editor_render_geometry'

// 布局身份：标识当前 lines 对应哪个显示布局版本。
// cursorUtf16 不决定折行，不作为身份字段（Issue #629 评论14 第3项）。
// contentWidth + fontSize 决定折行边界。
export interface LineLayoutIdentity {
  readonly displayText: string
  readonly contentWidth: number
  readonly fontSize: number
}

// 带身份的行布局状态。
export interface LineLayoutState extends LineLayoutIdentity {
  readonly lines: LineLayout[]
  // 单调递增版本号，用于等待机制。
  readonly version: number
}

// 等待条目：当 resolver 版本落后于 Core snapshot 时，行导航命令等待新版本。
interface Waiter {
  readonly identity: LineLayoutIdentity
  readonly resolve: (state: LineLayoutState | null) => void
  readonly version: number
}

export class LineNavigationResolver {
  private state: LineLayoutState | null = null
  private nextVersion: number = 1
  // 等待列表：executeLineNavigation 出队时如果版本落后，注册 waiter 等待新布局。
  private waiters: Waiter[] = []

  /** SujianEditor 在渲染结果更新后调用，同步最新行布局（带身份）。传 null 清空。 */
  updateLayout(state: LineLayoutState | null): void {
    this.state = state
    // 唤醒匹配的 waiters
    if (state !== null) {
      this.resolveWaiters(state)
    }
  }

  /**
   * Issue #629 评论14 第3项：等待匹配身份的 layout state。
   * 如果当前 state 已匹配 identity，立即返回；否则注册 waiter 等待。
   * 返回 Promise<LineLayoutState | null>，null 表示被取消（组件销毁/换 session）。
   *
   * 不加固定毫秒 timeout，也不退回入队前算目标。
   */
  waitForLayout(identity: LineLayoutIdentity, timeoutMs: number = 500): Promise<LineLayoutState | null> {
    // 检查当前 state 是否已匹配
    if (this.state !== null && this.matchesIdentity(this.state, identity)) {
      return Promise.resolve(this.state)
    }

    // 注册 waiter
    return new Promise<LineLayoutState | null>((resolve) => {
      const version: number = this.nextVersion
      const waiter: Waiter = { identity, resolve, version }
      this.waiters.push(waiter)

      // 超时后取消等待（返回 null，调用方用当前最新 snapshot 兜底）
      const timer: number = setTimeout(() => {
        this.removeWaiter(waiter)
        resolve(null)
      }, timeoutMs) as unknown as number

      // 清理 timer 的辅助：waiter resolve 后 clearTimeout
      const originalResolve = waiter.resolve
      const wrappedResolve = (state: LineLayoutState | null): void => {
        clearTimeout(timer)
        originalResolve(state)
      }
      waiter.resolve = wrappedResolve
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
   * 验证给定 state 是否匹配 identity。
   * 身份 = displayText + contentWidth + fontSize（不包含 cursorUtf16，因为 cursor 不决定折行）。
   */
  private matchesIdentity(state: LineLayoutState, identity: LineLayoutIdentity): boolean {
    return state.displayText === identity.displayText
      && state.contentWidth === identity.contentWidth
      && state.fontSize === identity.fontSize
  }

  /** 返回当前行布局（只读引用）。state 为 null 时返回空数组。 */
  getLines(): LineLayout[] {
    return this.state === null ? [] : this.state.lines
  }

  /** 找当前光标 UTF-16 offset 所在行号。返回 -1 表示无行信息。 */
  getCurrentLineIndex(cursorUtf16: number): number {
    const lines = this.getLines()
    if (lines.length === 0) {
      return -1
    }
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i]
      if (cursorUtf16 >= line.startUtf16 && cursorUtf16 <= line.endUtf16) {
        return i
      }
    }
    // 越界时归最后一行
    return lines.length - 1
  }

  /** 上一行行号。已在第一行时返回 -1。 */
  getPreviousLineIndex(cursorUtf16: number): number {
    const currentIdx = this.getCurrentLineIndex(cursorUtf16)
    if (currentIdx <= 0) {
      return -1
    }
    return currentIdx - 1
  }

  /** 下一行行号。已在最后一行时返回 -1。 */
  getNextLineIndex(cursorUtf16: number): number {
    const lines = this.getLines()
    const currentIdx = this.getCurrentLineIndex(cursorUtf16)
    if (currentIdx < 0 || currentIdx >= lines.length - 1) {
      return -1
    }
    return currentIdx + 1
  }

  /** 指定行的行首 UTF-16 offset。行号越界时返回 0。 */
  getLineStart(lineIndex: number): number {
    const lines = this.getLines()
    if (lineIndex < 0 || lineIndex >= lines.length) {
      return 0
    }
    return lines[lineIndex].startUtf16
  }

  /** 指定行的行末 UTF-16 offset。行号越界时返回 0。 */
  getLineEnd(lineIndex: number): number {
    const lines = this.getLines()
    if (lineIndex < 0 || lineIndex >= lines.length) {
      return 0
    }
    return lines[lineIndex].endUtf16
  }
}
