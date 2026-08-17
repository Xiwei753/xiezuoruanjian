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
import { LineNavigationResolver, LineLayoutState } from '../selection/LineNavigationResolver'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'

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
  // Issue #629 评论11 第1项：清空整篇文档的语义命令。
  // 出队时读取最新 snapshot 计算 UTF-8 全文范围，调 coordinator.replace 清空。
  // 保证之前的输入先执行，之后的新输入排在 clear 后面，不再有跨队列抢跑。
  | { kind: 'clearDocument' }
  // Issue #629 评论11 第2项：行级导航语义命令（不提前计算目标位置）。
  // 出队时基于最新 Core snapshot + 最新行布局计算目标，再调 SelectionController。
  // extend 为 true 时保持 anchor 不变（Shift+方向键扩展选区）。
  | { kind: 'lineUp'; extend: boolean }
  | { kind: 'lineDown'; extend: boolean }
  | { kind: 'lineHome'; extend: boolean }
  | { kind: 'lineEnd'; extend: boolean }

export class EditorSemanticDispatcher {
  private inputAdapter: EditorInputAdapter
  private selectionController: SelectionController
  private coordinator: EditorSessionCoordinator
  // 统一串行队列：物理键盘/IME/鼠标/触摸都从这里排队。
  // 出队执行时才读 Coordinator snapshot（前一条命令已完成并更新 state）。
  private queue: SerialCommandQueue = new SerialCommandQueue()
  // Issue #629 评论8 第3项：close 开始后 sealed，拒绝新的普通输入命令（已排队命令仍可 drain）。
  private sealed: boolean = false
  // Issue #629 评论11 第2项：行级导航解析器。由 SujianEditor 在 aboutToAppear 注册，
  // 行导航命令出队时基于最新 renderResult.lines 计算目标位置。
  private lineResolver: LineNavigationResolver | null = null

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
      case 'clearDocument':
        return this.executeClearDocument()
      case 'lineUp':
        return this.executeLineNavigation('up', cmd.extend)
      case 'lineDown':
        return this.executeLineNavigation('down', cmd.extend)
      case 'lineHome':
        return this.executeLineNavigation('home', cmd.extend)
      case 'lineEnd':
        return this.executeLineNavigation('end', cmd.extend)
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

  // Issue #629 评论11 第1项：清空整篇文档。
  // 整个 clear thunk 自包含：读 snapshot → cancel 活跃 composition → replace 全文为空
  // → 重新读 snapshot 确认空正文。下一条 Dispatcher 命令只能在这个 thunk 返回后执行，
  // 之前排队输入一定先执行，之后新输入只能排在 clear 后面，不再有跨队列抢跑。
  // Issue #629 评论11 第1项补充：活跃 composition 先显式 cancel（不依赖 replace 顺手清除），
  // cancel 失败则不继续清空；replace 后 Core snapshot 非空则返回失败。
  private async executeClearDocument(): Promise<ResultEnvelope<EditorEditResult>> {
    let snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    // 有活跃 composition 时先 cancel：用户选择清空时不应保留 preedit。
    if (this.inputAdapter.isComposing()) {
      const cancelResult = await this.inputAdapter.onCompositionCancel()
      if (!cancelResult.success) {
        return cancelResult
      }
      // cancel 后重新读 snapshot（composition 已清除，正文可能已变化）
      snap = this.coordinator.getSnapshot()
      if (!snap) {
        return this.noSession()
      }
    }
    const fullText: string = snap.text
    const utf8EndByte: number = TextOffsetMapper.utf16ToUtf8(fullText, fullText.length)
    const replaceResult = await this.coordinator.replace(
      0, utf8EndByte, '', fullText, EditorTransactionCause.Programmatic
    )
    if (!replaceResult.success) {
      return replaceResult
    }
    // replace 成功后重新读 Core snapshot 确认正文为空。
    // StaleRevision/Invalid* 或最终 snapshot 非空都视为清空失败。
    const afterClear = this.coordinator.getSnapshot()
    if (afterClear === null || afterClear.text !== '') {
      return { success: false, errorCode: 'CLEAR_NOT_EMPTY', warnings: [], changedPaths: [], changedEntities: [] }
    }
    return replaceResult
  }

  // Issue #629 评论14 第3项 + 评论15 第5项：行级导航（up/down/home/end）。
  // 出队后基于最新 Core snapshot + 等待匹配编辑状态身份的行布局，计算目标位置。
  // 不再返回 STALE_LAYOUT 丢键，也不设 timeout 兜底。
  // 等待只能有两个出口：SujianEditor 发布匹配的布局，或 cancelWait（组件销毁/换 session）。
  // composition 活跃时，行导航目标坐标是显示文本 UTF-16 坐标，复用 imeSetSelection 语义。
  private async executeLineNavigation(direction: 'up' | 'down' | 'home' | 'end', extend: boolean): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    const resolver = this.lineResolver
    if (!resolver) {
      return this.noSession()
    }
    // Issue #629 评论15 第5项：构造编辑状态身份（不包含 contentWidth/fontSize）。
    // EditorLayoutIdentity = { revision, generation, compositionGeneration, displayText }。
    const displaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap)
    const identity = {
      revision: displaySnapshot.revision,
      generation: displaySnapshot.generation,
      compositionGeneration: displaySnapshot.compositionGeneration,
      displayText: displaySnapshot.text
    }
    const displayText: string = displaySnapshot.text
    // Issue #629 评论15 第5项：等待匹配身份的 layout state，不设 timeout。
    const layoutState = await resolver.waitForLayout(identity)
    // layoutState === null 表示被取消（组件销毁/换 session），直接结束。
    if (layoutState === null) {
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    }
    // 等待后重新读 snapshot（布局更新期间 Core 可能已前进）。
    const latestSnap = this.coordinator.getSnapshot()
    if (!latestSnap) {
      return this.noSession()
    }
    // Issue #629 评论15 第5项：用最新 snapshot 构造坐标，用等待到的 layoutState 计算目标。
    // 如果 Core 又前进（identity 变了），重新以最新 identity 等待，不把旧 layout 和新 cursor 混用。
    const latestDisplaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(latestSnap)
    const latestIdentity = {
      revision: latestDisplaySnapshot.revision,
      generation: latestDisplaySnapshot.generation,
      compositionGeneration: latestDisplaySnapshot.compositionGeneration,
      displayText: latestDisplaySnapshot.text
    }
    let finalState: LineLayoutState = layoutState
    if (latestIdentity.revision !== identity.revision
      || latestIdentity.generation !== identity.generation
      || latestIdentity.compositionGeneration !== identity.compositionGeneration
      || latestIdentity.displayText !== identity.displayText) {
      // Core 前进了，重新等待最新 identity 的 layout。
      const refreshedState = await resolver.waitForLayout(latestIdentity)
      if (refreshedState === null) {
        return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
      }
      finalState = refreshedState
      // 重新读最新 snapshot 计算坐标。
      const finalSnap = this.coordinator.getSnapshot()
      if (!finalSnap) {
        return this.noSession()
      }
      const finalDisplaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(finalSnap)
      const finalCursorUtf16: number = TextOffsetMapper.utf8ToUtf16(finalDisplaySnapshot.text, finalDisplaySnapshot.cursorByteOffset)
      const finalAnchorUtf16: number = TextOffsetMapper.utf8ToUtf16(finalDisplaySnapshot.text, finalDisplaySnapshot.selectionAnchor)
      // composition 活跃时走 imeSetSelection 语义。
      if (finalSnap.composition !== null && finalSnap.composition !== undefined) {
        return this.executeImeLineNavigation(direction, extend, finalState, finalCursorUtf16, finalAnchorUtf16, finalDisplaySnapshot)
      }
      return this.executeCommittedLineNavigation(direction, extend, finalState, finalCursorUtf16, finalAnchorUtf16)
    }
    // Core 未前进，用等待到的 finalState + 当前 snapshot 坐标。
    const latestCursorUtf16: number = TextOffsetMapper.utf8ToUtf16(latestDisplaySnapshot.text, latestDisplaySnapshot.cursorByteOffset)
    const latestAnchorUtf16: number = TextOffsetMapper.utf8ToUtf16(latestDisplaySnapshot.text, latestDisplaySnapshot.selectionAnchor)
    // composition 活跃时走 imeSetSelection 语义。
    if (latestSnap.composition !== null && latestSnap.composition !== undefined) {
      return this.executeImeLineNavigation(direction, extend, finalState, latestCursorUtf16, latestAnchorUtf16, latestDisplaySnapshot)
    }
    return this.executeCommittedLineNavigation(direction, extend, finalState, latestCursorUtf16, latestAnchorUtf16)
  }

  // Issue #629 评论15 第5项：committed text 上的行导航（无 composition）。
  private executeCommittedLineNavigation(
    direction: 'up' | 'down' | 'home' | 'end',
    extend: boolean,
    state: LineLayoutState,
    cursorUtf16: number,
    anchorUtf16: number
  ): Promise<ResultEnvelope<EditorEditResult>> {
    switch (direction) {
      case 'up': {
        const targetLineIdx = LineNavigationResolver.getPreviousLineIndex(state, cursorUtf16)
        if (targetLineIdx < 0) {
          return Promise.resolve({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
        }
        const targetOffset = LineNavigationResolver.getLineStart(state, targetLineIdx)
        if (extend) {
          return this.selectionController.onDragSelect(anchorUtf16, targetOffset)
        }
        return this.selectionController.onTap(targetOffset)
      }
      case 'down': {
        const targetLineIdx = LineNavigationResolver.getNextLineIndex(state, cursorUtf16)
        if (targetLineIdx < 0) {
          return Promise.resolve({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
        }
        const targetOffset = LineNavigationResolver.getLineStart(state, targetLineIdx)
        if (extend) {
          return this.selectionController.onDragSelect(anchorUtf16, targetOffset)
        }
        return this.selectionController.onTap(targetOffset)
      }
      case 'home': {
        const lineIdx = LineNavigationResolver.getCurrentLineIndex(state, cursorUtf16)
        const targetOffset = lineIdx >= 0 ? LineNavigationResolver.getLineStart(state, lineIdx) : 0
        if (extend) {
          return this.selectionController.onDragSelect(anchorUtf16, targetOffset)
        }
        return this.selectionController.onTap(targetOffset)
      }
      case 'end': {
        const lineIdx = LineNavigationResolver.getCurrentLineIndex(state, cursorUtf16)
        const textUtf16Len = state.displayText.length
        const targetOffset = lineIdx >= 0 ? LineNavigationResolver.getLineEnd(state, lineIdx) : textUtf16Len
        if (extend) {
          return this.selectionController.onDragSelect(anchorUtf16, targetOffset)
        }
        return this.selectionController.onTap(targetOffset)
      }
    }
  }

  // Issue #629 评论15 第5项：composition 活跃时的行导航。
  // 行内移动：走 imeSetSelection 语义（更新 preedit cursor）。
  // 移出 composition 区域：先 finish composition，再走 imeSetSelection。
  private executeImeLineNavigation(
    direction: 'up' | 'down' | 'home' | 'end',
    extend: boolean,
    state: LineLayoutState,
    cursorUtf16: number,
    anchorUtf16: number,
    displaySnapshot: EditorLayoutSnapshot
  ): Promise<ResultEnvelope<EditorEditResult>> {
    // 计算目标行的 UTF-16 offset（基于 displayText 的坐标）。
    let targetOffset: number = cursorUtf16
    switch (direction) {
      case 'up': {
        const targetLineIdx = LineNavigationResolver.getPreviousLineIndex(state, cursorUtf16)
        if (targetLineIdx < 0) {
          return Promise.resolve({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
        }
        targetOffset = LineNavigationResolver.getLineStart(state, targetLineIdx)
        break
      }
      case 'down': {
        const targetLineIdx = LineNavigationResolver.getNextLineIndex(state, cursorUtf16)
        if (targetLineIdx < 0) {
          return Promise.resolve({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
        }
        targetOffset = LineNavigationResolver.getLineStart(state, targetLineIdx)
        break
      }
      case 'home': {
        const lineIdx = LineNavigationResolver.getCurrentLineIndex(state, cursorUtf16)
        targetOffset = lineIdx >= 0 ? LineNavigationResolver.getLineStart(state, lineIdx) : 0
        break
      }
      case 'end': {
        const lineIdx = LineNavigationResolver.getCurrentLineIndex(state, cursorUtf16)
        targetOffset = lineIdx >= 0 ? LineNavigationResolver.getLineEnd(state, lineIdx) : displaySnapshot.text.length
        break
      }
    }
    if (extend) {
      return this.executeImeSetSelection(anchorUtf16, targetOffset)
    }
    return this.executeImeSetSelection(targetOffset, targetOffset)
  }

  // Issue #629 评论11 第2项：注册行级导航解析器。
  // SujianEditor 在 aboutToAppear 时注册，提供最新 renderResult.lines 读取入口。
  // 行导航命令出队时通过 resolver 计算目标位置（不在入队前读旧布局）。
  registerLineResolver(resolver: LineNavigationResolver): void {
    this.lineResolver = resolver
  }

  unregisterLineResolver(): void {
    this.lineResolver = null
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
