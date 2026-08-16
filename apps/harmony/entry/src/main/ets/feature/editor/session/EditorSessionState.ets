// EditorSessionState — 编辑会话不可变状态值对象。
// 持有 sessionId（来自 Core create 返回的 u64）+ 最后一次 EditorSessionSnapshot。
// #629 评论3：sessionId 来自 create 的独立 u64，不在 snapshot 内；
// 编辑命令返回 EditorEditResult，通过 withEditResult 应用 displayPatches 增量更新。
//
// 纯逻辑（applyPatch / applyEditResultToSnapshot）提取到 editor_patch_logic.ts，
// 供 Node --experimental-strip-types 直接 import 单测；本 .ets 文件只保留薄壳。

import { EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { applyEditResultToSnapshot } from './editor_patch_logic'

export class EditorSessionState {
  readonly sessionId: number
  readonly snapshot: EditorSessionSnapshot

  constructor(sessionId: number, snapshot: EditorSessionSnapshot) {
    this.sessionId = sessionId
    this.snapshot = snapshot
  }

  static fromCreate(sessionId: number, snapshot: EditorSessionSnapshot): EditorSessionState {
    return new EditorSessionState(sessionId, snapshot)
  }

  // 从 EditorEditResult 增量更新：委托纯函数 applyEditResultToSnapshot。
  // outcome 为 staleRevision/invalidOffset/invalidRange 时不更新（返回 this）。
  withEditResult(result: EditorEditResult): EditorSessionState {
    return new EditorSessionState(this.sessionId, applyEditResultToSnapshot(this.snapshot, result))
  }

  get revision(): number {
    return this.snapshot.revision
  }
}
