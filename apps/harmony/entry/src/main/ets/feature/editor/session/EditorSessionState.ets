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

import { EditorSessionSnapshot, EditorEditResult, EditorCompositionState } from '../../../corebridge/dto/EditorDtos'
import { applyEditResultToSnapshot, ApplyEditResultOutcome, EditorSessionSnapshot as PatchSnapshot } from './editor_patch_logic'

/**
 * withEditResult 的结果：明确区分成功与失败，不吞失败。
 * - ok=true：state 是应用 patch 后的新状态
 * - ok=false：reason 说明失败原因（与 ApplyEditResultOutcome.reason 一致）
 *   调用方（Coordinator）收到 ok=false 时必须从 Core snapshot() 重建 state。
 */
interface EditStateSuccess {
  readonly ok: true
  readonly state: EditorSessionState
}

interface EditStateFailure {
  readonly ok: false
  readonly reason: string
}

export type EditStateOutcome = EditStateSuccess | EditStateFailure

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
    // Issue #753：editor_patch_logic.EditorSessionSnapshot 与 EditorDtos.EditorSessionSnapshot
    // 字段一致，区别仅在 composition 可选性（DTO 可选 vs 纯逻辑 | null）。
    // 构造符合纯逻辑类型的桥接对象传入，把可选 composition 归一为 | null。
    const patchInput: PatchSnapshot = {
      text: this.snapshot.text,
      revision: this.snapshot.revision,
      cursor: this.snapshot.cursor,
      selectionAnchor: this.snapshot.selectionAnchor,
      generation: this.snapshot.generation,
      chapterId: this.snapshot.chapterId,
      composition: this.snapshot.composition ?? null
    }
    const outcome: ApplyEditResultOutcome = applyEditResultToSnapshot(patchInput, result as Object)
    if (outcome.ok === true) {
      return { ok: true, state: new EditorSessionState(this.sessionId, outcome.snapshot as EditorSessionSnapshot) }
    }
    if (outcome.ok === false) {
      return { ok: false, reason: outcome.reason }
    }
    // ApplyEditResultOutcome 是 ok:true | ok:false union，不会到这里；兜底保证返回类型完备。
    return { ok: false, reason: 'unknown outcome' }
  }

  get revision(): number {
    return this.snapshot.revision
  }

  // #629 评论6 Part B：当前 composition 完整状态。composition 活跃时非 null，
  // EditorLayoutSnapshot.fromEditorSnapshot 据此构造临时显示文本和下划线。
  // finish/cancel/普通编辑后为 null。状态由 withEditResult → applyEditResultToSnapshot
  // 透传 Core 返回的 result.composition，不在此处复制 composition 状态机。
  get composition(): EditorCompositionState | null {
    // Issue #753：DTO composition 是可选属性（undefined），归一为 null。
    return this.snapshot.composition ?? null
  }
}
