// EditorSessionState — 编辑会话不可变状态值对象。
// 持有 sessionId（来自 Core create 返回的 u64）+ 最后一次 EditorSessionSnapshot。
// #629 评论3：sessionId 来自 create 的独立 u64，不在 snapshot 内；
// 编辑命令返回 EditorEditResult，通过 withEditResult 应用 displayPatches 增量更新。
//
// 纯逻辑（applyEditResultToSnapshot / SerialCommandQueue）提取到 editor_patch_logic.ts，
// 供 Node --experimental-strip-types 直接 import 单测；本 .ets 文件只保留薄壳。
//
// Issue #629 评论 5 第 3 部分：withEditResult 不吞 patch 失败。
// 成功返回 { ok: true, state }；失败返回 { ok: false, reason }。
// 调用方（Coordinator）收到 ok=false 时必须从 Core snapshot() 重建 state，不吞失败。

import { EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import { applyEditResultToSnapshot, ApplyEditResultOutcome } from './editor_patch_logic'

/**
 * withEditResult 的结果：明确区分成功与失败，不吞失败。
 * - ok=true：state 是应用 patch 后的新状态
 * - ok=false：reason 说明失败原因（与 ApplyEditResultOutcome.reason 一致）
 *   调用方（Coordinator）收到 ok=false 时必须从 Core snapshot() 重建 state。
 */
export type EditStateOutcome =
  | { readonly ok: true; readonly state: EditorSessionState }
  | { readonly ok: false; readonly reason: string }

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
  // 成功返回 { ok: true, state }；失败返回 { ok: false, reason }。
  // 调用方（Coordinator）收到 ok=false 时必须从 Core snapshot() 重建 state，不吞失败。
  withEditResult(result: EditorEditResult): EditStateOutcome {
    const outcome: ApplyEditResultOutcome = applyEditResultToSnapshot(this.snapshot, result)
    if (outcome.ok) {
      return { ok: true, state: new EditorSessionState(this.sessionId, outcome.snapshot) }
    }
    return { ok: false, reason: outcome.reason }
  }

  get revision(): number {
    return this.snapshot.revision
  }
}
