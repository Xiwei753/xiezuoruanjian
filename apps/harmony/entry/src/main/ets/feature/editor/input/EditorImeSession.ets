// EditorImeSession — IME composition 会话状态。
// 持有 compositionSessionId / generation / preedit。composition 由 Core 驱动，本类只跟踪状态。

export class EditorImeSession {
  private compositionSessionId: string | null = null
  private generation: number = 0
  private preedit: string = ''

  isActive(): boolean {
    return this.compositionSessionId !== null
  }

  begin(sessionId: string): void {
    this.compositionSessionId = sessionId
    this.generation += 1
    this.preedit = ''
  }

  updatePreedit(preedit: string): void {
    this.preedit = preedit
  }

  finish(): { sessionId: string; preedit: string } | null {
    if (this.compositionSessionId === null) return null
    const result = { sessionId: this.compositionSessionId, preedit: this.preedit }
    this.compositionSessionId = null
    this.preedit = ''
    return result
  }

  cancel(): void {
    this.compositionSessionId = null
    this.preedit = ''
  }

  getPreedit(): string {
    return this.preedit
  }

  getGeneration(): number {
    return this.generation
  }
}
