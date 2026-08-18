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
  // Issue #629 R9 评论5326213302 第4项：visual hint 绑定 revision/generation/compositionGeneration+offset。
  // 文本身份或 composition generation 变化即失效，避免旧 affinity 跨 revision 残留。
  // 无命中时返回默认 Downstream。
  private visualCaret: VisualCaretPosition | null = null
  private visualCaretRevision: number = -1
  private visualCaretGeneration: number = -1
  private visualCaretCompositionGeneration: number = -1
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

  // Issue #629 R9 评论5326213302 第4项：visual hint 绑定 revision/generation/compositionGeneration+offset。
  // 只有身份（revision+generation+compositionGeneration）和 offset 都匹配时返回 hint，否则默认 Downstream。
  // 这样 forward delete / composition forward delete / replace 等文本身份变化时旧 affinity 不残留。
  getVisualCaret(cursorUtf16: number): VisualCaretPosition {
    if (this.visualCaret !== null
      && this.visualCaret.utf16Offset === cursorUtf16
      && this.coordinator.getSnapshot() !== null
      && this.coordinator.getSnapshot() !== undefined) {
      const snap = this.coordinator.getSnapshot()!
      if ((snap.revision ?? 0) === this.visualCaretRevision
        && (snap.generation ?? 0) === this.visualCaretGeneration
        && (snap.compositionGeneration ?? -1) === this.visualCaretCompositionGeneration) {
        return this.visualCaret
      }
    }
    return { utf16Offset: cursorUtf16, affinity: CaretAffinity.Downstream }
  }

  // Issue #629 R7 + R9：唯一 owner 语义——只更新/发布视觉位置，不重复发 Core selection。
  // 从 Coordinator 当前 snapshot 记录身份（revision+generation+compositionGeneration），
  // 保证文本身份或 composition generation 变化时旧 hint 自动失效。
  rememberVisualCaret(position: VisualCaretPosition): void {
    this.visualCaret = position
    const snap = this.coordinator.getSnapshot()
    this.visualCaretRevision = snap?.revision ?? 0
    this.visualCaretGeneration = snap?.generation ?? 0
    this.visualCaretCompositionGeneration = snap?.compositionGeneration ?? -1
    this.visualCaretListener?.(position)
  }

  // Issue #629 评论18 + R9：发布 visual caret position（命中测试成功后调用）。
  private publishVisualCaret(position: VisualCaretPosition): void {
    this.visualCaret = position
    const snap = this.coordinator.getSnapshot()
    this.visualCaretRevision = snap?.revision ?? 0
    this.visualCaretGeneration = snap?.generation ?? 0
    this.visualCaretCompositionGeneration = snap?.compositionGeneration ?? -1
    this.visualCaretListener?.(position)
  }

  // Issue #629 R9 评论5326213302 第4项：宽度/字体变化调用 invalidateVisualCaret。
  // 布局宽度变化不改 Core revision，但 soft-wrap 可能重新发生，旧 affinity 可能不再有效。
  invalidateVisualCaret(): void {
    this.visualCaret = null
    this.visualCaretRevision = -1
    this.visualCaretGeneration = -1
    this.visualCaretCompositionGeneration = -1
    this.visualCaretListener?.(null)
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

  // Issue #629 评论18 + 评论5324447292 第1项：拖动选区：anchor/head 都是 VisualCaretPosition（含 affinity）。
  // 向后兼容：如果参数是 number，按旧签名处理（默认 Downstream affinity）。
  async onDragSelect(utf16Anchor: VisualCaretPosition | number, utf16Head: VisualCaretPosition | number): Promise<ResultEnvelope<EditorEditResult>> {
    const anchorOffset = typeof utf16Anchor === 'number' ? utf16Anchor : utf16Anchor.utf16Offset
    const headOffset = typeof utf16Head === 'number' ? utf16Head : utf16Head.utf16Offset
    const headAffinity = typeof utf16Head === 'number' ? CaretAffinity.Downstream : utf16Head.affinity
    const text = this.coordinator.getSnapshot()?.text ?? ''
    const result = await this.coordinator.setSelection(
      TextOffsetMapper.utf16ToUtf8(text, anchorOffset),
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
