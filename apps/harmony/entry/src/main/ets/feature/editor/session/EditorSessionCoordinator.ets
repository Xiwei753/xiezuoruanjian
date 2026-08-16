// EditorSessionCoordinator — 编辑会话协调器。
// 持有 IEditorSessionBridge 与 EditorSessionState。
// 所有文本修改发给 Core，再用 Core 返回的 EditorEditResult 增量更新 state 并 notifyListener。不自己维护正文。
// expectedRevision 由本协调器从 state.snapshot.revision 传入；cause 由调用方按输入语义传入。
// Issue #629 评论 3：open() create→sessionId→snapshot 二步；applyEdit 消费 EditorEditResult；
// setStateListener 回流 EditorSessionSnapshot 供 Screen 消费。

import { IEditorSessionBridge } from '../interop/IEditorSessionBridge'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { EditorSessionState } from './EditorSessionState'

// 状态监听器：每次 state 变化（open/编辑命令成功）时回调，Screen 用来回流 editorSnapshot/content/hasUnsaved/charCount。
export type EditorStateListener = (snapshot: EditorSessionSnapshot) => void

export class EditorSessionCoordinator {
  private bridge: IEditorSessionBridge
  private state: EditorSessionState | null = null
  private stateListener: EditorStateListener | null = null

  constructor(bridge: IEditorSessionBridge) {
    this.bridge = bridge
  }

  // 注册状态监听器。传 null 清除。Screen 在 aboutToAppear 注册，aboutToDisappear 清除。
  setStateListener(listener: EditorStateListener | null): void {
    this.stateListener = listener
  }

  private notifyListener(): void {
    if (this.stateListener && this.state) {
      this.stateListener(this.state.snapshot)
    }
  }

  // open：create→sessionId→snapshot→state→notifyListener。返回初始 snapshot 供调用方判断成功。
  async open(targetId: string, initialText: string): Promise<ResultEnvelope<EditorSessionSnapshot>> {
    const createResult = await this.bridge.create(targetId, initialText, 0, false)
    if (!createResult.success || createResult.data === undefined || createResult.data === null) {
      return {
        success: false,
        errorCode: createResult.errorCode ?? 'CREATE_FAILED',
        warnings: createResult.warnings,
        changedPaths: createResult.changedPaths,
        changedEntities: createResult.changedEntities
      }
    }
    const sessionId: number = createResult.data
    const snapResult = await this.bridge.snapshot(sessionId)
    if (!snapResult.success || !snapResult.data) {
      return {
        success: false,
        errorCode: snapResult.errorCode ?? 'SNAPSHOT_FAILED',
        warnings: snapResult.warnings,
        changedPaths: snapResult.changedPaths,
        changedEntities: snapResult.changedEntities
      }
    }
    this.state = EditorSessionState.fromCreate(sessionId, snapResult.data)
    this.notifyListener()
    return {
      success: true,
      data: snapResult.data,
      warnings: snapResult.warnings,
      changedPaths: snapResult.changedPaths,
      changedEntities: snapResult.changedEntities
    }
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

  // applyEdit：消费 EditorEditResult，增量更新 state（由 EditorSessionState.withEditResult 决定是否更新），再 notifyListener。
  private async applyEdit(editPromise: Promise<ResultEnvelope<EditorEditResult>>): Promise<ResultEnvelope<EditorEditResult>> {
    const result = await editPromise
    if (result.success && result.data && this.state) {
      this.state = this.state.withEditResult(result.data)
      this.notifyListener()
    }
    return result
  }

  async insert(byteOffset: number, text: string, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.insert(this.state.sessionId, byteOffset, text, cause, this.state.snapshot.revision))
  }

  async delete(byteOffsetStart: number, byteOffsetEnd: number, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.delete(this.state.sessionId, byteOffsetStart, byteOffsetEnd, cause, this.state.snapshot.revision))
  }

  async replace(byteOffsetStart: number, byteOffsetEnd: number, text: string, originalText: string, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.replace(this.state.sessionId, byteOffsetStart, byteOffsetEnd, text, originalText, cause, this.state.snapshot.revision))
  }

  async setSelection(anchorByteOffset: number, headByteOffset: number): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.setSelection(this.state.sessionId, anchorByteOffset, headByteOffset, this.state.snapshot.revision))
  }

  async undo(): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.undo(this.state.sessionId, this.state.snapshot.revision))
  }

  async redo(): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.redo(this.state.sessionId, this.state.snapshot.revision))
  }

  async commitText(byteStart: number, byteEndExclusive: number, replacementText: string, resultingSelectionAnchor: number, resultingSelectionHead: number, compositionSessionId: number, compositionBaseRevision: number, compositionGeneration: number, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.commitText(this.state.sessionId, byteStart, byteEndExclusive, replacementText, resultingSelectionAnchor, resultingSelectionHead, compositionSessionId, compositionBaseRevision, compositionGeneration, cause, this.state.snapshot.revision))
  }

  async deleteSurrounding(beforeByteStart: number, beforeByteEndExclusive: number, afterByteStart: number, afterByteEndExclusive: number, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.deleteSurrounding(this.state.sessionId, beforeByteStart, beforeByteEndExclusive, afterByteStart, afterByteEndExclusive, cause, this.state.snapshot.revision))
  }

  async beginComposition(replaceStart: number, replaceEndExclusive: number): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.beginComposition(this.state.sessionId, replaceStart, replaceEndExclusive, this.state.snapshot.revision))
  }

  async updateComposition(compositionSessionId: number, compositionGeneration: number, newPreeditText: string, newPreeditCursorOffset: number): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.updateComposition(this.state.sessionId, compositionSessionId, compositionGeneration, newPreeditText, newPreeditCursorOffset, this.state.snapshot.revision))
  }

  async finishComposition(compositionSessionId: number, compositionGeneration: number): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.finishComposition(this.state.sessionId, compositionSessionId, compositionGeneration, this.state.snapshot.revision))
  }

  async cancelComposition(compositionSessionId: number, compositionGeneration: number): Promise<ResultEnvelope<EditorEditResult>> {
    if (!this.state) return this.noSession()
    return this.applyEdit(this.bridge.cancelComposition(this.state.sessionId, compositionSessionId, compositionGeneration, this.state.snapshot.revision))
  }

  getSnapshot(): EditorSessionSnapshot | null {
    return this.state ? this.state.snapshot : null
  }

  getSessionId(): number | null {
    return this.state ? this.state.sessionId : null
  }

  getRevision(): number | null {
    return this.state ? this.state.snapshot.revision : null
  }

  private noSession<T>(): ResultEnvelope<T> {
    return { success: false, errorCode: 'NO_SESSION', warnings: [], changedPaths: [], changedEntities: [] }
  }
}
