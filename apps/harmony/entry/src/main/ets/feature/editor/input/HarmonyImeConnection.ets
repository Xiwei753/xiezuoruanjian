// HarmonyImeConnection — HarmonyOS IME Kit 自绘编辑框连接。
// 把 IME Kit 事件翻译成 EditorInputAdapter / SelectionController 调用。
// 不把 HarmonyOS 原始 IME 对象传进 Rust Core；只翻译成 insert/delete/selection/composition 命令。
// composition 状态机：setPreviewText 触发 begin/update，insertText 在 composing 时触发 finish（commit），
// unsetPreviewText 触发 cancel。onCompositionFinish(committed) 不忽略 committed 参数。
//
// Issue #629 评论 5 第 1 部分：IME 事件保持顺序，把"平台输入事件有序 → Core 命令有序"固定成编辑器边界。
// - 所有 IME 事件（insertText/deleteText/sendEnterKey/setSelection/setPreviewText/unsetPreviewText）
//   都 enqueue 到 imeQueue（SerialCommandQueue），按事件到达顺序串行执行。
// - onSetPreviewText 第一次必须 await onCompositionBegin() 成功并拿到 Core composition session 后才能 update。
//   begin 失败时不设 composing、不 update（不让 update 读到 sessionId=0/generation=0）。
// - 后续连续 setPreviewText、insertText、unsetPreviewText 全部按事件到达顺序串行，
//   不再 .catch(() => {}) 后放任多个 Core 请求并发。
//
// Issue #629 评论 5 第 2 部分：不自己假定删除一定是"光标前/后一个字符"；
// 先让 EditorInputAdapter 判断当前是否有选区（onDelete 传 direction，由 adapter 决定删选区还是单字符）。

import { inputMethod } from '@kit.IMEKit'
import { EditorInputAdapter, DeleteDirection } from './EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { SerialCommandQueue } from '../session/editor_patch_logic'

export class HarmonyImeConnection {
  private controller: inputMethod.InputMethodController = inputMethod.getController()
  private inputAdapter: EditorInputAdapter | null = null
  private selectionController: SelectionController | null = null
  private attached: boolean = false
  // composition 状态：IME setPreviewText 期间为 true，insertText commit 或 unsetPreviewText 后为 false。
  private composing: boolean = false
  private lastPreedit: string = ''
  // IME 事件串行队列：保证事件按到达顺序执行（begin→update→update→finish/cancel），
  // 不放任多个 Core 请求并发。
  private imeQueue: SerialCommandQueue = new SerialCommandQueue()

  // 编辑框获得焦点时调用：绑定输入适配器，attach 到 IME，注册事件回调。
  async attach(inputAdapter: EditorInputAdapter, selectionController: SelectionController): Promise<void> {
    if (this.attached) {
      await this.detach()
    }
    this.inputAdapter = inputAdapter
    this.selectionController = selectionController
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
    this.inputAdapter = null
    this.selectionController = null
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
  // 串行 enqueue：保证 IME 事件按到达顺序执行。
  private onInsertText(text: string): void {
    this.imeQueue.enqueue<void>(async () => {
      if (!this.inputAdapter) {
        return
      }
      if (this.composing) {
        // composition commit：携带最终提交文本
        this.composing = false
        this.lastPreedit = ''
        await this.inputAdapter.onCompositionFinish(text)
        return
      }
      // 普通文本插入：由 EditorInputAdapter 判断当前是否有选区（有选区则替换，空选区则插入）
      await this.inputAdapter.onTextInput(text)
    }).catch((_e) => {
      // IME 事件失败不阻塞后续事件；错误已由 EditorInputAdapter/Coordinator 处理（recover）
    })
  }

  // IME 删除：由 EditorInputAdapter 判断当前是否有选区（有选区删整个选区，空选区删前/后一个字符）。
  // 不在这里自己假定删除一定是"光标前/后一个字符"。
  private onDeleteText(direction: inputMethod.DeleteDirection): void {
    this.imeQueue.enqueue<void>(async () => {
      if (!this.inputAdapter) {
        return
      }
      const dir: DeleteDirection = direction === inputMethod.DeleteDirection.BACKWARD ? 'backward' : 'forward'
      await this.inputAdapter.onDelete(dir)
    }).catch((_e) => {})
  }

  // IME 回车：由 EditorInputAdapter 处理（插入换行，尊重当前选区）。
  private onSendEnterKey(_enterKey: inputMethod.EnterKeyType): void {
    this.imeQueue.enqueue<void>(async () => {
      if (!this.inputAdapter) {
        return
      }
      await this.inputAdapter.onTextInput('\n')
    }).catch((_e) => {})
  }

  // IME 选区变化：翻译成 SelectionController.onDragSelect / onTap。串行 enqueue。
  private onSetSelection(selection: inputMethod.Selection): void {
    this.imeQueue.enqueue<void>(async () => {
      if (!this.selectionController) {
        return
      }
      const start = selection.start
      const end = selection.end
      if (start === end) {
        await this.selectionController.onTap(start)
      } else {
        await this.selectionController.onDragSelect(start, end)
      }
    }).catch((_e) => {})
  }

  /**
   * IME composition preedit：第一次触发 begin，await 成功后才 update。
   * 串行 enqueue：保证 begin→update→update→finish/cancel 顺序。
   * 第一次 setPreviewText 必须 await onCompositionBegin() 成功并拿到 Core composition session 后才能 update；
   * begin 失败时不设 composing、不 update（不让 update 读到 sessionId=0/generation=0）。
   */
  private onSetPreviewText(text: string): void {
    this.imeQueue.enqueue<void>(async () => {
      if (!this.inputAdapter) {
        return
      }
      if (!this.composing) {
        // composition 开始：由 EditorInputAdapter 判断当前选区作为 replace range
        const beginResult = await this.inputAdapter.onCompositionBegin()
        if (!beginResult.success || !beginResult.data) {
          // begin 失败：不设 composing、不 update。避免 update 读到 sessionId=0/generation=0。
          return
        }
        this.composing = true
      }
      this.lastPreedit = text
      await this.inputAdapter.onCompositionUpdate(text)
    }).catch((_e) => {})
  }

  // IME 取消 composition preedit。串行 enqueue。
  private onUnsetPreviewText(): void {
    this.imeQueue.enqueue<void>(async () => {
      if (!this.inputAdapter) {
        return
      }
      if (this.composing) {
        this.composing = false
        this.lastPreedit = ''
        await this.inputAdapter.onCompositionCancel()
      }
    }).catch((_e) => {})
  }
}
