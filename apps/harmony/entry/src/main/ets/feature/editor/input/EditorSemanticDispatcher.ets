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
//
// Issue #629 评论5324447292 第1项：统一 executeDisplaySelection(display anchor, head)。
// - display/committed 坐标分离：命中测试返回 display text（含 preedit）的 VisualCaretPosition。
// - 无 composition：走 committed onTap/onDragSelect。
// - 有 composition，collapsed target 在 preedit 内：更新 preedit cursor，成功后 owner 记忆。
// - 有 composition，target 移出 preedit 或非空 drag：同一 thunk 先 finish composition，
//   再以原 display UTF-16 坐标执行 committed selection（finish 后 committed == display，恒等映射）。
//
// Issue #629 评论5324447292 第2项：composition 活跃时 Left/Right/Backspace/Delete 路由到
// Core composition grapheme API（composition_move_grapheme_left/right,
// composition_delete_grapheme_backward/forward），不操作 committed text。
// insertText/paste/selectAll/Enter 在同一 Dispatcher thunk 显式 finish composition 成功后执行。
//
// Issue #629 评论5324447292 第3项：删除 committed Up/Down/Home/End 在 Core 成功前的提前
// rememberVisualCaret；成功出口唯一更新 owner。

import { EditorInputAdapter } from './EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { SerialCommandQueue } from '../session/editor_patch_logic'
import { TextOffsetMapper } from './TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { LineNavigationResolver, LineLayoutState } from '../selection/LineNavigationResolver'
import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { CaretAffinity, LineBreakKind } from '../render/editor_layout_math'
import type { VisualCaretPosition } from '../render/editor_layout_math'

// 语义命令类型：把物理键盘/IME/鼠标/触摸的输入统一成语义命令。
// 出队执行时才读 Coordinator snapshot，避免并发命令拿到同一个旧 cursor/selection。
// Issue #629 评论7 第2项：composition 命令改为 imePreviewText/imeCommitText/imeCancelPreview，
// 出队时查 isComposing() 决定 begin/update/finish/cancel（不在 HarmonyImeConnection 队列外判断）。
// Issue #629 评论7 第6项：删除 {kind:'delete',direction}，软键盘删除统一 graphemeBackspace/graphemeDelete。
export type SemanticEditCommand =
  | { kind: 'insertText'; text: string }
  // Issue #629 评论18：setSelection/dragSelect 接受 VisualCaretPosition（含 affinity），
  // 不丢弃命中测试的 soft-wrap 亲和性。
  | { kind: 'setSelection'; position: VisualCaretPosition }
  | { kind: 'dragSelect'; anchor: VisualCaretPosition; head: VisualCaretPosition }
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
  | { kind: 'clearDocument' }
  // Issue #629 评论11 第2项：行级导航语义命令。
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
  // Issue #629 评论16 第2项：Up/Down 导航保持横向位置。
  // 上一次 lineUp/lineDown 时的 x 坐标（px），用于在目标行找最近的 caret stop。
  // 有选区时 / lineHome/lineEnd 时重置为 -1（下次 Up/Down 回到行首/行尾）。
  private navTargetX: number = -1

  constructor(inputAdapter: EditorInputAdapter, selectionController: SelectionController, coordinator: EditorSessionCoordinator) {
    this.inputAdapter = inputAdapter
    this.selectionController = selectionController
    this.coordinator = coordinator
  }

  /**
   * 派发一条语义命令。命令在出队执行时才读 Coordinator 最新 snapshot。
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
   */
  dispatchFireAndForget(cmd: SemanticEditCommand): void {
    this.dispatch(cmd).then(() => {}, (_e: Object) => {
      // 命令失败不阻塞后续；错误已由 EditorInputAdapter/Coordinator 处理（recover）
    })
  }

  // Issue #629 R11 评论5329310563 第2项：只读查询当前 display selection（UTF-16 code unit offset）。
  // 从 coordinator.getSnapshot() 构造 EditorLayoutSnapshot.fromEditorSnapshot(snap)，
  // 把 selectionAnchor/selectionHead（byte）用 TextOffsetMapper.utf8ToUtf16 转 UTF-16 返回。
  // composition 活跃时 fromEditorSnapshot 已投影到 displayText 坐标
  // （selectionAnchor/Head === displayCaretByte），自然得到 preedit display 选区；无第二份 selection 状态。
  // 鼠标/触摸 selection 命令在 dispatch Promise resolve（命令已应用、snapshot 已更新）后调用本方法，
  // 再经 HarmonyImeConnection.syncSelectionFromEditor 通知 IME，避免拿原 hitPos 猜未来状态。
  getCurrentDisplaySelectionUtf16(): { start: number; end: number } | null {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return null
    }
    const displaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap)
    const start = TextOffsetMapper.utf8ToUtf16(displaySnapshot.text, displaySnapshot.selectionAnchor)
    const end = TextOffsetMapper.utf8ToUtf16(displaySnapshot.text, displaySnapshot.selectionHead)
    return { start, end }
  }

  // Issue #629 评论8 第3项：close 生命周期入口。
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
   */
  finishActiveComposition(): Promise<ResultEnvelope<EditorEditResult>> {
    return this.queue.enqueue<ResultEnvelope<EditorEditResult>>(async () => {
      if (!this.inputAdapter.isComposing()) {
        return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
      }
      return this.inputAdapter.finishActiveComposition()
    })
  }

  // ── 出队执行 ──

  private async executeCommand(cmd: SemanticEditCommand): Promise<ResultEnvelope<EditorEditResult>> {
    switch (cmd.kind) {
      case 'insertText':
        this.navTargetX = -1
        return this.executeFinishThenInsertText(cmd.text)
      case 'setSelection':
        this.navTargetX = -1
        return this.executeDisplaySelection(cmd.position, cmd.position)
      case 'dragSelect':
        this.navTargetX = -1
        return this.executeDisplaySelection(cmd.anchor, cmd.head)
      case 'selectAll':
        this.navTargetX = -1
        return this.executeFinishThenSelectAll()
      case 'imePreviewText':
        this.navTargetX = -1
        return this.executeImePreviewText(cmd.text)
      case 'imeCommitText':
        this.navTargetX = -1
        return this.executeImeCommitText(cmd.text)
      case 'imeCancelPreview':
        this.navTargetX = -1
        return this.executeImeCancelPreview()
      case 'imeSetSelection':
        this.navTargetX = -1
        return this.executeImeSetSelection(cmd.utf16Start, cmd.utf16End)
      case 'paste':
        this.navTargetX = -1
        return this.executeFinishThenPaste(cmd.text)
      case 'graphemeBackspace':
        this.navTargetX = -1
        return this.executeGraphemeBackspace()
      case 'graphemeDelete':
        this.navTargetX = -1
        return this.executeGraphemeDelete()
      case 'graphemeLeft':
        this.navTargetX = -1
        return this.executeGraphemeLeft(cmd.extend)
      case 'graphemeRight':
        this.navTargetX = -1
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

  // ── Issue #629 评论5324447292 第1项：统一 executeDisplaySelection ──
  // display/committed 坐标分离：命中测试返回 display text（含 preedit）的 VisualCaretPosition。
  // - 无 composition：走 committed onTap/onDragSelect
  // - 有 composition，collapsed target 在 preedit 内：更新 preedit cursor，成功后 owner 记忆
  // - 有 composition，target 移出 preedit 或非空 drag：同一 thunk 先 finish composition，
  //   再以原 display UTF-16 坐标执行 committed selection（finish 后恒等映射）
  private async executeDisplaySelection(
    anchor: VisualCaretPosition,
    head: VisualCaretPosition,
  ): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }

    if (!this.inputAdapter.isComposing() || snap.composition === null || snap.composition === undefined) {
      // 无 composition：直接走 committed selection。
      const isRange = anchor.utf16Offset !== head.utf16Offset
      if (isRange) {
        return this.selectionController.onDragSelect(anchor, head)
      }
      return this.selectionController.onTap(head)
    }

    // 有 composition。
    const comp = snap.composition
    const committedText: string = snap.text
    const compStartUtf16: number = TextOffsetMapper.utf8ToUtf16(committedText, comp.replaceByteStart)
    const preeditUtf16Len: number = comp.preeditText.length
    const compEndDisplay: number = compStartUtf16 + preeditUtf16Len

    const isRange = anchor.utf16Offset !== head.utf16Offset
    const targetOffset = head.utf16Offset
    const targetInComposition = targetOffset >= compStartUtf16 && targetOffset <= compEndDisplay

    if (!isRange && targetInComposition) {
      // collapsed target 落在 preedit 显示区内：只更新 preedit cursor，不发 Core setSelection。
      const preeditCursorUtf16: number = targetOffset - compStartUtf16
      const updateResult = await this.inputAdapter.onCompositionUpdate(comp.preeditText, preeditCursorUtf16)
      if (updateResult.success) {
        // 成功后 owner 记忆目标 visual caret position。
        this.selectionController.rememberVisualCaret(head)
      }
      return updateResult
    }

    // target 移出 preedit，或形成非空 drag selection：同一 thunk 先 finish composition。
    const finishResult = await this.inputAdapter.finishActiveComposition()
    if (!finishResult.success) {
      return finishResult
    }
    // finish 后 committed text == 之前的 display text（prefix + preedit + suffix），
    // 原 display UTF-16 坐标可直接作为 committed 坐标（恒等映射）。
    if (isRange) {
      return this.selectionController.onDragSelect(anchor, head)
    }
    return this.selectionController.onTap(head)
  }

  // ── Issue #629 评论5324447292 第2项：insertText/paste/selectAll 先 finish composition ──

  private async executeFinishThenInsertText(text: string): Promise<ResultEnvelope<EditorEditResult>> {
    if (this.inputAdapter.isComposing()) {
      const finish = await this.inputAdapter.finishActiveComposition()
      if (!finish.success) {
        return finish
      }
    }
    return this.inputAdapter.onTextInput(text)
  }

  private async executeFinishThenPaste(text: string): Promise<ResultEnvelope<EditorEditResult>> {
    if (this.inputAdapter.isComposing()) {
      const finish = await this.inputAdapter.finishActiveComposition()
      if (!finish.success) {
        return finish
      }
    }
    return this.inputAdapter.onPaste(text)
  }

  private async executeFinishThenSelectAll(): Promise<ResultEnvelope<EditorEditResult>> {
    if (this.inputAdapter.isComposing()) {
      const finish = await this.inputAdapter.finishActiveComposition()
      if (!finish.success) {
        return finish
      }
    }
    return this.selectionController.selectAll()
  }

  // ── imePreviewText ──

  private async executeImePreviewText(text: string): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.inputAdapter.isComposing()) {
      const beginResult = await this.inputAdapter.onCompositionBegin()
      if (!beginResult.success) {
        return beginResult
      }
      return this.inputAdapter.onCompositionUpdate(text)
    }
    return this.inputAdapter.onCompositionUpdate(text)
  }

  private async executeImeCommitText(text: string): Promise<ResultEnvelope<EditorEditResult>> {
    if (this.inputAdapter.isComposing()) {
      return this.inputAdapter.onCompositionFinish(text)
    }
    return this.inputAdapter.onTextInput(text)
  }

  private async executeImeCancelPreview(): Promise<ResultEnvelope<EditorEditResult>> {
    if (this.inputAdapter.isComposing()) {
      return this.inputAdapter.onCompositionCancel()
    }
    return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
  }

  // ── imeSetSelection ──

  // Issue #629 R9 评论5326213302 第3项：删除旧 composition range 规则。
  // start/end 包装成 VisualCaretPosition（IME 没给 affinity 时默认 Downstream），
  // 统一调用 executeDisplaySelection。非空 range 不再压成单光标。
  private async executeImeSetSelection(utf16Start: number, utf16End: number): Promise<ResultEnvelope<EditorEditResult>> {
    const anchor: VisualCaretPosition = { utf16Offset: utf16Start, affinity: CaretAffinity.Downstream }
    const head: VisualCaretPosition = { utf16Offset: utf16End, affinity: CaretAffinity.Downstream }
    return this.executeDisplaySelection(anchor, head)
  }

  // ── Issue #629 评论5324447292 第2项：composition grapheme 操作 ──
  // composition 活跃时 Left/Right/Backspace/Delete 路由到 Core composition grapheme API。

  private async executeGraphemeBackspace(): Promise<ResultEnvelope<EditorEditResult>> {
    // composition 活跃时走 composition 专用 API。
    if (this.inputAdapter.isComposing()) {
      return this.inputAdapter.compositionDeleteGraphemeBackward()
    }
    // 无 composition：走 committed grapheme。
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    const sel = this.inputAdapter.currentSelectionUtf8()
    if (sel.end > sel.start) {
      return this.coordinator.delete(sel.start, sel.end, EditorTransactionCause.Delete)
    }
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

  private async executeGraphemeDelete(): Promise<ResultEnvelope<EditorEditResult>> {
    // composition 活跃时走 composition 专用 API。
    if (this.inputAdapter.isComposing()) {
      return this.inputAdapter.compositionDeleteGraphemeForward()
    }
    // 无 composition：走 committed grapheme。
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

  // Issue #629 R11 评论5329310563 第1项：抽出"等待当前 snapshot 对应的 layout state"，
  // 供 executeCommittedGraphemeLeft/Right 的 extend 分支算目标 affinity 复用，
  // 避免每个分支各自重复 identity 计算 + waitForLayout。
  // resolver 或 snapshot 不可用时返回 null（调用方按默认 Downstream 兜底）。
  private async waitForCurrentLayoutState(): Promise<LineLayoutState | null> {
    const resolver = this.lineResolver
    if (!resolver) {
      return null
    }
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return null
    }
    const displaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap)
    const identity = {
      revision: displaySnapshot.revision,
      generation: displaySnapshot.generation,
      compositionGeneration: displaySnapshot.compositionGeneration,
      displayText: displaySnapshot.text,
    }
    return resolver.waitForLayout(identity)
  }

  // Issue #629 R11 评论5329310563 第1项：水平 grapheme 移动到达 targetByte 后，
  // 用 positionForHorizontalArrival 算目标 visual position 并 rememberVisualCaret。
  // layoutState===null（resolver/snap 不可用或 waitForLayout cancel）时默认 Downstream。
  private rememberVisualCaretAfterHorizontalMove(
    layoutState: LineLayoutState | null,
    direction: 'left' | 'right',
    targetByte: number,
    displayText: string,
  ): void {
    const targetUtf16 = TextOffsetMapper.utf8ToUtf16(displayText, targetByte)
    if (layoutState === null) {
      this.selectionController.rememberVisualCaret({ utf16Offset: targetUtf16, affinity: CaretAffinity.Downstream })
      return
    }
    const targetPosition = LineNavigationResolver.positionForHorizontalArrival(layoutState, direction, targetUtf16)
    this.selectionController.rememberVisualCaret(targetPosition)
  }

  // Issue #629 R11 评论5329310563 第1项：composition grapheme move 成功后用最新 snapshot 身份
  // rememberVisualCaret。composition move 只改 preedit cursor 不改 preedit text，
  // 本次 move 前拿到的行几何（layoutState）仍可用于判断目标 visual side；
  // Core 成功后再用最新 snapshot 身份 rememberVisualCaret。
  private rememberVisualCaretForCompositionMove(
    layoutState: LineLayoutState | null,
    direction: 'left' | 'right',
  ): void {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return
    }
    const displaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap)
    this.rememberVisualCaretAfterHorizontalMove(
      layoutState, direction, displaySnapshot.cursorByteOffset, displaySnapshot.text
    )
  }

  // Issue #629 R11 评论5329310563 第1项：返回 layoutState 供 handled=false 时复用，
  // 避免同一按键再等第二次布局。handled=true（纯 affinity 切换）时 layoutState=null（不需要）。
  private async trySoftWrapAffinitySwitch(
    direction: 'left' | 'right',
  ): Promise<{ handled: boolean; result: ResultEnvelope<EditorEditResult> | null; layoutState: LineLayoutState | null }> {
    const resolver = this.lineResolver
    if (!resolver) {
      return { handled: false, result: null, layoutState: null }
    }
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return { handled: false, result: null, layoutState: null }
    }
    const displaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap)
    const identity = {
      revision: displaySnapshot.revision,
      generation: displaySnapshot.generation,
      compositionGeneration: displaySnapshot.compositionGeneration,
      displayText: displaySnapshot.text,
    }
    const layoutState = await resolver.waitForLayout(identity)
    if (layoutState === null) {
      return { handled: true, result: { success: true, warnings: [], changedPaths: [], changedEntities: [] }, layoutState: null }
    }
    const cursorUtf16 = TextOffsetMapper.utf8ToUtf16(displaySnapshot.text, displaySnapshot.cursorByteOffset)
    const currentPosition: VisualCaretPosition = this.selectionController.getVisualCaret(cursorUtf16)
    let atSoftWrapEnd = false
    for (let i = 0; i < layoutState.lines.length; i++) {
      const l = layoutState.lines[i]
      if (cursorUtf16 === l.endUtf16 && l.breakKind === LineBreakKind.SoftWrap) {
        atSoftWrapEnd = true
        break
      }
    }
    if (!atSoftWrapEnd) {
      return { handled: false, result: null, layoutState }
    }
    if (direction === 'left' && currentPosition.affinity === CaretAffinity.Downstream) {
      this.selectionController.rememberVisualCaret({ utf16Offset: cursorUtf16, affinity: CaretAffinity.Upstream })
      return { handled: true, result: { success: true, warnings: [], changedPaths: [], changedEntities: [] }, layoutState: null }
    }
    if (direction === 'right' && currentPosition.affinity === CaretAffinity.Upstream) {
      this.selectionController.rememberVisualCaret({ utf16Offset: cursorUtf16, affinity: CaretAffinity.Downstream })
      return { handled: true, result: { success: true, warnings: [], changedPaths: [], changedEntities: [] }, layoutState: null }
    }
    return { handled: false, result: null, layoutState }
  }

  // ── Issue #629 R9 评论5326213302 第1+2项：
  // 1. Shift+Left/Right composition 下先 finish 再走 committed helper（保留 extend=true）。
  // 2. trySoftWrapAffinitySwitch 必须先于 composition 路由。
  // 3. preedit 最左/最右继续同方向时 finish 后跨到 committed 正文。

  /** 无 composition 时 Left 语义：collapse selection → affinity switch → grapheme move。 */
  // Issue #629 R11 评论5329310563 第1项：Core setSelection 成功后用 positionForHorizontalArrival
  // 算目标 visual position 并 rememberVisualCaret，保留"这次移动刚好到达 soft-wrap 边界"的 affinity。
  // extend=true 同样补：selection head 到达 soft-wrap 边界时 affinity 不能丢。
  private async executeCommittedGraphemeLeft(extend: boolean): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    const displaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap)
    const displayText = displaySnapshot.text
    const cursorByte = snap.cursor
    const anchorByte = snap.selectionAnchor
    if (extend) {
      const headByte = snap.cursor
      if (headByte <= 0) {
        const setResult = await this.coordinator.setSelection(anchorByte, 0)
        if (setResult.success) {
          // 0 永远不是 SoftWrap 行尾，Downstream。
          this.selectionController.rememberVisualCaret({ utf16Offset: 0, affinity: CaretAffinity.Downstream })
        }
        return setResult
      }
      // 拿 layoutState 用于算目标 affinity（extend 分支无 trySoftWrap，独立拿一次）。
      const layoutState = await this.waitForCurrentLayoutState()
      const prevResult = await this.coordinator.previousGraphemeBoundary(headByte)
      if (!prevResult.success || prevResult.data === undefined || prevResult.data === null) {
        return prevResult as ResultEnvelope<EditorEditResult>
      }
      const setResult = await this.coordinator.setSelection(anchorByte, prevResult.data)
      if (setResult.success) {
        this.rememberVisualCaretAfterHorizontalMove(layoutState, 'left', prevResult.data, displayText)
      }
      return setResult
    }
    if (anchorByte !== cursorByte) {
      const selStart = Math.min(anchorByte, cursorByte)
      return this.coordinator.setSelection(selStart, selStart)
    }
    const leftAffinitySwitch = await this.trySoftWrapAffinitySwitch('left')
    if (leftAffinitySwitch.handled) {
      return leftAffinitySwitch.result!
    }
    // 复用 trySoftWrap 已拿到的 layoutState，不再等第二次布局。
    const layoutState = leftAffinitySwitch.layoutState
    if (cursorByte <= 0) {
      const setResult = await this.coordinator.setSelection(0, 0)
      if (setResult.success) {
        this.selectionController.rememberVisualCaret({ utf16Offset: 0, affinity: CaretAffinity.Downstream })
      }
      return setResult
    }
    const prevResult = await this.coordinator.previousGraphemeBoundary(cursorByte)
    if (!prevResult.success || prevResult.data === undefined || prevResult.data === null) {
      return prevResult as ResultEnvelope<EditorEditResult>
    }
    const setResult = await this.coordinator.setSelection(prevResult.data, prevResult.data)
    if (setResult.success) {
      this.rememberVisualCaretAfterHorizontalMove(layoutState, 'left', prevResult.data, displayText)
    }
    return setResult
  }

  /** 无 composition 时 Right 语义：collapse selection → affinity switch → grapheme move。 */
  // Issue #629 R11 评论5329310563 第1项：Core setSelection 成功后用 positionForHorizontalArrival
  // 算目标 visual position 并 rememberVisualCaret。extend=true 同样补。
  private async executeCommittedGraphemeRight(extend: boolean): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    const displaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap)
    const displayText = displaySnapshot.text
    const textByteLen = TextOffsetMapper.utf8ToUtf8(displayText, displayText.length)
    const cursorByte = snap.cursor
    const anchorByte = snap.selectionAnchor
    if (extend) {
      const headByte = snap.cursor
      if (headByte >= textByteLen) {
        const setResult = await this.coordinator.setSelection(anchorByte, textByteLen)
        if (setResult.success) {
          // 文本末尾不是 SoftWrap 行尾（EndOfText），Downstream。
          const endUtf16 = TextOffsetMapper.utf8ToUtf16(displayText, textByteLen)
          this.selectionController.rememberVisualCaret({ utf16Offset: endUtf16, affinity: CaretAffinity.Downstream })
        }
        return setResult
      }
      const layoutState = await this.waitForCurrentLayoutState()
      const nextResult = await this.coordinator.nextGraphemeBoundary(headByte)
      if (!nextResult.success || nextResult.data === undefined || nextResult.data === null) {
        return nextResult as ResultEnvelope<EditorEditResult>
      }
      const setResult = await this.coordinator.setSelection(anchorByte, nextResult.data)
      if (setResult.success) {
        this.rememberVisualCaretAfterHorizontalMove(layoutState, 'right', nextResult.data, displayText)
      }
      return setResult
    }
    if (anchorByte !== cursorByte) {
      const selEnd = Math.max(anchorByte, cursorByte)
      return this.coordinator.setSelection(selEnd, selEnd)
    }
    const rightAffinitySwitch = await this.trySoftWrapAffinitySwitch('right')
    if (rightAffinitySwitch.handled) {
      return rightAffinitySwitch.result!
    }
    const layoutState = rightAffinitySwitch.layoutState
    if (cursorByte >= textByteLen) {
      const setResult = await this.coordinator.setSelection(textByteLen, textByteLen)
      if (setResult.success) {
        const endUtf16 = TextOffsetMapper.utf8ToUtf16(displayText, textByteLen)
        this.selectionController.rememberVisualCaret({ utf16Offset: endUtf16, affinity: CaretAffinity.Downstream })
      }
      return setResult
    }
    const nextResult = await this.coordinator.nextGraphemeBoundary(cursorByte)
    if (!nextResult.success || nextResult.data === undefined || nextResult.data === null) {
      return nextResult as ResultEnvelope<EditorEditResult>
    }
    const setResult = await this.coordinator.setSelection(nextResult.data, nextResult.data)
    if (setResult.success) {
      this.rememberVisualCaretAfterHorizontalMove(layoutState, 'right', nextResult.data, displayText)
    }
    return setResult
  }

  private async executeGraphemeLeft(extend: boolean): Promise<ResultEnvelope<EditorEditResult>> {
    // Issue #629 R10 评论5327548809 第2项：只有 composition 活跃时才在进入 composition grapheme API 前做一次 affinity switch。
    // 无 composition 时直接交给 committed helper（其内会按当前 identity 检查一次），避免普通 Left 重复检查两次。
    // Issue #629 R11 评论5329310563 第1项：composition move 成功后用最新 snapshot 身份 rememberVisualCaret，
    // 复用 trySoftWrap 已拿到的 layoutState（composition move 不改 preedit text，行几何不变）。
    const composing = this.inputAdapter.isComposing()
    let compositionLayoutState: LineLayoutState | null = null
    if (composing && !extend) {
      const affinitySwitch = await this.trySoftWrapAffinitySwitch('left')
      if (affinitySwitch.handled) {
        return affinitySwitch.result!
      }
      compositionLayoutState = affinitySwitch.layoutState
    }

    if (!composing) {
      // 无 composition：走 committed grapheme。
      return this.executeCommittedGraphemeLeft(extend)
    }

    // composition 活跃。
    if (extend) {
      // Shift+Left：composition 没有 selection range，不能退化成移动 preedit cursor。
      // 先 finish composition 成功后，再走 committed helper 保留 extend=true。
      const finishResult = await this.inputAdapter.finishActiveComposition()
      if (!finishResult.success) {
        return finishResult
      }
      return this.executeCommittedGraphemeLeft(true)
    }

    // 无 extend：先走 composition grapheme move。
    // 如果 preedit cursor 已在最左端（compositionMoveGraphemeLeft 返回 no-op），
    // 则 finish composition 后进入 committed Left。
    const beforeCursor = this.coordinator.getSnapshot()?.composition?.preeditCursorUtf16 ?? 0
    const moveResult = await this.inputAdapter.compositionMoveGraphemeLeft()
    if (!moveResult.success) {
      return moveResult
    }
    const afterCursor = this.coordinator.getSnapshot()?.composition?.preeditCursorUtf16 ?? 0
    if (beforeCursor === afterCursor && beforeCursor <= 0) {
      // preedit 最左端，继续 Left → finish 后进入 committed 正文
      const finishResult = await this.inputAdapter.finishActiveComposition()
      if (!finishResult.success) {
        return finishResult
      }
      return this.executeCommittedGraphemeLeft(false)
    }
    // 纯 composition move 成功：用最新 snapshot 身份 rememberVisualCaret。
    // composition move 只改 preedit cursor 不改 preedit text，本次 move 前拿到的行几何仍可用于判断目标 visual side。
    this.rememberVisualCaretForCompositionMove(compositionLayoutState, 'left')
    return moveResult
  }

  private async executeGraphemeRight(extend: boolean): Promise<ResultEnvelope<EditorEditResult>> {
    // Issue #629 R10 评论5327548809 第2项：只有 composition 活跃时才在进入 composition grapheme API 前做一次 affinity switch。
    // Issue #629 R11 评论5329310563 第1项：composition move 成功后用最新 snapshot 身份 rememberVisualCaret。
    const composing = this.inputAdapter.isComposing()
    let compositionLayoutState: LineLayoutState | null = null
    if (composing && !extend) {
      const affinitySwitch = await this.trySoftWrapAffinitySwitch('right')
      if (affinitySwitch.handled) {
        return affinitySwitch.result!
      }
      compositionLayoutState = affinitySwitch.layoutState
    }

    if (!composing) {
      return this.executeCommittedGraphemeRight(extend)
    }

    // composition 活跃。
    if (extend) {
      // Shift+Right：先 finish composition，再走 committed helper 保留 extend=true。
      const finishResult = await this.inputAdapter.finishActiveComposition()
      if (!finishResult.success) {
        return finishResult
      }
      return this.executeCommittedGraphemeRight(true)
    }

    // 无 extend：先走 composition grapheme move。
    const snap = this.coordinator.getSnapshot()
    const preeditText = snap?.composition?.preeditText ?? ''
    const beforeCursor = snap?.composition?.preeditCursorUtf16 ?? 0
    const moveResult = await this.inputAdapter.compositionMoveGraphemeRight()
    if (!moveResult.success) {
      return moveResult
    }
    const afterCursor = this.coordinator.getSnapshot()?.composition?.preeditCursorUtf16 ?? 0
    if (beforeCursor === afterCursor && beforeCursor >= preeditText.length) {
      // preedit 最右端，继续 Right → finish 后进入 committed 正文
      const finishResult = await this.inputAdapter.finishActiveComposition()
      if (!finishResult.success) {
        return finishResult
      }
      return this.executeCommittedGraphemeRight(false)
    }
    // 纯 composition move 成功：用最新 snapshot 身份 rememberVisualCaret。
    this.rememberVisualCaretForCompositionMove(compositionLayoutState, 'right')
    return moveResult
  }

  // ── clearDocument ──

  private async executeClearDocument(): Promise<ResultEnvelope<EditorEditResult>> {
    this.navTargetX = -1
    let snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    if (this.inputAdapter.isComposing()) {
      const cancelResult = await this.inputAdapter.onCompositionCancel()
      if (!cancelResult.success) {
        return cancelResult
      }
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
    const afterClear = this.coordinator.getSnapshot()
    if (afterClear === null || afterClear.text !== '') {
      return { success: false, errorCode: 'CLEAR_NOT_EMPTY', warnings: [], changedPaths: [], changedEntities: [] }
    }
    return replaceResult
  }

  // ── 行级导航 ──

  private async executeLineNavigation(direction: 'up' | 'down' | 'home' | 'end', extend: boolean): Promise<ResultEnvelope<EditorEditResult>> {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return this.noSession()
    }
    const resolver = this.lineResolver
    if (!resolver) {
      return this.noSession()
    }
    const displaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(snap)
    const identity = {
      revision: displaySnapshot.revision,
      generation: displaySnapshot.generation,
      compositionGeneration: displaySnapshot.compositionGeneration,
      displayText: displaySnapshot.text
    }
    const layoutState = await resolver.waitForLayout(identity)
    if (layoutState === null) {
      return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
    }
    const latestSnap = this.coordinator.getSnapshot()
    if (!latestSnap) {
      return this.noSession()
    }
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
      const refreshedState = await resolver.waitForLayout(latestIdentity)
      if (refreshedState === null) {
        return { success: true, warnings: [], changedPaths: [], changedEntities: [] }
      }
      finalState = refreshedState
      const finalSnap = this.coordinator.getSnapshot()
      if (!finalSnap) {
        return this.noSession()
      }
      const finalDisplaySnapshot = EditorLayoutSnapshot.fromEditorSnapshot(finalSnap)
      const finalCursorUtf16: number = TextOffsetMapper.utf8ToUtf16(finalDisplaySnapshot.text, finalDisplaySnapshot.cursorByteOffset)
      const finalAnchorUtf16: number = TextOffsetMapper.utf8ToUtf16(finalDisplaySnapshot.text, finalDisplaySnapshot.selectionAnchor)
      const currentPosition: VisualCaretPosition = this.selectionController.getVisualCaret(finalCursorUtf16)
      if (finalSnap.composition !== null && finalSnap.composition !== undefined) {
        return this.executeImeLineNavigation(direction, extend, finalState, currentPosition, finalAnchorUtf16, finalDisplaySnapshot)
      }
      return this.executeCommittedLineNavigation(direction, extend, finalState, currentPosition, finalAnchorUtf16)
    }
    const latestCursorUtf16: number = TextOffsetMapper.utf8ToUtf16(latestDisplaySnapshot.text, latestDisplaySnapshot.cursorByteOffset)
    const latestAnchorUtf16: number = TextOffsetMapper.utf8ToUtf16(latestDisplaySnapshot.text, latestDisplaySnapshot.selectionAnchor)
    const currentPosition: VisualCaretPosition = this.selectionController.getVisualCaret(latestCursorUtf16)
    if (latestSnap.composition !== null && latestSnap.composition !== undefined) {
      return this.executeImeLineNavigation(direction, extend, finalState, currentPosition, latestAnchorUtf16, latestDisplaySnapshot)
    }
    return this.executeCommittedLineNavigation(direction, extend, finalState, currentPosition, latestAnchorUtf16)
  }

  // Issue #629 评论5324447292 第3项：删除 committed Up/Down/Home/End 在 Core 成功前的提前
  // rememberVisualCaret；成功出口唯一更新 owner。
  private executeCommittedLineNavigation(
    direction: 'up' | 'down' | 'home' | 'end',
    extend: boolean,
    state: LineLayoutState,
    currentPosition: VisualCaretPosition,
    anchorUtf16: number
  ): Promise<ResultEnvelope<EditorEditResult>> {
    switch (direction) {
      case 'up': {
        const targetLineIdx = LineNavigationResolver.getPreviousLineIndex(state, currentPosition)
        if (targetLineIdx < 0) {
          return Promise.resolve({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
        }
        if (this.navTargetX < 0) {
          this.navTargetX = LineNavigationResolver.getCaretX(state, currentPosition)
        }
        const targetOffset = LineNavigationResolver.getNearestOffsetAtX(state, targetLineIdx, this.navTargetX)
        const targetPosition = LineNavigationResolver.positionForOffsetInLine(state, targetLineIdx, targetOffset)
        // 不提前 rememberVisualCaret；onTap/onDragSelect 成功后 publishVisualCaret 唯一更新 owner。
        if (extend) {
          return this.selectionController.onDragSelect(anchorUtf16, targetPosition)
        }
        return this.selectionController.onTap(targetPosition)
      }
      case 'down': {
        const targetLineIdx = LineNavigationResolver.getNextLineIndex(state, currentPosition)
        if (targetLineIdx < 0) {
          return Promise.resolve({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
        }
        if (this.navTargetX < 0) {
          this.navTargetX = LineNavigationResolver.getCaretX(state, currentPosition)
        }
        const targetOffset = LineNavigationResolver.getNearestOffsetAtX(state, targetLineIdx, this.navTargetX)
        const targetPosition = LineNavigationResolver.positionForOffsetInLine(state, targetLineIdx, targetOffset)
        if (extend) {
          return this.selectionController.onDragSelect(anchorUtf16, targetPosition)
        }
        return this.selectionController.onTap(targetPosition)
      }
      case 'home': {
        this.navTargetX = -1
        const lineIdx = LineNavigationResolver.getCurrentLineIndex(state, currentPosition)
        const targetOffset = lineIdx >= 0 ? LineNavigationResolver.getLineStart(state, lineIdx) : 0
        const targetPosition = LineNavigationResolver.positionForOffsetInLine(state, lineIdx, targetOffset)
        if (extend) {
          return this.selectionController.onDragSelect(anchorUtf16, targetPosition)
        }
        return this.selectionController.onTap(targetPosition)
      }
      case 'end': {
        this.navTargetX = -1
        const lineIdx = LineNavigationResolver.getCurrentLineIndex(state, currentPosition)
        const textUtf16Len = state.displayText.length
        const targetOffset = lineIdx >= 0 ? LineNavigationResolver.getLineEnd(state, lineIdx) : textUtf16Len
        const targetPosition = LineNavigationResolver.positionForOffsetInLine(state, lineIdx, targetOffset)
        if (extend) {
          return this.selectionController.onDragSelect(anchorUtf16, targetPosition)
        }
        return this.selectionController.onTap(targetPosition)
      }
    }
  }

  // Issue #629 评论5324447292 第1+3项：composition 活跃时行导航。
  // 行内移动：复用 executeDisplaySelection 统一入口（preedit 内更新 preedit cursor；
  // 移出 composition 区域：先 finish）。Up/Down 同样保持横向位置。
  // 不提前 rememberVisualCaret；成功出口统一更新 owner。
  private executeImeLineNavigation(
    direction: 'up' | 'down' | 'home' | 'end',
    extend: boolean,
    state: LineLayoutState,
    currentPosition: VisualCaretPosition,
    anchorUtf16: number,
    displaySnapshot: EditorLayoutSnapshot
  ): Promise<ResultEnvelope<EditorEditResult>> {
    let targetOffset: number = currentPosition.utf16Offset
    let targetLineIdx: number = -1
    switch (direction) {
      case 'up': {
        targetLineIdx = LineNavigationResolver.getPreviousLineIndex(state, currentPosition)
        if (targetLineIdx < 0) {
          return Promise.resolve({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
        }
        if (this.navTargetX < 0) {
          this.navTargetX = LineNavigationResolver.getCaretX(state, currentPosition)
        }
        targetOffset = LineNavigationResolver.getNearestOffsetAtX(state, targetLineIdx, this.navTargetX)
        break
      }
      case 'down': {
        targetLineIdx = LineNavigationResolver.getNextLineIndex(state, currentPosition)
        if (targetLineIdx < 0) {
          return Promise.resolve({ success: true, warnings: [], changedPaths: [], changedEntities: [] })
        }
        if (this.navTargetX < 0) {
          this.navTargetX = LineNavigationResolver.getCaretX(state, currentPosition)
        }
        targetOffset = LineNavigationResolver.getNearestOffsetAtX(state, targetLineIdx, this.navTargetX)
        break
      }
      case 'home': {
        this.navTargetX = -1
        targetLineIdx = LineNavigationResolver.getCurrentLineIndex(state, currentPosition)
        targetOffset = targetLineIdx >= 0 ? LineNavigationResolver.getLineStart(state, targetLineIdx) : 0
        break
      }
      case 'end': {
        this.navTargetX = -1
        targetLineIdx = LineNavigationResolver.getCurrentLineIndex(state, currentPosition)
        targetOffset = targetLineIdx >= 0 ? LineNavigationResolver.getLineEnd(state, targetLineIdx) : displaySnapshot.text.length
        break
      }
    }
    const targetPosition = targetLineIdx >= 0
      ? LineNavigationResolver.positionForOffsetInLine(state, targetLineIdx, targetOffset)
      : { utf16Offset: targetOffset, affinity: CaretAffinity.Downstream }
    // 评论5324447292 第3项：不提前 rememberVisualCaret；复用 executeDisplaySelection 统一入口，
    // 成功出口统一更新 owner（preedit 内 → onCompositionUpdate 成功后 rememberVisualCaret；
    // 移出 preedit → finish 后 onTap(head)，保留 targetPosition 的 affinity，不用裸 number）。
    if (!extend || anchorUtf16 === targetOffset) {
      return this.executeDisplaySelection(targetPosition, targetPosition)
    }
    // Shift+Up/Down/Home/End：先 finish composition，再普通 selection。
    const finishResult = await this.inputAdapter.finishActiveComposition()
    if (!finishResult.success) {
      return finishResult
    }
    return this.selectionController.onDragSelect(anchorUtf16, targetPosition)
  }

  // ── 注册/注销 ──

  registerLineResolver(resolver: LineNavigationResolver): void {
    this.lineResolver = resolver
  }

  unregisterLineResolver(): void {
    this.lineResolver = null
  }

  async flush(): Promise<void> {
    return this.queue.whenIdle()
  }

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
