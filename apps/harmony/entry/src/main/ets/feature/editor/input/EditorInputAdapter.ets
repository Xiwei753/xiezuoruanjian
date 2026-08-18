// EditorInputAdapter — 把 HarmonyOS 键盘/IME/粘贴翻译成 insert/delete/replace/composition 编辑命令。
// 委托 EditorSessionCoordinator。不自己维护正文或光标。
// cause 由本适配器按输入语义决定（Typing/Delete/Paste/ImeComposition/TypingCommit）。
//
// Issue #629 评论 9 第 1 项：composition 唯一真源是 Core snapshot。
// - 删除本地 EditorImeSession 状态机；isComposing/onCompositionUpdate/Finish/Cancel/
//   finishActiveComposition 全部从 coordinator.getSnapshot().composition 读取
//   sessionId/generation/preeditText。
// - 不本地提前改任何 composition 状态；Core 成功后 Coordinator snapshot 自动反映新 composition。
// - generation/sessionId 完全由 Core 裁判，从 snapshot.composition 读取，不自造、不 ++。
//
// Issue #629 评论 5 第 2 部分：输入尊重当前选区。
// - 加 currentSelectionUtf8()，从 snapshot 的 selectionAnchor/cursor 取当前 Core 选区并 normalize（start <= end）。
// - onTextInput：有选区时走 replace(selectionStart, selectionEnd, text, selectedText, Typing)；只有空选区才 insert。
// - onPaste：同样替换当前选区，不是永远在 cursor 插入。
// - onCompositionBegin：replace range 是当前选区，不是永远 [cursor,cursor)。
// 签名变化：onTextInput/onPaste/onCompositionBegin 不再由调用方传 utf16Offset；
// 由本适配器从 snapshot 选区/cursor 决定。调用方（HarmonyImeConnection）只传语义输入。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { TextOffsetMapper } from './TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorEditResult } from '../../../corebridge/dto/EditorDtos'

/** 当前选区的 UTF-8 byte offset（normalized：start <= end）。 */
export interface SelectionRangeUtf8 {
  readonly start: number
  readonly end: number
}

/**
 * 删除方向：backward = backspace（向前删），forward = delete（向后删）。
 * Issue #629 评论7 第6项：EditorInputAdapter.onDelete 已删除，软键盘删除统一走
 * graphemeBackspace/graphemeDelete（EditorSemanticDispatcher）。此类型保留供 ohosTest 引用。
 */
export type DeleteDirection = 'backward' | 'forward'

export class EditorInputAdapter {
  private coordinator: EditorSessionCoordinator

  constructor(coordinator: EditorSessionCoordinator) {
    this.coordinator = coordinator
  }

  /**
   * 当前选区的 UTF-8 byte offset（normalized：start <= end）。
   * 从 snapshot 的 selectionAnchor/cursor 取：anchor 是选区起点，cursor 是选区终点（head）。
   * normalize：start = min(anchor, cursor), end = max(anchor, cursor)。
   * 没有 snapshot 时返回 { 0, 0 }。
   */
  currentSelectionUtf8(): SelectionRangeUtf8 {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return { start: 0, end: 0 }
    }
    const anchor = snap.selectionAnchor
    const cursor = snap.cursor
    return {
      start: Math.min(anchor, cursor),
      end: Math.max(anchor, cursor),
    }
  }

  /** 当前是否有非空选区（end > start）。 */
  hasSelection(): boolean {
    const sel = this.currentSelectionUtf8()
    return sel.end > sel.start
  }

  /**
   * 普通字符输入。
   * - 有选区：replace(selectionStart, selectionEnd, text, selectedText, Typing) — 替换选区
   * - 空选区：insert(cursor, text, Typing) — 在光标插入
   * 不再由调用方传 utf16Offset；由本适配器从 snapshot 选区/cursor 决定。
   */
  async onTextInput(text: string): Promise<ResultEnvelope<EditorEditResult>> {
    const sel = this.currentSelectionUtf8()
    if (sel.end > sel.start) {
      const selectedText = this.getSelectedTextUtf8(sel)
      return this.coordinator.replace(sel.start, sel.end, text, selectedText, EditorTransactionCause.Typing)
    }
    return this.coordinator.insert(sel.start, text, EditorTransactionCause.Typing)
  }

  /**
   * 粘贴。
   * - 有选区：replace(selectionStart, selectionEnd, text, selectedText, Paste) — 替换选区
   * - 空选区：insert(cursor, text, Paste) — 在光标插入
   * 不再永远在 cursor 插入。
   */
  async onPaste(text: string): Promise<ResultEnvelope<EditorEditResult>> {
    const sel = this.currentSelectionUtf8()
    if (sel.end > sel.start) {
      const selectedText = this.getSelectedTextUtf8(sel)
      return this.coordinator.replace(sel.start, sel.end, text, selectedText, EditorTransactionCause.Paste)
    }
    return this.coordinator.insert(sel.start, text, EditorTransactionCause.Paste)
  }

  /**
   * IME composition 开始：replace range 是当前选区（不是空范围 [cursor,cursor)）。
   * 输入法在选中文本后开始预输入时，应替换该选区。
   *
   * Issue #629 评论 9 第 1 项：不本地持有 compositionSession。
   * Core 返回的 composition 会通过 Coordinator snapshot 反映，后续 isComposing/onCompositionUpdate
   * 等从 snapshot.composition 读取。
   */
  async onCompositionBegin(): Promise<ResultEnvelope<EditorEditResult>> {
    const sel = this.currentSelectionUtf8()
    return this.coordinator.beginComposition(sel.start, sel.end)
  }

  // IME composition preedit 更新。
  // Issue #629 评论 9 第 1 项：从 coordinator.getSnapshot().composition 取 sessionId/generation，
  // 不本地提前改任何状态。Core 成功后 Coordinator snapshot 自动反映新 composition。
  // Issue #629 评论7 第1项：composition cursor 单位是 UTF-16 code unit（preedit.length），
  // 不是 UTF-8 byte offset。光标在预输入末尾，直接传 preedit.length。
  // Issue #629 评论8 第4项：imeSetSelection 在 composition 内移动光标时显式传
  // preeditCursorUtf16（显示坐标换算出的 preedit 内 UTF-16 cursor），省略时默认末尾。
  async onCompositionUpdate(preedit: string, preeditCursorUtf16?: number): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap || snap.composition === null || snap.composition === undefined) {
      return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    const comp = snap.composition
    const cursorUtf16: number = preeditCursorUtf16 === undefined ? preedit.length : preeditCursorUtf16
    return this.coordinator.updateComposition(comp.sessionId, comp.generation, preedit, cursorUtf16)
  }

  // IME composition 提交。committed 是 IME 最终提交文本，不能忽略。
  // - committed == preedit → finishComposition（确认原样提交）
  // - committed != preedit → updateComposition（把 preedit 改成 committed）→ 重新读 snapshot.composition
  //   取新 generation → finishComposition。
  //   禁止 cancel 后 commitText（评论明确说这会 StaleRevision）。
  // Issue #629 评论 9 第 1 项：sessionId/generation/preedit 全部从 Core snapshot 读，不本地持有。
  async onCompositionFinish(committed: string): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap || snap.composition === null || snap.composition === undefined) {
      return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    const comp = snap.composition
    if (committed === comp.preeditText) {
      return this.coordinator.finishComposition(comp.sessionId, comp.generation)
    }
    // committed != preedit：updateComposition 提交最终文本，再用新 generation finishComposition
    // Issue #629 评论7 第1项：composition cursor 单位是 UTF-16 code unit（committed.length）。
    const cursorUtf16 = committed.length
    const updateResult = await this.coordinator.updateComposition(comp.sessionId, comp.generation, committed, cursorUtf16)
    if (!updateResult.success || !updateResult.data) {
      return updateResult
    }
    // update 成功后重新读最新 snapshot.composition 的 generation（Core 已更新）。
    const latestSnap = this.coordinator.getSnapshot()
    const newComp = latestSnap ? latestSnap.composition : null
    if (newComp === null || newComp === undefined) {
      return {
        success: false,
        errorCode: 'NO_COMPOSITION_SESSION',
        warnings: updateResult.warnings,
        changedPaths: updateResult.changedPaths,
        changedEntities: updateResult.changedEntities
      }
    }
    return this.coordinator.finishComposition(newComp.sessionId, newComp.generation)
  }

  async onCompositionCancel(): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap || snap.composition === null || snap.composition === undefined) {
      return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    const comp = snap.composition
    return this.coordinator.cancelComposition(comp.sessionId, comp.generation)
  }

  /**
   * Issue #629 评论8 第3项：close 生命周期收尾入口。
   * - 有活跃 composition：用当前 preedit（从 Core snapshot 读）调 onCompositionFinish
   *   （committed === preedit，确认原样提交，preedit 落进 committed 正文）；
   * - 无 composition：no-op（返回 success）。
   *
   * Issue #629 评论 9 第 1 项：composition 是否活跃由 snapshot.composition 判断，
   * preedit 从 snapshot.composition.preeditText 读，不本地持有。
   */
  async finishActiveComposition(): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap || snap.composition === null || snap.composition === undefined) {
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    }
    return this.onCompositionFinish(snap.composition.preeditText)
  }

  // Issue #629 评论 9 第 1 项：composition 唯一真源是 Core snapshot。
  isComposing(): boolean {
    const snap = this.coordinator.getSnapshot()
    return snap !== null && snap.composition !== null && snap.composition !== undefined
  }

  // Issue #629 评论5324447292 第2项：composition 专用 grapheme 语义操作。
  // 操作 Core 已持有的 preeditText + preeditCursorUtf16，不把 platform raw event 带进 Core，
  // 不创建 Harmony 第二份 preedit；grapheme 边界由 Core 的 Unicode 逻辑裁判。
  // 仅 composition 活跃时调用（由 Dispatcher 出队时判断）。
  async compositionMoveGraphemeLeft(): Promise<ResultEnvelope<EditorEditResult>> {
    return this.coordinator.compositionMoveGraphemeLeft()
  }

  async compositionMoveGraphemeRight(): Promise<ResultEnvelope<EditorEditResult>> {
    return this.coordinator.compositionMoveGraphemeRight()
  }

  async compositionDeleteGraphemeBackward(): Promise<ResultEnvelope<EditorEditResult>> {
    return this.coordinator.compositionDeleteGraphemeBackward()
  }

  async compositionDeleteGraphemeForward(): Promise<ResultEnvelope<EditorEditResult>> {
    return this.coordinator.compositionDeleteGraphemeForward()
  }

  // 当前光标的 UTF-16 code unit offset（供外部查询，如 HarmonyImeConnection 通知 IME 选区）。
  // Issue #629 评论 3：用 snap.cursor（而非 selectionHead）。
  getCurrentCursorUtf16(): number {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return 0
    }
    return TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursor)
  }

  // 当前正文（UTF-16 string，供外部查询）。
  getCurrentText(): string {
    const snap = this.coordinator.getSnapshot()
    return snap ? snap.text : ''
  }

  // 获取选区内文本（UTF-8 byte offset → UTF-16 substring）。用于 replace 的 originalText 参数。
  private getSelectedTextUtf8(sel: SelectionRangeUtf8): string {
    const text = this.getCurrentText()
    const startUtf16 = TextOffsetMapper.utf8ToUtf16(text, sel.start)
    const endUtf16 = TextOffsetMapper.utf8ToUtf16(text, sel.end)
    return text.substring(startUtf16, endUtf16)
  }
}
