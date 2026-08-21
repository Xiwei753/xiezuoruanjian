// line_layout_store.ts — 与 ArkUI 无关的布局状态/等待核心。
//
// Issue #629 评论21：把 LineNavigationResolver 的 updateLayout/waitForLayout/
// cancelWait/resolveWaiters 搬到独立 .ts，供生产 .ets 和 Node 测试共同使用。
// 不依赖 ArkUI，只依赖 EditorLayoutIdentity + matchesEditorLayoutIdentity。

import {
  matchesEditorLayoutIdentity,
} from './editor_layout_identity.ts'
import type { EditorLayoutIdentity } from './editor_layout_identity.ts'

/** 等待条目。 */
interface Waiter<TState> {
  identity: EditorLayoutIdentity
  resolve: (state: TState | null) => void
}

/**
 * 布局状态存储 + 等待者管理。
 *
 * - update(state)：发布新布局，自动 resolve 匹配的 waiter。
 * - waitFor(identity)：等待匹配 identity 的布局，无固定 timeout。
 * - cancelAll()：取消所有等待（组件销毁/换 session 时调用）。
 */
export class LineLayoutStore<TState extends EditorLayoutIdentity> {
  private state: TState | null = null
  private waiters: Waiter<TState>[] = []

  /** 当前持有的布局状态。 */
  getState(): TState | null {
    return this.state
  }

  /** SujianEditor 在渲染结果更新后调用，发布最新行布局。传 null 清空。 */
  update(state: TState | null): void {
    this.state = state
    if (state !== null) {
      this.resolveWaiters(state)
    }
  }

  /**
   * 等待匹配编辑状态身份的 layout state。
   * 不加固定 timeout；只有匹配布局发布或 cancelAll 两个出口。
   */
  waitFor(identity: EditorLayoutIdentity): Promise<TState | null> {
    if (this.state !== null && matchesEditorLayoutIdentity(this.state, identity)) {
      return Promise.resolve(this.state)
    }
    return new Promise<TState | null>((resolve) => {
      this.waiters.push({ identity, resolve })
    })
  }

  /** 取消所有等待。 */
  cancelAll(): void {
    for (const waiter of this.waiters) {
      waiter.resolve(null)
    }
    this.waiters = []
  }

  private resolveWaiters(state: TState): void {
    const resolved: Waiter<TState>[] = []
    for (const waiter of this.waiters) {
      if (matchesEditorLayoutIdentity(state, waiter.identity)) {
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
}
