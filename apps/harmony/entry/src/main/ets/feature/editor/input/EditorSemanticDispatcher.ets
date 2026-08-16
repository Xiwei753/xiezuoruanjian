// EditorSemanticDispatcher — 统一语义输入调度器。
// 物理键盘、IME、鼠标/触摸选择都从这里排队；真正出队后再读 Coordinator 最新 snapshot。
// 把"读取当前 selection/cursor → 决定 insert/replace/delete/setSelection → 发 Core 命令"
// 整个过程放进同一个语义输入队列，避免并发命令拿到同一个旧 cursor。
//
// Issue #629 评论6 Part A：统一串行边界。
// HarmonyImeConnection 不再自己拥有另一套编辑顺序状态（imeQueue），
// 物理键盘也不再直接调用 EditorInputAdapter/SelectionController。
// 所有语义输入（insertText/delete/setSelection/composition/grapheme 导航/paste）
// 都经此 Dispatcher 排队，出队执行时才读 snapshot。
//
// grapheme-based 导航（graphemeBackspace/graphemeDelete/graphemeLeft/graphemeRight）
// 用 Core previousGraphemeBoundary/nextGraphemeBoundary（Part E 已暴露），
// 不再用 UTF-16 ±1 猜字符边界（避免切进组合附加符 / ZWJ emoji）。

import { EditorInputAdapter, DeleteDirection } from './EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { SerialCommandQueue } from '../session/editor_patch_logic'
import { TextOffsetMapper } from './TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorEditResult } from '../../../corebridge/dto/EditorDtos'

// 语义命令类型：把物理键盘/IME/鼠标/触摸的输入统一成语义命令。
// 出队执行时才读 Coordinator snapshot，避免并发命令拿到同一个旧 cursor/selection。
export type SemanticEditCommand =
  | { kind: 'insertText'; text: string }
  | { kind: 'delete'; direction: DeleteDirection }
  | { kind: 'setSelection'; utf16Offset: number }
  | { kind: 'dragSelect'; utf16Anchor: number; utf16Head: number }
  | { kind: 'selectAll' }
  | { kind: 'compositionBegin' }
  | { kind: 'compositionUpdate'; preedit: string }
  | { kind: 'compositionFinish'; committed: string }
  | { kind: 'compositionCancel' }
  | { kind: 'paste'; text: string }
  // grapheme-based navigation (Part E)
  | { kind: 'graphemeBackspace' }
  | { kind: 'graphemeDelete' }
  | { kind: 'graphemeLeft'; extend: boolean }
  | { kind: 'graphemeRight'; extend: boolean }

export class EditorSemanticDispatcher {
  private inputAdapter: EditorInputAdapter
  private selectionController: SelectionController
  private coordinator: EditorSessionCoordinator
  // 统一串行队列：物理键盘/IME/鼠标/触摸都从这里排队。
  // 出队执行时才读 Coordinator snapshot（前一条命令已完成并更新 state）。
  private queue: SerialCommandQueue = new SerialCommandQueue()

  constructor(inputAdapter: EditorInputAdapter, selectionController: SelectionController, coordinator: EditorSessionCoordinator) {
    this.inputAdapter = inputAdapter
    this.selectionController = selectionController
    this.coordinator = coordinator
  }

  /**
   * 派发一条语义命令。命令在出队执行时才读 Coordinator 最新 snapshot。
   * 返回的 Promise 在命令完成时 resolve/reject，与命令结果一致。
   */
  dispatch(cmd: SemanticEditCommand): Promise<ResultEnvelope<EditorEditResult>> {
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

  // 出队后真正执行：此时 snapshot 已是最新（前一条命令已完成并更新 state）
  private async executeCommand(cmd: SemanticEditCommand): Promise<ResultEnvelope<EditorEditResult>> {
    switch (cmd.kind) {
      case 'insertText':
        return this.inputAdapter.onTextInput(cmd.text)
      case 'delete':
        return this.inputAdapter.onDelete(cmd.direction)
      case 'setSelection':
        return this.selectionController.onTap(cmd.utf16Offset)
      case 'dragSelect':
        return this.selectionController.onDragSelect(cmd.utf16Anchor, cmd.utf16Head)
      case 'selectAll':
        return this.selectionController.selectAll()
      case 'compositionBegin':
        return this.inputAdapter.onCompositionBegin()
      case 'compositionUpdate':
        return this.inputAdapter.onCompositionUpdate(cmd.preedit)
      case 'compositionFinish':
        return this.inputAdapter.onCompositionFinish(cmd.committed)
      case 'compositionCancel':
        return this.inputAdapter.onCompositionCancel()
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
