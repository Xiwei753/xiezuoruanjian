// EditorImeSession — IME composition 会话状态。
// #629 评论3：compositionSessionId/generation 由 Core 在 beginComposition/updateComposition
// 返回的 EditorEditResult.compositionSession 中给出，ArkTS 端只持有不伪造。
// 删除本地 nextId/generation 自增。

import { CompositionSession } from '../../../corebridge/dto/EditorDtos'

export class EditorImeSession {
  private compositionSession: CompositionSession | null = null
  private preedit: string = ''

  isActive(): boolean {
    return this.compositionSession !== null
  }

  // 从 Core 的 EditorEditResult.compositionSession 设置真实会话标识。
  setCompositionSession(session: CompositionSession | null): void {
    this.compositionSession = session
    if (session === null) {
      this.preedit = ''
    }
  }

  getCompositionSession(): CompositionSession | null {
    return this.compositionSession
  }

  getCompositionSessionId(): number {
    return this.compositionSession ? this.compositionSession.sessionId : 0
  }

  getGeneration(): number {
    return this.compositionSession ? this.compositionSession.generation : 0
  }

  getBaseRevision(): number {
    return this.compositionSession ? this.compositionSession.baseRevision : 0
  }

  updatePreedit(preedit: string): void {
    this.preedit = preedit
  }

  getPreedit(): string {
    return this.preedit
  }

  // 返回当前 Core compositionSession 值（不清空；清空由 setCompositionSession(null) 显式调用）。
  finish(): { sessionId: number; generation: number; preedit: string } | null {
    if (this.compositionSession === null) return null
    return { sessionId: this.compositionSession.sessionId, generation: this.compositionSession.generation, preedit: this.preedit }
  }

  cancel(): { sessionId: number; generation: number } | null {
    if (this.compositionSession === null) return null
    return { sessionId: this.compositionSession.sessionId, generation: this.compositionSession.generation }
  }

  clear(): void {
    this.compositionSession = null
    this.preedit = ''
  }
}
