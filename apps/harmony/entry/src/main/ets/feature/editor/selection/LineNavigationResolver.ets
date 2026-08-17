// LineNavigationResolver — 行级导航解析器。
// Issue #629 评论11 第2项：Up/Down/Home/End 不再在入队前读旧布局计算目标位置。
// 行导航命令（lineUp/lineDown/lineHome/lineEnd）在 Dispatcher 唯一队列出队时
// 通过此 resolver 基于"最新 Core snapshot + 最新行布局"计算目标。
//
// Issue #629 评论11 第2项补充：resolver 不只存裸 LineLayout[]，存带身份的布局状态
// （revision + displayText + cursorUtf16 + lines）。身份必须对应 EditorLayoutSnapshot 的
// 显示文本（composition 时显示文本和 committed snap.text 不是同一个坐标系）。
// executeLineNavigation 出队时先验证 resolver identity 与当前 Core snapshot 匹配，
// 不匹配时禁止拿旧 lines 算目标（返回 STALE_LAYOUT），避免 Core snapshot 已前进而
// ArkUI 布局还没刷新时拿到混合坐标。
//
// 由 SujianEditor 持有 renderResult；EditorSemanticDispatcher 持有本 resolver。
// SujianEditor 在 contentWidth > 0 后每次 snapshot/revision/composition 更新都直接
// 基于当前宽度和 measureText 重新计算行布局，同步给 resolver（带身份）。

import { LineLayout } from '../render/editor_render_geometry'

// 布局身份：标识当前 lines 对应哪个显示布局版本。
// - revision：Core snapshot revision
// - displayText：显示文本（composition 活跃时含 preedit，与 committed snap.text 可能不同）
// - cursorUtf16：显示光标 UTF-16 offset（composition 活跃时是 displayCaret 投影）
export interface LineLayoutIdentity {
  readonly revision: number
  readonly displayText: string
  readonly cursorUtf16: number
}

// 带身份的行布局状态。
export interface LineLayoutState extends LineLayoutIdentity {
  readonly lines: LineLayout[]
}

export class LineNavigationResolver {
  private state: LineLayoutState | null = null

  /** SujianEditor 在渲染结果更新后调用，同步最新行布局（带身份）。传 null 清空。 */
  updateLayout(state: LineLayoutState | null): void {
    this.state = state
  }

  /**
   * 验证当前布局身份是否与给定 (revision, displayText, cursorUtf16) 匹配。
   * executeLineNavigation 出队时先调此方法；不匹配返回 false → STALE_LAYOUT。
   * state 为 null 时返回 false（无布局信息）。
   */
  matchesIdentity(revision: number, displayText: string, cursorUtf16: number): boolean {
    if (this.state === null) {
      return false
    }
    return this.state.revision === revision
      && this.state.displayText === displayText
      && this.state.cursorUtf16 === cursorUtf16
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
