// EditorInputAdapter — 把 HarmonyOS 键盘/IME/粘贴翻译成 insert/delete/replace/composition 编辑命令。
// 委托 EditorSessionCoordinator。不自己维护正文或光标。
// cause 由本适配器按输入语义决定（Typing/Delete/Paste/ImeComposition/TypingCommit）。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { EditorImeSession } from './EditorImeSession'
import { TextOffsetMapper } from './TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'
import { EditorSnapshot } from '../interop/IEditorSessionBridge'

export class EditorInputAdapter {
  private coordinator: EditorSessionCoordinator
  private imeSession: EditorImeSession = new EditorImeSession()

  constructor(coordinator: EditorSessionCoordinator) {
    this.coordinator = coordinator
  }

  // 普通字符输入（UTF-16 offset 来自 ArkTS 事件）。
  async onTextInput(text: string, utf16Offset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    const byteOffset = TextOffsetMapper.utf16ToUtf8(this.getCurrentText(), utf16Offset)
    return this.coordinator.insert(byteOffset, text, EditorTransactionCause.Typing)
  }

  // 删除（backspace/delete）。
  async onDelete(utf16Start: number, utf16End: number): Promise<ResultEnvelope<EditorSnapshot>> {
    const text = this.getCurrentText()
    return this.coordinator.delete(
      TextOffsetMapper.utf16ToUtf8(text, utf16Start),
      TextOffsetMapper.utf16ToUtf8(text, utf16End),
      EditorTransactionCause.Delete
    )
  }

  // 粘贴：在 byteOffset 处插入 text（替换空范围，originalText 为空）。
  async onPaste(text: string, utf16Offset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    const byteOffset = TextOffsetMapper.utf16ToUtf8(this.getCurrentText(), utf16Offset)
    return this.coordinator.replace(byteOffset, byteOffset, text, '', EditorTransactionCause.Paste)
  }

  // IME composition 开始：替换范围 [byteOffset, byteOffset]（空范围）。
  async onCompositionBegin(utf16Offset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    const byteOffset = TextOffsetMapper.utf16ToUtf8(this.getCurrentText(), utf16Offset)
    this.imeSession.begin()
    return this.coordinator.beginComposition(byteOffset, byteOffset)
  }

  async onCompositionUpdate(preedit: string): Promise<ResultEnvelope<EditorSnapshot>> {
    this.imeSession.updatePreedit(preedit)
    const compId = this.imeSession.getCompositionSessionId()
    const gen = this.imeSession.getGeneration()
    const cursorByte = TextOffsetMapper.utf16ToUtf8(preedit, preedit.length)
    return this.coordinator.updateComposition(compId, gen, preedit, cursorByte)
  }

  async onCompositionFinish(committed: string): Promise<ResultEnvelope<EditorSnapshot>> {
    const comp = this.imeSession.finish()
    if (!comp) {
      return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    return this.coordinator.finishComposition(comp.sessionId, comp.generation)
  }

  async onCompositionCancel(): Promise<ResultEnvelope<EditorSnapshot>> {
    const comp = this.imeSession.cancel()
    if (!comp) {
      return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    return this.coordinator.cancelComposition(comp.sessionId, comp.generation)
  }

  // onChange 整篇变化：把整篇替换翻译成 replace 编辑命令发给 Core。
  // ArkUI TextArea/SujianEditor 的 onChange 返回整篇文本，本方法把它转成 replace 编辑命令。
  async onContentChange(oldText: string, newText: string): Promise<ResultEnvelope<EditorSnapshot>> {
    const byteStart = TextOffsetMapper.utf16ToUtf8(oldText, 0)
    const byteEnd = TextOffsetMapper.utf16ToUtf8(oldText, oldText.length)
    return this.coordinator.replace(byteStart, byteEnd, newText, oldText, EditorTransactionCause.Typing)
  }

  isComposing(): boolean {
    return this.imeSession.isActive()
  }

  private getCurrentText(): string {
    const snap = this.coordinator.getSnapshot()
    return snap ? snap.text : ''
  }
}
