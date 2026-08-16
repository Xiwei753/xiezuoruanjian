// EditorSemanticDispatcher — 统一语义输入调度器。
// 物理键盘、IME、鼠标/触摸选择都从这里排队；真正出队后再读 Coordinator 最新 snapshot。
// 把"读取当前 selection/cursor → 决定 insert/replace/delete/setSelection → 发 Core 命令"
// 整个过程放进同一个语义输入队列，避免并发命令拿到同一个旧 cursor。
//
// Issue #629 评论6 Part A：统一串行边界。
// HarmonyImeConnection 不再自己拥有另一套编辑顺序状态（imeQueue），
// 物理键盘也不再直接调用 EditorInputAdapter/SelectionController。
// 所有语义输入（insertText/setSelection/ime*/grapheme 导航/paste）
// 都经此 Dispatcher 排队，出队执行时才读 snapshot。
//
// Issue #629 评论 9 第 1 项：composition 唯一真源是 Core snapshot。
// - inputAdapter.isComposing() 读 coordinator.getSnapshot().composition，不再有 Harmony 第二套状态。
// - composition 命令出队时从 snapshot.composition 取 sessionId/generation/preeditText，
//   不本地持有 compositionSession，不自造 generation。
//
// Issue #629 评论7 第2项：HarmonyImeConnection 变纯协议翻译器。
// - HarmonyImeConnection 只派发 imePreviewText/imeCommitText/imeCancelPreview（不判断 composition 状态）。
// - 本 Dispatcher 在唯一队列出队时查 inputAdapter.isComposing()（Core snapshot 驱动状态）决定
//   begin/update/finish/cancel。事件顺序和 composition 状态判断属于同一条串行边界。
//
// Issue #629 评论7 第6项：软键盘删除统一 Core grapheme。
// - 删除 {kind:'delete',direction} 命令变体和 inputAdapter.onDelete 调用。
// - graphemeBackspace/graphemeDelete 用 Core previousGraphemeBoundary/nextGraphemeBoundary（Part E 已暴露），
//   不再用 UTF-16 ±1 猜字符边界（避免切进组合附加符 / ZWJ emoji）。
//
// Issue #629 评论8 第3项：close 生命周期入口。
// - seal()/unseal()：close 开始后拒绝新的普通输入命令，但已排队命令仍可 drain。
// - finishActiveComposition()：在唯一队列里执行，出队时查 isComposing()（读 Core snapshot），
//   有 composition 就用 Core snapshot 当前 preedit 调 onCompositionFinish(currentPreedit)，
//   没有就 no-op。
//   seal 后仍可入队（它是关闭链的一部分），保证最后中文 preedit 提交进正文再保存。
//
// Issue #629 评论8 第4项：imeSetSelection 语义命令。
// - IME 来源的 selection 不再复用普通 setSelection/dragSelect，改派发 imeSetSelection（只传 start/end UTF-16）。
// - 出队后判断 composition：无 composition 按普通 selection；有 composition 且 selection 在
//   composition 显示区域内 → 换算成 preedit 内 preeditCursorUtf16 走 composition update；
//   移出 composition 区域 → 同一 thunk 内先 finish 再普通 selection。

import { EditorInputAdapter } from './EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { SerialCommandQueue } from '../session/editor_patch_logic'
import { TextOffsetMapper } from './TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorEditResult } from '../../../corebridge/dto/EditorDtos'

// 语义命令类型：把物理键盘/IME/鼠标/触摸的输入统一成语义命令。
// 出队执行时才读 Coordinator snapshot，避免并发命令拿到同一个旧 cursor/selection。
// Issue #629 评论7 第2项：composition 命令改为 imePreviewText/imeCommitText/imeCancelPreview，
// 出队时查 isComposing() 决定 begin/update/finish/cancel（不在 HarmonyImeConnection 队列外判断）。
// Issue #629 评论7 第6项：删除 {kind:'delete',direction}，软键盘删除统一 graphemeBackspace/graphemeDelete。
export type SemanticEditCommand =
  | { kind: 'insertText'; text: string }
  | { kind: 'setSelection'; utf16Offset: number }
  | { kind: 'dragSelect'; utf16Anchor: number; utf16Head: number }
  | { kind: 'selectAll' }
  | { kind: 'imePreviewText'; text: string }
  | { kind: 'imeCommitText'; text: string }
  | { kind: 'imeCancelPreview' }
  | { kind: 'paste'; text: string }
  // grapheme-based navigation (Part E)
  | { kind: 'graphemeBackspace' }
  | { kind: 'graphemeDelete' }
  | { kind: 'graphemeLeft'; extend: boolean }
  | { kind: 'graphemeRight'; extend: boolean }
  // Issue #629 评论8 第4项：IME 来源的 selection 语义命令（start/end 是 UTF-16 code unit offset，
  // 显示文本坐标）。composition 内外判断由本 Dispatcher 在唯一队列出队时做。
  | { kind: 'imeSetSelection'; utf16Start: number; utf16End: number }

export class EditorSemanticDispatcher {
  private inputAdapter: EditorInputAdapter
  private selectionController: SelectionController
  private coordinator: EditorSessionCoordinator
  // 统一串行队列：物理键盘/IME/鼠标/触摸都从这里排队。
  // 出队执行时才读 Coordinator snapshot（前一条命令已完成并更新 state）。
  private queue: SerialCommandQueue = new SerialCommandQueue()
  // Issue #629 评论8 第3项：close 开始后 sealed，拒绝新的普通输入命令（已排队命令仍可 drain）。
  private sealed: boolean = false

  constructor(inputAdapter: EditorInputAdapter, selectionController: SelectionController, coordinator: EditorSessionCoordinator) {
    this.inputAdapter = inputAdapter
    this.selectionController = selectionController
    this.coordinator = coordinator
  }

  /**
   * 派发一条语义命令。命令在出队执行时才读 Coordinator 最新 snapshot。
   * 返回的 Promise 在命令完成时 resolve/reject，与命令结果一致。
   *
   * Issue #629 评论8 第3项：seal 后拒绝新的普通输入命令（返回 SEALED，不排队）。
   * 已排队命令不受影响，仍可 drain（flush/whenIdle 照常）。
   */
  dispatch(cmd: SemanticEditCommand): Promise<ResultEnvelope<EditorEditResult>> {
    if (this.sealed) {
      return Promise.resolve({
        success: false,
        errorCode: 'SEALED',
        warnings: [],
        changedPaths: [],
        changedEntities: []
      })
    }
    return this.queue.enqueue<ResultEnvelope<EditorEditResult>>(async () => {
      return this.executeCommand(cmd)
    })
  }

  /**
   * 派发一条语义命令，吞掉异常（用于 fire-and-forget 场景：物理键盘/鼠标/触摸）。
   * 错误已由 EditorInputAdapter/Coordinator 处理（recover），不阻塞后续命令。
   */
  dispatchFireAndForget(cmd: SemanticEditCommand): void {
    this.dispatch(cmd).then(() => {}, (_e: Object) => {
      // 命令失败不阻塞后续；错误已由 EditorInputAdapter/Coordinator 处理（recover）
    })
  }

  // Issue #629 评论8 第3项：close 生命周期入口。
  // seal：close 开始后拒绝新的普通输入命令（dispatch 返回 SEALED，不排队）。
  // unseal：保存失败不离开时恢复输入（页面继续编辑）。
  seal(): void {
    this.sealed = true
  }

  unseal(): void {
    this.sealed = false
  }

  isSealed(): boolean {
    return this.sealed
  }

  /**
   * Issue #629 评论8 第3项：在 dispatcher 唯一队列里 finish 当前 composition。
   *
   * 出队时查 inputAdapter.isComposing()（读 Core snapshot.composition）：
   * - 有 composition：用 Core snapshot 当前 preedit 调 onCompositionFinish(currentPreedit)
   *   （确认原样提交，preedit 落进 committed 正文，随后的保存不会漏掉最后预输入）；
   * - 没有 composition：no-op（返回 success）。
   *
   * 与普通输入不同：seal 后仍能入队——它是关闭链的一部分（seal → finishActiveComposition →
   * drain → coordinator idle → 保存 → detach → close）。
   */
  finishActiveComposition(): Promise<ResultEnvelope<EditorEditResult>> {
    return this.queue.enqueue<ResultEnvelope<EditorEditResult>>(async () => {
      if (!this.inputAdapter.isComposing()) {
        return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
      }
      return this.inputAdapter.finishActiveComposition()
    })
  }

  // 出队后真正执行：此时 snapshot 已是最新（前一条命令已完成并更新 state）
  // Issue #629 评论7 第2项：ime* 命令出队时查 inputAdapter.isComposing()（Core snapshot 驱动状态）
  // 决定 begin/update/finish/cancel。事件顺序和 composition 状态判断属于同一条串行边界。
  private async executeCommand(cmd: SemanticEditCommand): Promise<ResultEnvelope<EditorEditResult>> {
    switch (cmd.kind) {
      case 'insertText':
        return this.inputAdapter.onTextInput(cmd.text)
      case 'setSelection':
        return this.selectionController.onTap(cmd.utf16Offset)
      case 'dragSelect':
        return this.selectionController.onDragSelect(cmd.utf16Anchor, cmd.utf16Head)
      case 'selectAll':
        return this.selectionController.selectAll()
      case 'imePreviewText':
        return this.executeImePreviewText(cmd.text)
      case 'imeCommitText':
        return this.executeImeCommitText(cmd.text)
      case 'imeCancelPreview':
        return this.executeImeCancelPreview()
      case 'imeSetSelection':
        return this.executeImeSetSelection(cmd.utf16Start, cmd.utf16End)
      case 'paste':
        return this.inputAdapter.onPaste(cmd.text)
      case 'graphemeBackspace':
        return this.executeGraphemeBackspace()
      case 'graphemeDelete':
        return this.executeGraphemeDelete()
      case 'graphemeLeft':
        return this.executeGraphemeLeft(cmd.extend)
      case 'graphemeRight':
        return this.executeGraphemeRight(cmd.extend)
    }
  }

  // imePreviewText：出队时查 isComposing() 决定 begin+update 还是只 update。
  // - 未开始 composition：await onCompositionBegin 成功后紧接 await onCompositionUpdate。
  //   begin 失败时不 update（不让 update 读到 sessionId=0/generation=0）。
  // - 已开始 composition：只 await onCompositionUpdate。
  // 顺序执行（begin→update 在同一出队 thunk 内 await 串行），不派生新队列任务。
  private async executeImePreviewText(text: string): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.inputAdapter.isComposing()) {
      // composition 开始：先 begin，成功后才 update。
      const beginResult = await this.inputAdapter.onCompositionBegin()
      if (!beginResult.success) {
        // begin 失败：不 update。避免 update 读到 sessionId=0/generation=0。
        return beginResult
      }
      // begin 成功后紧接 update（同一出队 thunk 内 await 串行）。
      return this.inputAdapter.onCompositionUpdate(text)
    }
    // 已开始 composition：只 update。
    return this.inputAdapter.onCompositionUpdate(text)
  }

  // imeCommitText：出队时查 isComposing() 决定 finish 还是普通插入。
  // - 有 composition：await onCompositionFinish（composition commit）。
  // - 无 composition：await onTextInput（普通文字插入）。
  private async executeImeCommitText(text: string): Promise<ResultEnvelope<EditorEditResult>> {
    if (this.inputAdapter.isComposing()) {
      return this.inputAdapter.onCompositionFinish(text)
    }
    return this.inputAdapter.onTextInput(text)
  }

  // imeCancelPreview：出队时查 isComposing() 决定 cancel 还是 no-op。
  // - 有 composition：await onCompositionCancel。
  // - 无 composition：no-op（返回 success）。
  private async executeImeCancelPreview(): Promise<ResultEnvelope<EditorEditResult>> {
    if (this.inputAdapter.isComposing()) {
      return this.inputAdapter.onCompositionCancel()
    }
    // 无 composition：no-op（不设 data，调用方据此知道无编辑结果）
    return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
  }

  /**
   * Issue #629 评论8 第4项：IME selection 语义命令（出队执行）。
   *
   * start/end 是 IME 给的显示文本 UTF-16 code unit offset（composition 活跃时显示文本 =
   * committed text 在 replace range 处插入 preedit 的投影）。出队后判断 composition：
   *
   * - 无 composition：按普通 selection 处理（与 setSelection/dragSelect 同语义）。
   * - 有 composition 且 selection 完全落在 composition 显示区域内：把显示坐标换算成 preedit 内
   *   preeditCursorUtf16，用当前 preedit 文本调 composition update，让 Core 更新
   *   composition cursor/generation（不改 committed selection）。
   * - selection 移出 composition 区域（含部分越界）：同一 thunk 内先 finish 当前 composition，
   *   再把显示坐标映射到 committed 文本坐标，执行普通 selection。
   */
  private async executeImeSetSelection(utf16Start: number, utf16End: number): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    if (!this.inputAdapter.isComposing() || snap.composition === null || snap.composition === undefined) {
      // 无 composition：普通 selection（start/end 即 committed 文本坐标）。
      if (utf16Start === utf16End) {
        return this.selectionController.onTap(utf16Start)
      }
      return this.selectionController.onDragSelect(utf16Start, utf16End)
    }
    const comp = snap.composition
    const committedText: string = snap.text
    // composition 显示区域（UTF-16）：[compStartUtf16, compStartUtf16 + preeditUtf16Len)
    const compStartUtf16: number = TextOffsetMapper.utf8ToUtf16(committedText, comp.replaceByteStart)
    const preeditUtf16Len: number = comp.preeditText.length
    const compEndDisplay: number = compStartUtf16 + preeditUtf16Len
    const start: number = Math.min(utf16Start, utf16End)
    const end: number = Math.max(utf16Start, utf16End)

    if (start >= compStartUtf16 && end <= compEndDisplay) {
      // selection 完全在 composition 显示区域内：换算成 preedit 内 cursor，走 composition update。
      const cursorInPreedit: number = end - compStartUtf16
      // 同一 preedit 文本 + 新 cursor：Core 更新 composition cursor/generation。
      return this.inputAdapter.onCompositionUpdate(comp.preeditText, cursorInPreedit)
    }

    // selection 移出 composition 区域：先 finish 当前 composition（preedit 提交进正文），
    // 再执行普通 selection。整个过程仍在同一 dispatcher thunk 内顺序 await。
    const finishResult = await this.inputAdapter.finishActiveComposition()
    if (!finishResult.success) {
      return finishResult
    }
    // finish 后 coordinator snapshot 已更新：finish 把 [replaceStart, replaceEnd) 替换为
    // preedit，committed 正文恰好等于之前的显示文本（prefix + preedit + suffix）。
    // 因此 IME 给的显示坐标就是 committed 文本坐标（恒等映射），直接按普通 selection 处理，
    // 不再换算（旧公式 d - preeditLen + (end-start) 会把光标放错位置——它假设被替换区还在）。
    if (start === end) {
      return this.selectionController.onTap(start)
    }
    return this.selectionController.onDragSelect(start, end)
  }

  // grapheme-based backspace: 用 Core previous_grapheme_boundary 找前一个 grapheme 边界
  private async executeGraphemeBackspace(): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    const sel = this.inputAdapter.currentSelectionUtf8()
    if (sel.end > sel.start) {
      // 有选区：删除整个选区
      return this.coordinator.delete(sel.start, sel.end, EditorTransactionCause.Delete)
    }
    // 空选区：用 Core grapheme boundary 找前一个边界
    if (sel.start <= 0) {
      return { success: false, errorCode: 'NO_CHAR_TO_DELETE', warnings: [], changedPaths: [], changedEntities: [] }
    }
    const prevBoundaryResult = await this.coordinator.previousGraphemeBoundary(sel.start)
    if (!prevBoundaryResult.success || prevBoundaryResult.data === undefined || prevBoundaryResult.data === null) {
      return prevBoundaryResult as ResultEnvelope<EditorEditResult>
    }
    const prevByte = prevBoundaryResult.data
    return this.coordinator.delete(prevByte, sel.start, EditorTransactionCause.Delete)
  }

  // grapheme-based delete: 用 Core next_grapheme_boundary 找后一个 grapheme 边界
  private async executeGraphemeDelete(): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    const sel = this.inputAdapter.currentSelectionUtf8()
    if (sel.end > sel.start) {
      return this.coordinator.delete(sel.start, sel.end, EditorTransactionCause.Delete)
    }
    const text = snap.text
    const textByteLen = TextOffsetMapper.utf16ToUtf8(text, text.length)
    if (sel.start >= textByteLen) {
      return { success: false, errorCode: 'NO_CHAR_TO_DELETE', warnings: [], changedPaths: [], changedEntities: [] }
    }
    const nextBoundaryResult = await this.coordinator.nextGraphemeBoundary(sel.start)
    if (!nextBoundaryResult.success || nextBoundaryResult.data === undefined || nextBoundaryResult.data === null) {
      return nextBoundaryResult as ResultEnvelope<EditorEditResult>
    }
    const nextByte = nextBoundaryResult.data
    return this.coordinator.delete(sel.start, nextByte, EditorTransactionCause.Delete)
  }

  // grapheme-based left arrow: 用 Core previous_grapheme_boundary
  // extend=true（shift+left）：保持 anchor，head 左移到前一个 grapheme 边界
  // extend=false：有选区时移到选区开头（collapse），空选区时左移一个 grapheme
  private async executeGraphemeLeft(extend: boolean): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    const cursorByte = snap.cursor
    const anchorByte = snap.selectionAnchor
    if (extend) {
      // shift+left: 保持 anchor，head 左移到前一个 grapheme 边界
      const headByte = snap.cursor
      if (headByte <= 0) {
        return this.coordinator.setSelection(anchorByte, 0)
      }
      const prevResult = await this.coordinator.previousGraphemeBoundary(headByte)
      if (!prevResult.success || prevResult.data === undefined || prevResult.data === null) {
        return prevResult as ResultEnvelope<EditorEditResult>
      }
      return this.coordinator.setSelection(anchorByte, prevResult.data)
    }
    // 非扩展：有选区时移到选区开头，空选区时左移一个 grapheme
    if (anchorByte !== cursorByte) {
      const selStart = Math.min(anchorByte, cursorByte)
      return this.coordinator.setSelection(selStart, selStart)
    }
    if (cursorByte <= 0) {
      return this.coordinator.setSelection(0, 0)
    }
    const prevResult = await this.coordinator.previousGraphemeBoundary(cursorByte)
    if (!prevResult.success || prevResult.data === undefined || prevResult.data === null) {
      return prevResult as ResultEnvelope<EditorEditResult>
    }
    return this.coordinator.setSelection(prevResult.data, prevResult.data)
  }

  // grapheme-based right arrow: 用 Core next_grapheme_boundary
  // extend=true（shift+right）：保持 anchor，head 右移到后一个 grapheme 边界
  // extend=false：有选区时移到选区末尾（collapse），空选区时右移一个 grapheme
  private async executeGraphemeRight(extend: boolean): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    const text = snap.text
    const textByteLen = TextOffsetMapper.utf16ToUtf8(text, text.length)
    const cursorByte = snap.cursor
    const anchorByte = snap.selectionAnchor
    if (extend) {
      // shift+right: 保持 anchor，head 右移到后一个 grapheme 边界
      const headByte = snap.cursor
      if (headByte >= textByteLen) {
        return this.coordinator.setSelection(anchorByte, textByteLen)
      }
      const nextResult = await this.coordinator.nextGraphemeBoundary(headByte)
      if (!nextResult.success || nextResult.data === undefined || nextResult.data === null) {
        return nextResult as ResultEnvelope<EditorEditResult>
      }
      return this.coordinator.setSelection(anchorByte, nextResult.data)
    }
    if (anchorByte !== cursorByte) {
      const selEnd = Math.max(anchorByte, cursorByte)
      return this.coordinator.setSelection(selEnd, selEnd)
    }
    if (cursorByte >= textByteLen) {
      return this.coordinator.setSelection(textByteLen, textByteLen)
    }
    const nextResult = await this.coordinator.nextGraphemeBoundary(cursorByte)
    if (!nextResult.success || nextResult.data === undefined || nextResult.data === null) {
      return nextResult as ResultEnvelope<EditorEditResult>
    }
    return this.coordinator.setSelection(nextResult.data, nextResult.data)
  }

  // flush: 等待所有排队命令完成（供页面退出/退后台事务用）
  async flush(): Promise<void> {
    return this.queue.whenIdle()
  }

  // 队列状态
  isIdle(): boolean {
    return this.queue.isIdle()
  }

  queueSize(): number {
    return this.queue.size()
  }

  private noSession(): ResultEnvelope<EditorEditResult> {
    return { success: false, errorCode: 'NO_SESSION', warnings: [], changedPaths: [], changedEntities: [] }
  }
}
