// HarmonyImeConnection — HarmonyOS IME Kit 自绘编辑框连接。
// 把 IME Kit 事件翻译成语义命令，经 EditorSemanticDispatcher 统一排队执行。
// 不把 HarmonyOS 原始 IME 对象传进 Rust Core；只翻译成 insert/delete/selection/composition 语义命令。
//
// Issue #629 评论6 Part A：统一串行边界。
// - 不再自己拥有 imeQueue（SerialCommandQueue）；所有 IME 事件经 EditorSemanticDispatcher 派发。
// - Dispatcher 持有唯一串行队列，物理键盘/IME/鼠标/触摸都从这里排队。
//
// Issue #629 评论7 第2项：HarmonyImeConnection 变纯协议翻译器。
// - 删除 composing/lastPreedit 本地状态（不再在队列外判断 composition begin/update/finish/cancel）。
// - 只派发三种语义命令：imePreviewText / imeCommitText / imeCancelPreview。
// - composition 状态判断（begin/update/finish/cancel）由 EditorSemanticDispatcher 在唯一队列出队时
//   查 inputAdapter.isComposing()（Core 驱动状态）决定，不在本类队列外判断。
// - 串行队列保证事件顺序，composition 状态判断属于同一条串行边界。
//
// Issue #629 评论7 第6项：软键盘删除统一 Core grapheme。
// - onDeleteText 派发 graphemeBackspace/graphemeDelete（不再派发 {kind:'delete',direction}）。
// - 删除边界由 Core previousGraphemeBoundary/nextGraphemeBoundary 决定，不手算 UTF-16 surrogate pair。

import { inputMethod } from '@kit.IMEKit'
import { EditorSemanticDispatcher } from './EditorSemanticDispatcher'

export class HarmonyImeConnection {
  private controller: inputMethod.InputMethodController = inputMethod.getController()
  private dispatcher: EditorSemanticDispatcher | null = null
  private attached: boolean = false

  // 编辑框获得焦点时调用：绑定语义调度器，attach 到 IME，注册事件回调。
  // 接受 dispatcher 而非 inputAdapter + selectionController：所有输入经 dispatcher 统一排队。
  async attach(dispatcher: EditorSemanticDispatcher): Promise<void> {
    if (this.attached) {
      await this.detach()
    }
    this.dispatcher = dispatcher
    await this.controller.attach(true)
    this.registerListeners()
    this.attached = true
  }

  // 失焦/页面退出时调用：取消监听并 detach。
  async detach(): Promise<void> {
    if (!this.attached) {
      return
    }
    this.unregisterListeners()
    try {
      await this.controller.detach()
    } catch (_e) {
      // detach 失败不阻塞清理
    }
    this.attached = false
    this.dispatcher = null
  }

  isAttached(): boolean {
    return this.attached
  }

  // 通知 IME 当前选区（UTF-16 code unit offset，ArkUI 事件坐标）。
  async notifySelection(utf16Start: number, utf16End: number): Promise<void> {
    if (!this.attached) {
      return
    }
    try {
      await this.controller.setSelection(utf16Start, utf16End)
    } catch (_e) {
      // IME 通知失败不影响编辑器内部状态
    }
  }

  // 通知 IME 当前光标位置（UTF-16 code unit offset）。
  async notifyCursor(utf16Offset: number): Promise<void> {
    await this.notifySelection(utf16Offset, utf16Offset)
  }

  private registerListeners(): void {
    this.controller.on('insertText', (text: string): void => { this.onInsertText(text) })
    this.controller.on('deleteText', (direction: inputMethod.DeleteDirection): void => { this.onDeleteText(direction) })
    this.controller.on('sendEnterKey', (enterKey: inputMethod.EnterKeyType): void => { this.onSendEnterKey(enterKey) })
    this.controller.on('setSelection', (selection: inputMethod.Selection): void => { this.onSetSelection(selection) })
    this.controller.on('setPreviewText', (text: string): void => { this.onSetPreviewText(text) })
    this.controller.on('unsetPreviewText', (): void => { this.onUnsetPreviewText() })
  }

  private unregisterListeners(): void {
    this.controller.off('insertText')
    this.controller.off('deleteText')
    this.controller.off('sendEnterKey')
    this.controller.off('setSelection')
    this.controller.off('setPreviewText')
    this.controller.off('unsetPreviewText')
  }

  // IME 插入文本：纯翻译器，派发 imeCommitText。
  // composition commit 还是普通插入由 EditorSemanticDispatcher 出队时查 isComposing() 决定，
  // 不在本类队列外判断。dispatcher 串行队列保证 IME 事件按到达顺序执行。
  private onInsertText(text: string): void {
    if (!this.dispatcher) {
      return
    }
    this.dispatcher.dispatchFireAndForget({ kind: 'imeCommitText', text })
  }

  // IME 删除：派发 graphemeBackspace/graphemeDelete（Issue #629 评论7 第6项）。
  // 删除边界由 Core previousGraphemeBoundary/nextGraphemeBoundary 决定，
  // 不手算 UTF-16 surrogate pair（EditorSemanticDispatcher.executeGraphemeBackspace/Delete 已实现）。
  private onDeleteText(direction: inputMethod.DeleteDirection): void {
    if (!this.dispatcher) {
      return
    }
    if (direction === inputMethod.DeleteDirection.BACKWARD) {
      this.dispatcher.dispatchFireAndForget({ kind: 'graphemeBackspace' })
    } else {
      this.dispatcher.dispatchFireAndForget({ kind: 'graphemeDelete' })
    }
  }

  // IME 回车：经 dispatcher 派发 insertText('\n')，由 EditorInputAdapter 处理（尊重当前选区）。
  private onSendEnterKey(_enterKey: inputMethod.EnterKeyType): void {
    if (!this.dispatcher) {
      return
    }
    this.dispatcher.dispatchFireAndForget({ kind: 'insertText', text: '\n' })
  }

  // IME 选区变化：翻译成 imeSetSelection 语义命令，经 dispatcher 派发。
  // Issue #629 评论8 第4项：IME 来源的 selection 不再复用普通 setSelection/dragSelect
  // （那会走 committed selection，把 Core/UI composition 状态拆开）；
  // composition 内外判断由 EditorSemanticDispatcher 在唯一队列出队时做，
  // 本连接只负责传 start/end UTF-16 显示坐标。
  private onSetSelection(selection: inputMethod.Selection): void {
    if (!this.dispatcher) {
      return
    }
    const start = selection.start
    const end = selection.end
    this.dispatcher.dispatchFireAndForget({ kind: 'imeSetSelection', utf16Start: start, utf16End: end })
  }

  // IME composition preedit：纯翻译器，派发 imePreviewText。
  // begin 还是 update 由 EditorSemanticDispatcher 出队时查 isComposing() 决定：
  //   - 未开始 composition → onCompositionBegin 成功后紧接 onCompositionUpdate
  //   - 已开始 composition → 只 onCompositionUpdate
  // 不在本类队列外判断 composing 状态。dispatcher 串行队列保证 begin→update→update→finish/cancel 顺序。
  private onSetPreviewText(text: string): void {
    if (!this.dispatcher) {
      return
    }
    this.dispatcher.dispatchFireAndForget({ kind: 'imePreviewText', text })
  }

  // IME 取消 composition preedit：纯翻译器，派发 imeCancelPreview。
  // 是否真的 cancel 由 EditorSemanticDispatcher 出队时查 isComposing() 决定（无 composition 时 no-op）。
  private onUnsetPreviewText(): void {
    if (!this.dispatcher) {
      return
    }
    this.dispatcher.dispatchFireAndForget({ kind: 'imeCancelPreview' })
  }
}
