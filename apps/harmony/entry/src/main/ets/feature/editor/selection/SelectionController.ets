// SelectionController — 点击命中、拖动选区、光标位置、selection command。
// 输入是 UTF-16 坐标（来自 ArkUI 事件），转 byte offset 后委托 Coordinator。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorSnapshot } from '../interop/IEditorSessionBridge'

export class SelectionController {
  private coordinator: EditorSessionCoordinator

  constructor(coordinator: EditorSessionCoordinator) {
    this.coordinator = coordinator
  }

  // 点击命中：把 UTF-16 offset 设为光标（anchor=head）。
  async onTap(utf16Offset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    const text = this.coordinator.getSnapshot()?.text ?? ''
    const byteOffset = TextOffsetMapper.utf16ToUtf8(text, utf16Offset)
    return this.coordinator.setSelection(byteOffset, byteOffset)
  }

  // 拖动选区：utf16Anchor..utf16Head。
  async onDragSelect(utf16Anchor: number, utf16Head: number): Promise<ResultEnvelope<EditorSnapshot>> {
    const text = this.coordinator.getSnapshot()?.text ?? ''
    return this.coordinator.setSelection(
      TextOffsetMapper.utf16ToUtf8(text, utf16Anchor),
      TextOffsetMapper.utf16ToUtf8(text, utf16Head)
    )
  }

  // 全选。
  async selectAll(): Promise<ResultEnvelope<EditorSnapshot>> {
    const text = this.coordinator.getSnapshot()?.text ?? ''
    const endByte = TextOffsetMapper.utf16ToUtf8(text, text.length)
    return this.coordinator.setSelection(0, endByte)
  }
}
