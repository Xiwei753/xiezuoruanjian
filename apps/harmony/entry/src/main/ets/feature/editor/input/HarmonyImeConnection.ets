// HarmonyImeConnection — HarmonyOS IME Kit 自绘编辑框连接。
// 把 IME Kit 事件翻译成语义命令，经 EditorSemanticDispatcher 统一排队执行。
// 不把 HarmonyOS 原始 IME 对象传进 Rust Core；只翻译成 insert/delete/selection/composition 语义命令。
// composition 状态机：setPreviewText 触发 begin/update，insertText 在 composing 时触发 finish（commit），
// unsetPreviewText 触发 cancel。onCompositionFinish(committed) 不忽略 committed 参数。
//
// Issue #629 评论6 Part A：统一串行边界。
// - 不再自己拥有 imeQueue（SerialCommandQueue）；所有 IME 事件经 EditorSemanticDispatcher 派发。
// - Dispatcher 持有唯一串行队列，物理键盘/IME/鼠标/触摸都从这里排队。
// - HarmonyImeConnection 只保留 IME 协议需要的 composing/lastPreedit 状态（判断 insertText 是
//   composition commit 还是普通插入、判断 setPreviewText 是 begin 还是 update），不再有第二套顺序状态。
// - onSetPreviewText 第一次必须 await dispatch(compositionBegin) 成功后才 update；
//   begin 失败时不设 composing、不 update（不让 update 读到 sessionId=0/generation=0）。
//   串行队列保证 begin→update→finish/cancel 顺序，不放任多个 Core 请求并发。

import { inputMethod } from '@kit.IMEKit'
import { EditorSemanticDispatcher } from './EditorSemanticDispatcher'

export class HarmonyImeConnection {
  private controller: inputMethod.InputMethodController = inputMethod.getController()
  private dispatcher: EditorSemanticDispatcher | null = null
  private attached: boolean = false
  // composition 状态：IME setPreviewText 期间为 true，insertText commit 或 unsetPreviewText 后为 false。
  // 这是 IME 协议状态（判断 insertText 是 commit 还是普通插入），不是编辑顺序状态。
  private composing: boolean = false
  private lastPreedit: string = ''

  // 编辑框获得焦点时调用：绑定语义调度器，attach 到 IME，注册事件回调。
  // 接受 dispatcher 而非 inputAdapter + selectionController：所有输入经 dispatcher 统一排队。
  async attach(dispatcher: EditorSemanticDispatcher): Promise<void> {
    if (this.attached) {
      await this.detach()
    }
    this.dispatcher = dispatcher
    this.composing = false
    this.lastPreedit = ''
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
    this.composing = false
    this.lastPreedit = ''
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

  // IME 插入文本：composing 时为 composition commit，否则为普通文本插入。
  // 经 dispatcher 派发：dispatcher 串行队列保证 IME 事件按到达顺序执行。
  private onInsertText(text: string): void {
    if (!this.dispatcher) {
      return
    }
    if (this.composing) {
      // composition commit：携带最终提交文本
      this.composing = false
      this.lastPreedit = ''
      this.dispatcher.dispatchFireAndForget({ kind: 'compositionFinish', committed: text })
      return
    }
    // 普通文本插入：由 EditorInputAdapter 判断当前是否有选区（有选区则替换，空选区则插入）
    this.dispatcher.dispatchFireAndForget({ kind: 'insertText', text })
  }

  // IME 删除：由 EditorInputAdapter 判断当前是否有选区（有选区删整个选区，空选区删前/后一个字符）。
  // 不在这里自己假定删除一定是"光标前/后一个字符"。
  private onDeleteText(direction: inputMethod.DeleteDirection): void {
    if (!this.dispatcher) {
      return
    }
    const dir = direction === inputMethod.DeleteDirection.BACKWARD ? 'backward' : 'forward'
    this.dispatcher.dispatchFireAndForget({ kind: 'delete', direction: dir })
  }

  // IME 回车：经 dispatcher 派发 insertText('\n')，由 EditorInputAdapter 处理（尊重当前选区）。
  private onSendEnterKey(_enterKey: inputMethod.EnterKeyType): void {
    if (!this.dispatcher) {
      return
    }
    this.dispatcher.dispatchFireAndForget({ kind: 'insertText', text: '\n' })
  }

  // IME 选区变化：翻译成 setSelection/dragSelect 语义命令，经 dispatcher 派发。
  private onSetSelection(selection: inputMethod.Selection): void {
    if (!this.dispatcher) {
      return
    }
    const start = selection.start
    const end = selection.end
    if (start === end) {
      this.dispatcher.dispatchFireAndForget({ kind: 'setSelection', utf16Offset: start })
    } else {
      this.dispatcher.dispatchFireAndForget({ kind: 'dragSelect', utf16Anchor: start, utf16Head: end })
    }
  }

  /**
   * IME composition preedit：第一次触发 begin，await 成功后才 update。
   * 经 dispatcher 派发：dispatcher 串行队列保证 begin→update→update→finish/cancel 顺序。
   * 第一次 setPreviewText 必须 await dispatch(compositionBegin) 成功并拿到 Core composition session 后才能 update；
   * begin 失败时不设 composing、不 update（不让 update 读到 sessionId=0/generation=0）。
   *
   * 注意：composing 状态是 IME 协议状态（判断下一次 setPreviewText 是 begin 还是 update），
   * 不是编辑顺序状态。编辑顺序由 dispatcher 串行队列保证。
   */
  private onSetPreviewText(text: string): void {
    if (!this.dispatcher) {
      return
    }
    if (!this.composing) {
      // composition 开始：先派发 begin，await 成功后才 update。
      // 用 dispatch（非 fire-and-forget）拿结果判断 begin 是否成功。
      this.dispatcher.dispatch({ kind: 'compositionBegin' }).then((beginResult) => {
        if (!beginResult.success || !beginResult.data) {
          // begin 失败：不设 composing、不 update。避免 update 读到 sessionId=0/generation=0。
          return
        }
        this.composing = true
        this.lastPreedit = text
        // begin 成功后才派发 update。dispatcher 串行队列保证此 update 在 begin 之后执行。
        this.dispatcher.dispatchFireAndForget({ kind: 'compositionUpdate', preedit: text })
      }, (_e: Object) => {
        // begin 异常：不设 composing、不 update
      })
      return
    }
    // 后续 update：composing 已 true，直接派发 update。
    this.lastPreedit = text
    this.dispatcher.dispatchFireAndForget({ kind: 'compositionUpdate', preedit: text })
  }

  // IME 取消 composition preedit。经 dispatcher 派发。
  private onUnsetPreviewText(): void {
    if (!this.dispatcher) {
      return
    }
    if (this.composing) {
      this.composing = false
      this.lastPreedit = ''
      this.dispatcher.dispatchFireAndForget({ kind: 'compositionCancel' })
    }
  }
}
