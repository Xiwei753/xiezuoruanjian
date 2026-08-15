// EditorImeSession — IME composition 会话状态。
// 持有 compositionSessionId / generation / preedit。composition 由 Core 驱动，本类只跟踪状态。
// compositionSessionId 是 Core u64，实际值 < 2^53，ArkTS 用 number。

export class EditorImeSession {
  private compositionSessionId: number | null = null
  private generation: number = 0
  private preedit: string = ''
  private nextId: number = 1

  isActive(): boolean {
    return this.compositionSessionId !== null
  }

  begin(): void {
    this.compositionSessionId = this.nextId
    this.nextId += 1
    this.generation += 1
    this.preedit = ''
  }

  updatePreedit(preedit: string): void {
    this.preedit = preedit
  }

  finish(): { sessionId: number; generation: number; preedit: string } | null {
    if (this.compositionSessionId === null) return null
    const result = { sessionId: this.compositionSessionId, generation: this.generation, preedit: this.preedit }
    this.compositionSessionId = null
    this.preedit = ''
    return result
  }

  cancel(): { sessionId: number; generation: number } | null {
    if (this.compositionSessionId === null) return null
    const result = { sessionId: this.compositionSessionId, generation: this.generation }
    this.compositionSessionId = null
    this.preedit = ''
    return result
  }

  getPreedit(): string {
    return this.preedit
  }

  getGeneration(): number {
    return this.generation
  }

  getCompositionSessionId(): number {
    return this.compositionSessionId ?? 0
  }
}
