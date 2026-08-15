// EditorInputAdapter — 把 HarmonyOS 键盘/IME/粘贴翻译成 insert/delete/replace/composition 编辑命令。
// 委托 EditorSessionCoordinator。不自己维护正文或光标。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { EditorImeSession } from './EditorImeSession'
import { TextOffsetMapper } from './TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
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
    return this.coordinator.insert(byteOffset, text)
  }

  // 删除（backspace/delete）。
  async onDelete(utf16Start: number, utf16End: number): Promise<ResultEnvelope<EditorSnapshot>> {
    const text = this.getCurrentText()
    return this.coordinator.delete(
      TextOffsetMapper.utf16ToUtf8(text, utf16Start),
      TextOffsetMapper.utf16ToUtf8(text, utf16End)
    )
  }

  // 粘贴。
  async onPaste(text: string, utf16Offset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    const byteOffset = TextOffsetMapper.utf16ToUtf8(this.getCurrentText(), utf16Offset)
    return this.coordinator.replace(byteOffset, byteOffset, text)
  }

  // IME composition 开始。
  async onCompositionBegin(utf16Offset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    const byteOffset = TextOffsetMapper.utf16ToUtf8(this.getCurrentText(), utf16Offset)
    this.imeSession.begin("comp-" + Date.now().toString())
    return this.coordinator.beginComposition(byteOffset)
  }

  async onCompositionUpdate(preedit: string): Promise<ResultEnvelope<EditorSnapshot>> {
    this.imeSession.updatePreedit(preedit)
    return this.coordinator.updateComposition(preedit)
  }

  async onCompositionFinish(committed: string): Promise<ResultEnvelope<EditorSnapshot>> {
    this.imeSession.finish()
    return this.coordinator.finishComposition(committed)
  }

  async onCompositionCancel(): Promise<ResultEnvelope<EditorSnapshot>> {
    this.imeSession.cancel()
    return this.coordinator.cancelComposition()
  }

  isComposing(): boolean {
    return this.imeSession.isActive()
  }

  private getCurrentText(): string {
    const snap = this.coordinator.getSnapshot()
    return snap ? snap.text : ''
  }
}
