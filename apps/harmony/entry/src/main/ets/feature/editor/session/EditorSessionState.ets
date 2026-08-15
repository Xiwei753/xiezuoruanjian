// EditorSessionState — 编辑会话不可变快照值对象。
// 持有 sessionId + revision + snapshot。每次 Core 返回新 snapshot 时构造新实例。

import { EditorSnapshot } from '../interop/IEditorSessionBridge'

export class EditorSessionState {
  readonly sessionId: string
  readonly revision: number
  readonly snapshot: EditorSnapshot

  constructor(sessionId: string, revision: number, snapshot: EditorSnapshot) {
    this.sessionId = sessionId
    this.revision = revision
    this.snapshot = snapshot
  }

  static fromSnapshot(snapshot: EditorSnapshot): EditorSessionState {
    return new EditorSessionState(snapshot.sessionId, snapshot.revision, snapshot)
  }

  withSnapshot(newSnapshot: EditorSnapshot): EditorSessionState {
    return new EditorSessionState(newSnapshot.sessionId, newSnapshot.revision, newSnapshot)
  }
}
