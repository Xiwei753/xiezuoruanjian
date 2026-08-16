// EditorSessionCoordinator — 编辑会话协调器。
// 持有 IEditorSessionBridge 与 EditorSessionState。
// 所有文本修改发给 Core，再用 Core 返回的 EditorEditResult 增量更新 state 并 notifyListener。不自己维护正文。
// cause 由调用方按输入语义传入。
// Issue #629 评论 3：open() create→sessionId→snapshot 二步；applyEdit 消费 EditorEditResult；
// setStateListener 回流 EditorSessionSnapshot 供 Screen 消费。
//
// Issue #629 评论 5 第 1 部分：编辑命令真正串行。
// - 不再把已启动的 Promise<ResultEnvelope<EditorEditResult>> 传给 applyEdit()。
// - 在 Coordinator 内放一个 SerialCommandQueue，insert/delete/replace/setSelection/undo/redo/
//   beginComposition/updateComposition/finishComposition/cancelComposition/commitText/deleteSurrounding 都 enqueue。
// - 每条命令真正出队执行时，再读取 this.state.snapshot.revision，再调用 bridge。
// - 前一条命令完成并更新 state 后，下一条才能开始。
//
// Issue #629 评论 5 第 3 部分：patch 失败真实恢复。
// - 收到 patch 应用失败或 StaleRevision 时，立即 bridge.snapshot(sessionId)，用 Core 完整 snapshot 重建 state，再 notify UI。
// - 不让 UI 静默停在旧文本。

import { IEditorSessionBridge } from '../interop/IEditorSessionBridge'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { EditorSessionState, EditStateOutcome } from './EditorSessionState'
import { SerialCommandQueue } from './editor_patch_logic'

// 状态监听器：每次 state 变化（open/编辑命令成功/失败恢复）时回调，Screen 用来回流 editorSnapshot/content/hasUnsaved/charCount。
export type EditorStateListener = (snapshot: EditorSessionSnapshot) => void

export class EditorSessionCoordinator {
  private bridge: IEditorSessionBridge
  private state: EditorSessionState | null = null
  private stateListener: EditorStateListener | null = null
  // 串行命令队列：保证编辑命令按 enqueue 顺序执行，前一条完成才下一条。
  // 每条命令在出队执行时才读 this.state.snapshot.revision，避免并发命令拿到同一个 expectedRevision。
  private commandQueue: SerialCommandQueue = new SerialCommandQueue()

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
  // open 不进命令队列：它是初始化，必须先完成才能 enqueue 编辑命令。
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

  /**
   * 串行执行一条编辑命令。
   * enqueue 一个 thunk 到 commandQueue；thunk 在出队执行时才：
   *   1. 读取 this.state.snapshot.revision 作为 expectedRevision
   *   2. 调用 bridge（传入 sid 和当时的 revision）
   *   3. 用 EditorSessionState.withEditResult 应用结果
   *   4. ok=true → 更新 state + notifyListener
   *      ok=false（patch 失败 / StaleRevision / InvalidOffset / InvalidRange）→ 立即 bridge.snapshot 重建 state + notifyListener
   * 前一条命令完成并更新 state 后，下一条才能开始。
   */
  private enqueueEdit(
    bridgeCall: (sessionId: number, expectedRevision: number) => Promise<ResultEnvelope<EditorEditResult>>
  ): Promise<ResultEnvelope<EditorEditResult>> {
    return this.commandQueue.enqueue<ResultEnvelope<EditorEditResult>>(async () => {
      if (!this.state) {
        return this.noSession<EditorEditResult>()
      }
      const sessionId = this.state.sessionId
      // 出队时才读 revision — 这是串行队列的核心：避免并发命令拿到同一个 expectedRevision
      const expectedRevision = this.state.snapshot.revision
      const result = await bridgeCall(sessionId, expectedRevision)
      if (result.success && result.data) {
        const outcome: EditStateOutcome = this.state.withEditResult(result.data)
        if (outcome.ok) {
          this.state = outcome.state
          this.notifyListener()
        } else {
          // patch 应用失败或 StaleRevision/InvalidOffset/InvalidRange：
          // 立即从 Core snapshot() 重建 state，不让 UI 停在旧文本。
          await this.recoverFromCoreSnapshot()
        }
      }
      return result
    })
  }

  /**
   * 从 Core snapshot() 重建 state：用于 patch 失败或 StaleRevision 恢复。
   * 拿到 Core 完整 snapshot 后替换 this.state 并 notifyListener。
   * snapshot 调用本身失败时无法恢复，保留旧 state（best effort）；不吞错误，记入 warnings 不阻塞后续命令。
   */
  private async recoverFromCoreSnapshot(): Promise<void> {
    if (!this.state) {
      return
    }
    const sid = this.state.sessionId
    const snapResult = await this.bridge.snapshot(sid)
    if (snapResult.success && snapResult.data) {
      this.state = EditorSessionState.fromCreate(sid, snapResult.data)
      this.notifyListener()
    }
    // snapResult 失败时无法恢复；保留旧 state。不抛错（避免阻塞命令队列后续命令），
    // 但调用方通过 result.success/data.outcome 已知本次命令失败。
  }

  async insert(byteOffset: number, text: string, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.insert(sid, byteOffset, text, cause, rev))
  }

  async delete(byteOffsetStart: number, byteOffsetEnd: number, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.delete(sid, byteOffsetStart, byteOffsetEnd, cause, rev))
  }

  async replace(byteOffsetStart: number, byteOffsetEnd: number, text: string, originalText: string, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.replace(sid, byteOffsetStart, byteOffsetEnd, text, originalText, cause, rev))
  }

  async setSelection(anchorByteOffset: number, headByteOffset: number): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.setSelection(sid, anchorByteOffset, headByteOffset, rev))
  }

  async undo(): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.undo(sid, rev))
  }

  async redo(): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.redo(sid, rev))
  }

  async commitText(byteStart: number, byteEndExclusive: number, replacementText: string, resultingSelectionAnchor: number, resultingSelectionHead: number, compositionSessionId: number, compositionBaseRevision: number, compositionGeneration: number, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.commitText(sid, byteStart, byteEndExclusive, replacementText, resultingSelectionAnchor, resultingSelectionHead, compositionSessionId, compositionBaseRevision, compositionGeneration, cause, rev))
  }

  async deleteSurrounding(beforeByteStart: number, beforeByteEndExclusive: number, afterByteStart: number, afterByteEndExclusive: number, cause: EditorTransactionCause): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.deleteSurrounding(sid, beforeByteStart, beforeByteEndExclusive, afterByteStart, afterByteEndExclusive, cause, rev))
  }

  async beginComposition(replaceStart: number, replaceEndExclusive: number): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.beginComposition(sid, replaceStart, replaceEndExclusive, rev))
  }

  async updateComposition(compositionSessionId: number, compositionGeneration: number, newPreeditText: string, newPreeditCursorOffset: number): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.updateComposition(sid, compositionSessionId, compositionGeneration, newPreeditText, newPreeditCursorOffset, rev))
  }

  async finishComposition(compositionSessionId: number, compositionGeneration: number): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.finishComposition(sid, compositionSessionId, compositionGeneration, rev))
  }

  async cancelComposition(compositionSessionId: number, compositionGeneration: number): Promise<ResultEnvelope<EditorEditResult>> {
    return this.enqueueEdit((sid, rev) => this.bridge.cancelComposition(sid, compositionSessionId, compositionGeneration, rev))
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

  // 队列状态（供测试/诊断；生产代码一般不查）。
  queueSize(): number {
    return this.commandQueue.size()
  }

  queueIsIdle(): boolean {
    return this.commandQueue.isIdle()
  }

  private noSession<T>(): ResultEnvelope<T> {
    return { success: false, errorCode: 'NO_SESSION', warnings: [], changedPaths: [], changedEntities: [] }
  }
}
