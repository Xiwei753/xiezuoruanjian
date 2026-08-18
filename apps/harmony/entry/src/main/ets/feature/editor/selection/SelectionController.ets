// SelectionController — 点击命中、拖动选区、光标位置、selection command。
// 输入是 UTF-16 坐标（来自 ArkUI 事件），转 byte offset 后委托 Coordinator。
// Issue #629 评论 3：编辑命令返回 EditorEditResult（由 Coordinator 消费+listener 回流）。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { CaretAffinity } from '../render/editor_layout_math'
import type { VisualCaretPosition } from '../render/editor_layout_math'

export class SelectionController {
  private coordinator: EditorSessionCoordinator
  // Issue #629 评论18：visualCaret 记录最近一次命中的 VisualCaretPosition（含 affinity）。
  // renderLayout 消费此 affinity 渲染光标，保证光标在 soft-wrap 边界上位置正确。
  // 无命中时保持 Downstream（默认值）。
  private visualCaret: VisualCaretPosition | null = null
  private visualCaretListener: ((position: VisualCaretPosition | null) => void) | null = null

  constructor(coordinator: EditorSessionCoordinator) {
    this.coordinator = coordinator
  }

  // Issue #629 评论18：注册 visual caret 监听器（SujianEditor 订阅后传 affinity 给 renderLayout）。
  registerVisualCaretListener(listener: (position: VisualCaretPosition | null) => void): void {
    this.visualCaretListener = listener
  }

  unregisterVisualCaretListener(): void {
    this.visualCaretListener = null
  }

  // Issue #629 评论18：获取当前 visual caret position。
  // 如果命中测试设置了 visualCaret，且 offset 匹配，返回带 affinity 的位置；
  // 否则返回默认 Downstream（普通键盘输入场景）。
  getVisualCaret(cursorUtf16: number): VisualCaretPosition {
    if (this.visualCaret !== null && this.visualCaret.utf16Offset === cursorUtf16) {
      return this.visualCaret
    }
    return { utf16Offset: cursorUtf16, affinity: CaretAffinity.Downstream }
  }

  // Issue #629 评论18：发布 visual caret position（命中测试成功后调用）。
  private publishVisualCaret(position: VisualCaretPosition): void {
    this.visualCaret = position
    this.visualCaretListener?.(position)
  }

  // Issue #629 评论18：点击命中接受完整 VisualCaretPosition（含 affinity）。
  // Core 仍只收 position.utf16Offset 转出来的 byte offset；成功后 publishVisualCaret。
  async onTap(position: VisualCaretPosition | number): Promise<ResultEnvelope<EditorEditResult>> {
    const utf16Offset = typeof position === 'number' ? position : position.utf16Offset
    const affinity = typeof position === 'number' ? CaretAffinity.Downstream : position.affinity
    const text = this.coordinator.getSnapshot()?.text ?? ''
    const byteOffset = TextOffsetMapper.utf16ToUtf8(text, utf16Offset)
    const result = await this.coordinator.setSelection(byteOffset, byteOffset)
    if (result.success) {
      this.publishVisualCaret({ utf16Offset, affinity })
    }
    return result
  }

  // Issue #629 评论18：拖动选区：anchor 是逻辑 offset，head 是 VisualCaretPosition（含 affinity）。
  // 向后兼容：如果 head 是 number，按旧签名处理。
  async onDragSelect(utf16Anchor: number, utf16Head: VisualCaretPosition | number): Promise<ResultEnvelope<EditorEditResult>> {
    const headOffset = typeof utf16Head === 'number' ? utf16Head : utf16Head.utf16Offset
    const headAffinity = typeof utf16Head === 'number' ? CaretAffinity.Downstream : utf16Head.affinity
    const text = this.coordinator.getSnapshot()?.text ?? ''
    const result = await this.coordinator.setSelection(
      TextOffsetMapper.utf16ToUtf8(text, utf16Anchor),
      TextOffsetMapper.utf16ToUtf8(text, headOffset)
    )
    if (result.success) {
      this.publishVisualCaret({ utf16Offset: headOffset, affinity: headAffinity })
    }
    return result
  }

  // 全选。
  async selectAll(): Promise<ResultEnvelope<EditorEditResult>> {
    const text = this.coordinator.getSnapshot()?.text ?? ''
    const endByte = TextOffsetMapper.utf16ToUtf8(text, text.length)
    return this.coordinator.setSelection(0, endByte)
  }
}
