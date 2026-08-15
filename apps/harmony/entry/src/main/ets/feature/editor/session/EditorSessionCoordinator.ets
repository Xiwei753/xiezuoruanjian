// EditorSessionCoordinator — 编辑会话协调器。
// 持有 IEditorSessionBridge 与 EditorSessionState。
// 所有文本修改发给 Core，再用 Core 返回的新 snapshot 更新 UI。不自己维护正文。

import { IEditorSessionBridge, EditorSnapshot } from '../interop/IEditorSessionBridge'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorSessionState } from './EditorSessionState'

export class EditorSessionCoordinator {
  private bridge: IEditorSessionBridge
  private state: EditorSessionState | null = null

  constructor(bridge: IEditorSessionBridge) {
    this.bridge = bridge
  }

  async open(targetId: string, initialText: string): Promise<ResultEnvelope<EditorSnapshot>> {
    const result = await this.bridge.create(initialText)
    if (result.success && result.data) {
      this.state = EditorSessionState.fromSnapshot(result.data)
    }
    return result
  }

  async close(): Promise<ResultEnvelope<boolean>> {
    if (!this.state) {
      return { success: false, errorCode: 'NO_SESSION', warnings: [], changedPaths: [], changedEntities: [] }
    }
    const sid = this.state.sessionId
    const result = await this.bridge.close(sid)
    if (result.success) {
      this.state = null
    }
    return result
  }

  private async update(snapshotPromise: Promise<ResultEnvelope<EditorSnapshot>>): Promise<ResultEnvelope<EditorSnapshot>> {
    const result = await snapshotPromise
    if (result.success && result.data && this.state) {
      this.state = this.state.withSnapshot(result.data)
    }
    return result
  }

  async insert(byteOffset: number, text: string): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.insert(this.state.sessionId, byteOffset, text))
  }

  async delete(byteOffsetStart: number, byteOffsetEnd: number): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.delete(this.state.sessionId, byteOffsetStart, byteOffsetEnd))
  }

  async replace(byteOffsetStart: number, byteOffsetEnd: number, text: string): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.replace(this.state.sessionId, byteOffsetStart, byteOffsetEnd, text))
  }

  async setSelection(anchorByteOffset: number, headByteOffset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.setSelection(this.state.sessionId, anchorByteOffset, headByteOffset))
  }

  async undo(): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.undo(this.state.sessionId))
  }

  async redo(): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.redo(this.state.sessionId))
  }

  async commitText(byteOffset: number, text: string): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.commitText(this.state.sessionId, byteOffset, text))
  }

  async deleteSurrounding(beforeByteLen: number, afterByteLen: number): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.deleteSurrounding(this.state.sessionId, beforeByteLen, afterByteLen))
  }

  async beginComposition(byteOffset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.beginComposition(this.state.sessionId, byteOffset))
  }

  async updateComposition(preedit: string): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.updateComposition(this.state.sessionId, preedit))
  }

  async finishComposition(committed: string): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.finishComposition(this.state.sessionId, committed))
  }

  async cancelComposition(): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.cancelComposition(this.state.sessionId))
  }

  getSnapshot(): EditorSnapshot | null {
    return this.state ? this.state.snapshot : null
  }

  getSessionId(): string | null {
    return this.state ? this.state.sessionId : null
  }

  getRevision(): number | null {
    return this.state ? this.state.revision : null
  }

  private noSession<T>(): ResultEnvelope<T> {
    return { success: false, errorCode: 'NO_SESSION', warnings: [], changedPaths: [], changedEntities: [] }
  }
}
