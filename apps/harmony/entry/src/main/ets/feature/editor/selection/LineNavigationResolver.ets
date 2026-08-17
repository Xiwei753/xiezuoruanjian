// LineNavigationResolver — 行级导航解析器。
// Issue #629 评论11 第2项：Up/Down/Home/End 不再在入队前读旧布局计算目标位置。
// 行导航命令（lineUp/lineDown/lineHome/lineEnd）在 Dispatcher 唯一队列出队时
// 通过此 resolver 基于"最新 Core snapshot + 最新行布局"计算目标。
//
// 由 SujianEditor 持有 renderResult；EditorSemanticDispatcher 持有本 resolver。
// SujianEditor 在 aboutToAppear 时通过 dispatcher.registerLineResolver(this) 注册，
// aboutToDisappear 时 unregisterLineResolver()。

import { LineLayout } from '../render/editor_render_geometry'

export class LineNavigationResolver {
  private renderResultLines: LineLayout[] = []

  /** SujianEditor 在渲染结果更新后调用，同步最新行布局。 */
  updateLayout(lines: LineLayout[]): void {
    this.renderResultLines = lines
  }

  /** 返回当前行布局（只读引用）。 */
  getLines(): LineLayout[] {
    return this.renderResultLines
  }

  /** 找当前光标 UTF-16 offset 所在行号。返回 -1 表示无行信息。 */
  getCurrentLineIndex(cursorUtf16: number): number {
    if (this.renderResultLines.length === 0) {
      return -1
    }
    for (let i = 0; i < this.renderResultLines.length; i++) {
      const line = this.renderResultLines[i]
      if (cursorUtf16 >= line.startUtf16 && cursorUtf16 <= line.endUtf16) {
        return i
      }
    }
    // 越界时归最后一行
    return this.renderResultLines.length - 1
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
    const currentIdx = this.getCurrentLineIndex(cursorUtf16)
    if (currentIdx < 0 || currentIdx >= this.renderResultLines.length - 1) {
      return -1
    }
    return currentIdx + 1
  }

  /** 指定行的行首 UTF-16 offset。行号越界时返回 0。 */
  getLineStart(lineIndex: number): number {
    if (lineIndex < 0 || lineIndex >= this.renderResultLines.length) {
      return 0
    }
    return this.renderResultLines[lineIndex].startUtf16
  }

  /** 指定行的行末 UTF-16 offset。行号越界时返回 0。 */
  getLineEnd(lineIndex: number): number {
    if (lineIndex < 0 || lineIndex >= this.renderResultLines.length) {
      return 0
    }
    return this.renderResultLines[lineIndex].endUtf16
  }
}
