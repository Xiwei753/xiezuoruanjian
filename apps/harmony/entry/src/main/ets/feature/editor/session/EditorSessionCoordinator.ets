// EditorSessionCoordinator — 编辑会话协调器。
// 持有 IEditorSessionBridge 与 EditorSessionState。
// 所有文本修改发给 Core，再用 Core 返回的新 snapshot 更新 UI。不自己维护正文。
// expectedRevision 由本协调器从 state.revision 传入；cause 由调用方按输入语义传入。

import { IEditorSessionBridge, EditorSnapshot } from '../interop/IEditorSessionBridge'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'
import { EditorSessionState } from './EditorSessionState'

export class EditorSessionCoordinator {
  private bridge: IEditorSessionBridge
  private state: EditorSessionState | null = null

  constructor(bridge: IEditorSessionBridge) {
    this.bridge = bridge
  }

  async open(targetId: string, initialText: string): Promise<ResultEnvelope<EditorSnapshot>> {
    const result = await this.bridge.create(targetId, initialText, 0, false)
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

  async insert(byteOffset: number, text: string, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.insert(this.state.sessionId, byteOffset, text, cause, this.state.revision))
  }

  async delete(byteOffsetStart: number, byteOffsetEnd: number, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.delete(this.state.sessionId, byteOffsetStart, byteOffsetEnd, cause, this.state.revision))
  }

  async replace(byteOffsetStart: number, byteOffsetEnd: number, text: string, originalText: string, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.replace(this.state.sessionId, byteOffsetStart, byteOffsetEnd, text, originalText, cause, this.state.revision))
  }

  async setSelection(anchorByteOffset: number, headByteOffset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.setSelection(this.state.sessionId, anchorByteOffset, headByteOffset, this.state.revision))
  }

  async undo(): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.undo(this.state.sessionId, this.state.revision))
  }

  async redo(): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.redo(this.state.sessionId, this.state.revision))
  }

  async commitText(byteStart: number, byteEndExclusive: number, replacementText: string, resultingSelectionAnchor: number, resultingSelectionHead: number, compositionSessionId: number, compositionBaseRevision: number, compositionGeneration: number, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.commitText(this.state.sessionId, byteStart, byteEndExclusive, replacementText, resultingSelectionAnchor, resultingSelectionHead, compositionSessionId, compositionBaseRevision, compositionGeneration, cause, this.state.revision))
  }

  async deleteSurrounding(beforeByteStart: number, beforeByteEndExclusive: number, afterByteStart: number, afterByteEndExclusive: number, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.deleteSurrounding(this.state.sessionId, beforeByteStart, beforeByteEndExclusive, afterByteStart, afterByteEndExclusive, cause, this.state.revision))
  }

  async beginComposition(replaceStart: number, replaceEndExclusive: number): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.beginComposition(this.state.sessionId, replaceStart, replaceEndExclusive, this.state.revision))
  }

  async updateComposition(compositionSessionId: number, compositionGeneration: number, newPreeditText: string, newPreeditCursorOffset: number): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.updateComposition(this.state.sessionId, compositionSessionId, compositionGeneration, newPreeditText, newPreeditCursorOffset, this.state.revision))
  }

  async finishComposition(compositionSessionId: number, compositionGeneration: number): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.finishComposition(this.state.sessionId, compositionSessionId, compositionGeneration, this.state.revision))
  }

  async cancelComposition(compositionSessionId: number, compositionGeneration: number): Promise<ResultEnvelope<EditorSnapshot>> {
    if (!this.state) return this.noSession()
    return this.update(this.bridge.cancelComposition(this.state.sessionId, compositionSessionId, compositionGeneration, this.state.revision))
  }

  getSnapshot(): EditorSnapshot | null {
    return this.state ? this.state.snapshot : null
  }

  getSessionId(): number | null {
    return this.state ? this.state.sessionId : null
  }

  getRevision(): number | null {
    return this.state ? this.state.revision : null
  }

  private noSession<T>(): ResultEnvelope<T> {
    return { success: false, errorCode: 'NO_SESSION', warnings: [], changedPaths: [], changedEntities: [] }
  }
}
