// EditorInputAdapter — 把 HarmonyOS 键盘/IME/粘贴翻译成 insert/delete/replace/composition 编辑命令。
// 委托 EditorSessionCoordinator。不自己维护正文或光标。
// cause 由本适配器按输入语义决定（Typing/Delete/Paste/ImeComposition/TypingCommit）。
// Issue #629 评论 3：composition 用 Core 返回的 compositionSession（不伪造 generation）；
// onCompositionFinish committed!=preedit 改成 updateComposition→新 generation→finishComposition（不 cancel 后 commitText）；
// getCurrentCursorUtf16 用 snap.cursor。

import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { EditorImeSession } from './EditorImeSession'
import { TextOffsetMapper } from './TextOffsetMapper'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorEditResult, CompositionSession } from '../../../corebridge/dto/EditorDtos'

export class EditorInputAdapter {
  private coordinator: EditorSessionCoordinator
  private imeSession: EditorImeSession = new EditorImeSession()

  constructor(coordinator: EditorSessionCoordinator) {
    this.coordinator = coordinator
  }

  // 普通字符输入（UTF-16 offset 来自 ArkTS 事件）。
  async onTextInput(text: string, utf16Offset: number): Promise<ResultEnvelope<EditorEditResult>> {
    const byteOffset = TextOffsetMapper.utf16ToUtf8(this.getCurrentText(), utf16Offset)
    return this.coordinator.insert(byteOffset, text, EditorTransactionCause.Typing)
  }

  // 删除（backspace/delete）。
  async onDelete(utf16Start: number, utf16End: number): Promise<ResultEnvelope<EditorEditResult>> {
    const text = this.getCurrentText()
    return this.coordinator.delete(
      TextOffsetMapper.utf16ToUtf8(text, utf16Start),
      TextOffsetMapper.utf16ToUtf8(text, utf16End),
      EditorTransactionCause.Delete
    )
  }

  // 粘贴：在 byteOffset 处插入 text（替换空范围，originalText 为空）。
  async onPaste(text: string, utf16Offset: number): Promise<ResultEnvelope<EditorEditResult>> {
    const byteOffset = TextOffsetMapper.utf16ToUtf8(this.getCurrentText(), utf16Offset)
    return this.coordinator.replace(byteOffset, byteOffset, text, '', EditorTransactionCause.Paste)
  }

  // IME composition 开始：替换范围 [byteOffset, byteOffset]（空范围）。
  // 从 result.compositionSession 设 imeSession（Core 驱动 composition 会话）。
  async onCompositionBegin(utf16Offset: number): Promise<ResultEnvelope<EditorEditResult>> {
    const byteOffset = TextOffsetMapper.utf16ToUtf8(this.getCurrentText(), utf16Offset)
    const result = await this.coordinator.beginComposition(byteOffset, byteOffset)
    if (result.success && result.data && result.data.compositionSession) {
      this.imeSession.setCompositionSession(result.data.compositionSession)
    }
    return result
  }

  // IME composition preedit 更新：从 result.compositionSession 更新 imeSession。
  async onCompositionUpdate(preedit: string): Promise<ResultEnvelope<EditorEditResult>> {
    this.imeSession.updatePreedit(preedit)
    const compId = this.imeSession.getCompositionSessionId()
    const gen = this.imeSession.getGeneration()
    const cursorByte = TextOffsetMapper.utf16ToUtf8(preedit, preedit.length)
    const result = await this.coordinator.updateComposition(compId, gen, preedit, cursorByte)
    if (result.success && result.data && result.data.compositionSession) {
      this.imeSession.setCompositionSession(result.data.compositionSession)
    }
    return result
  }

  // IME composition 提交。committed 是 IME 最终提交文本，不能忽略。
  // - committed == preedit → finishComposition（确认原样提交）
  // - committed != preedit → updateComposition（把 preedit 改成 committed）→ 从 result.compositionSession
  //   取新 generation → finishComposition。
  //   禁止 cancel 后 commitText（评论明确说这会 StaleRevision）。
  async onCompositionFinish(committed: string): Promise<ResultEnvelope<EditorEditResult>> {
    const comp = this.imeSession.finish()
    if (!comp) {
      return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    if (committed === comp.preedit) {
      const result = await this.coordinator.finishComposition(comp.sessionId, comp.generation)
      this.imeSession.clear()
      return result
    }
    // committed != preedit：updateComposition 提交最终文本，再用新 generation finishComposition
    const cursorByte = TextOffsetMapper.utf16ToUtf8(committed, committed.length)
    const updateResult = await this.coordinator.updateComposition(comp.sessionId, comp.generation, committed, cursorByte)
    if (!updateResult.success || !updateResult.data) {
      return updateResult
    }
    const newCompSession: CompositionSession | null = updateResult.data.compositionSession
    if (!newCompSession) {
      return {
        success: false,
        errorCode: 'NO_COMPOSITION_SESSION',
        warnings: updateResult.warnings,
        changedPaths: updateResult.changedPaths,
        changedEntities: updateResult.changedEntities
      }
    }
    this.imeSession.setCompositionSession(newCompSession)
    const finishResult = await this.coordinator.finishComposition(newCompSession.sessionId, newCompSession.generation)
    this.imeSession.clear()
    return finishResult
  }

  async onCompositionCancel(): Promise<ResultEnvelope<EditorEditResult>> {
    const comp = this.imeSession.cancel()
    if (!comp) {
      return { success: false, errorCode: 'NO_COMPOSITION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    const result = await this.coordinator.cancelComposition(comp.sessionId, comp.generation)
    this.imeSession.clear()
    return result
  }

  isComposing(): boolean {
    return this.imeSession.isActive()
  }

  // 当前光标的 UTF-16 code unit offset（供 HarmonyImeConnection 查询）。
  // Issue #629 评论 3：用 snap.cursor（而非 selectionHead）。
  getCurrentCursorUtf16(): number {
    const snap = this.coordinator.getSnapshot()
    if (!snap) {
      return 0
    }
    return TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursor)
  }

  // 当前正文（UTF-16 string，供 HarmonyImeConnection 删除范围计算用）。
  getCurrentText(): string {
    const snap = this.coordinator.getSnapshot()
    return snap ? snap.text : ''
  }
}
