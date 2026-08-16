// HarmonyImeConnection — HarmonyOS IME Kit 自绘编辑框连接。
// 把 IME Kit 事件翻译成 EditorInputAdapter / SelectionController 调用。
// 不把 HarmonyOS 原始 IME 对象传进 Rust Core；只翻译成 insert/delete/selection/composition 命令。
// composition 状态机：setPreviewText 触发 begin/update，insertText 在 composing 时触发 finish（commit），
// unsetPreviewText 触发 cancel。onCompositionFinish(committed) 不忽略 committed 参数。

import { inputMethod } from '@kit.IMEKit'
import { EditorInputAdapter } from './EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'

export class HarmonyImeConnection {
  private controller: inputMethod.InputMethodController = inputMethod.getController()
  private inputAdapter: EditorInputAdapter | null = null
  private selectionController: SelectionController | null = null
  private attached: boolean = false
  // composition 状态：IME setPreviewText 期间为 true，insertText commit 或 unsetPreviewText 后为 false。
  private composing: boolean = false
  private lastPreedit: string = ''

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
  private onInsertText(text: string): void {
    if (!this.inputAdapter) {
      return
    }
    if (this.composing) {
      // composition commit：携带最终提交文本
      this.composing = false
      this.lastPreedit = ''
      this.inputAdapter.onCompositionFinish(text).then((_r) => {})
      return
    }
    // 普通文本插入：在当前光标位置插入
    const cursorOffset = this.inputAdapter.getCurrentCursorUtf16()
    this.inputAdapter.onTextInput(text, cursorOffset).then((_r) => {})
  }

  // IME 删除：BACKWARD 删光标前一个 code point，FORWARD 删光标后一个 code point。
  private onDeleteText(direction: inputMethod.DeleteDirection): void {
    if (!this.inputAdapter) {
      return
    }
    const text = this.inputAdapter.getCurrentText()
    const cursor = this.inputAdapter.getCurrentCursorUtf16()
    if (direction === inputMethod.DeleteDirection.BACKWARD) {
      if (cursor <= 0) {
        return
      }
      let start = cursor - 1
      // 处理 UTF-16 代理对：若 start 位置是低代理项，向前再删一个 code unit
      if (start > 0 && text.charCodeAt(start) >= 0xDC00 && text.charCodeAt(start) <= 0xDFFF) {
        start -= 1
      }
      this.inputAdapter.onDelete(start, cursor).then((_r) => {})
    } else {
      // FORWARD
      if (cursor >= text.length) {
        return
      }
      let end = cursor + 1
      // 处理 UTF-16 代理对：若 cursor 位置是高代理项，向后再删一个 code unit
      if (end < text.length && text.charCodeAt(cursor) >= 0xD800 && text.charCodeAt(cursor) <= 0xDBFF) {
        end += 1
      }
      this.inputAdapter.onDelete(cursor, end).then((_r) => {})
    }
  }

  // IME 回车：在当前光标位置插入换行。
  private onSendEnterKey(_enterKey: inputMethod.EnterKeyType): void {
    if (!this.inputAdapter) {
      return
    }
    const cursorOffset = this.inputAdapter.getCurrentCursorUtf16()
    this.inputAdapter.onTextInput('\n', cursorOffset).then((_r) => {})
  }

  // IME 选区变化：翻译成 SelectionController.onDragSelect / onTap。
  private onSetSelection(selection: inputMethod.Selection): void {
    if (!this.selectionController) {
      return
    }
    const start = selection.start
    const end = selection.end
    if (start === end) {
      this.selectionController.onTap(start).then((_r) => {})
    } else {
      this.selectionController.onDragSelect(start, end).then((_r) => {})
    }
  }

  // IME composition preedit：第一次触发 begin，后续触发 update。
  private onSetPreviewText(text: string): void {
    if (!this.inputAdapter) {
      return
    }
    if (!this.composing) {
      // composition 开始：在当前光标位置
      const cursorOffset = this.inputAdapter.getCurrentCursorUtf16()
      this.inputAdapter.onCompositionBegin(cursorOffset).then((_r) => {})
      this.composing = true
    }
    this.lastPreedit = text
    this.inputAdapter.onCompositionUpdate(text).then((_r) => {})
  }

  // IME 取消 composition preedit。
  private onUnsetPreviewText(): void {
    if (!this.inputAdapter) {
      return
    }
    if (this.composing) {
      this.composing = false
      this.lastPreedit = ''
      this.inputAdapter.onCompositionCancel().then((_r) => {})
    }
  }
}
