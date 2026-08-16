// EditorInputAdapter — 把 HarmonyOS 键盘/IME/粘贴翻译成 insert/delete/replace/composition 编辑命令。
// 委托 EditorSessionCoordinator。不自己维护正文或光标。
// cause 由本适配器按输入语义决定（Typing/Delete/Paste/ImeComposition/TypingCommit）。
// Issue #629 评论 3：composition 用 Core 返回的 compositionSession（不伪造 generation）；
// onCompositionFinish committed!=preedit 改成 updateComposition→新 generation→finishComposition（不 cancel 后 commitText）。
//
// Issue #629 评论 5 第 2 部分：输入尊重当前选区。
// - 加 currentSelectionUtf8()，从 snapshot 的 selectionAnchor/cursor 取当前 Core 选区并 normalize（start <= end）。
// - onTextInput：有选区时走 replace(selectionStart, selectionEnd, text, selectedText, Typing)；只有空选区才 insert。
// - onPaste：同样替换当前选区，不是永远在 cursor 插入。
// - onCompositionBegin：replace range 是当前选区，不是永远 [cursor,cursor)。
// 签名变化：onTextInput/onPaste/onCompositionBegin 不再由调用方传 utf16Offset；
// 由本适配器从 snapshot 选区/cursor 决定。调用方（HarmonyImeConnection）只传语义输入。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { EditorImeSession } from './EditorImeSession'
import { TextOffsetMapper } from './TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorEditResult, CompositionSession } from '../../../corebridge/dto/EditorDtos'

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
  private imeSession: EditorImeSession = new EditorImeSession()

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
   * 从 result.compositionSession 设 imeSession（Core 驱动 composition 会话）。
   */
  async onCompositionBegin(): Promise<ResultEnvelope<EditorEditResult>> {
    const sel = this.currentSelectionUtf8()
    const result = await this.coordinator.beginComposition(sel.start, sel.end)
    if (result.success && result.data && result.data.compositionSession) {
      this.imeSession.setCompositionSession(result.data.compositionSession)
    }
    return result
  }

  // IME composition preedit 更新：从 result.compositionSession 更新 imeSession。
  // Issue #629 评论7 第1项：composition cursor 单位是 UTF-16 code unit（preedit.length），
  // 不是 UTF-8 byte offset。光标在预输入末尾，直接传 preedit.length。
  // Issue #629 评论8 第4项：imeSetSelection 在 composition 内移动光标时显式传
  // preeditCursorUtf16（显示坐标换算出的 preedit 内 UTF-16 cursor），省略时默认末尾。
  async onCompositionUpdate(preedit: string, preeditCursorUtf16?: number): Promise<ResultEnvelope<EditorEditResult>> {
    this.imeSession.updatePreedit(preedit)
    const compId = this.imeSession.getCompositionSessionId()
    const gen = this.imeSession.getGeneration()
    const cursorUtf16: number = preeditCursorUtf16 === undefined ? preedit.length : preeditCursorUtf16
    const result = await this.coordinator.updateComposition(compId, gen, preedit, cursorUtf16)
    if (result.success && result.data && result.data.compositionSession) {
      this.imeSession.setCompositionSession(result.data.compositionSession)
    }
    return result
  }

  // IME composition 提交。committed 是 IME 最终提交文本，不能忽略。
  // - committed == preedit → finishComposition（确认原样提交）
  // - committed != preedit → updateComposition（把 preedit 改成 committed）→ 从 result.compositionSession
  //   取新 generation → finishComposition。
  //   禁止 cancel 后 commitText（评论明确说这会 StaleRevision）。
  async onCompositionFinish(committed: string): Promise<ResultEnvelope<EditorEditResult>> {
    const comp = this.imeSession.finish()
    if (!comp) {
      return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    if (committed === comp.preedit) {
      const result = await this.coordinator.finishComposition(comp.sessionId, comp.generation)
      this.imeSession.clear()
      return result
    }
    // committed != preedit：updateComposition 提交最终文本，再用新 generation finishComposition
    // Issue #629 评论7 第1项：composition cursor 单位是 UTF-16 code unit（committed.length）。
    const cursorUtf16 = committed.length
    const updateResult = await this.coordinator.updateComposition(comp.sessionId, comp.generation, committed, cursorUtf16)
    if (!updateResult.success || !updateResult.data) {
      return updateResult
    }
    const newCompSession: CompositionSession | null = updateResult.data.compositionSession
    if (!newCompSession) {
      return {
        success: false,
        errorCode: 'NO_COMPOSITION_SESSION',
        warnings: updateResult.warnings,
        changedPaths: updateResult.changedPaths,
        changedEntities: updateResult.changedEntities
      }
    }
    this.imeSession.setCompositionSession(newCompSession)
    const finishResult = await this.coordinator.finishComposition(newCompSession.sessionId, newCompSession.generation)
    this.imeSession.clear()
    return finishResult
  }

  async onCompositionCancel(): Promise<ResultEnvelope<EditorEditResult>> {
    const comp = this.imeSession.cancel()
    if (!comp) {
      return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    const result = await this.coordinator.cancelComposition(comp.sessionId, comp.generation)
    this.imeSession.clear()
    return result
  }

  /**
   * Issue #629 评论8 第3项：close 生命周期收尾入口。
   * 内部从 EditorImeSession 当前 preedit / 当前真实 session 走现有 onCompositionFinish()，
   * 不新造 composition 状态机。
   * - 有活跃 composition：用当前 preedit 调 onCompositionFinish（committed === preedit，
   *   确认原样提交，preedit 落进 committed 正文）；
   * - 无 composition：no-op（返回 success）。
   */
  async finishActiveComposition(): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.imeSession.isActive()) {
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    }
    const preedit: string = this.imeSession.getPreedit()
    return this.onCompositionFinish(preedit)
  }

  isComposing(): boolean {
    return this.imeSession.isActive()
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
