// IEditorSessionBridge — 编辑会话与 Core 之间的接口契约。
// byte offset 始终是 UTF-8 byte offset（Core 用 byte offset）。
// ArkTS 端通过 TextOffsetMapper 做 UTF-16 ↔ UTF-8 转换后再调用本接口。
// sessionId / revision 是 Core u64，实际值 < 2^53，ArkTS 用 number。
// cause 传 EditorTransactionCause 枚举（字符串值与 Rust parse_cause 匹配）。
// Issue #629 评论 3：create→u64 sessionId（number），snapshot→EditorSessionSnapshot，
// 编辑命令→EditorEditResult。与 Core api/types/editor.rs 真实契约对齐。

import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause, EditorSessionSnapshot, EditorEditResult } from '../../../corebridge/dto/EditorDtos'

export interface IEditorSessionBridge {
  create(targetId: string, initialText: string, initialCursorByteOffset: number, isPersistent: boolean): Promise<ResultEnvelope<number>>
  close(sessionId: number): Promise<ResultEnvelope<boolean>>
  snapshot(sessionId: number): Promise<ResultEnvelope<EditorSessionSnapshot>>
  insert(sessionId: number, byteOffset: number, text: string, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  delete(sessionId: number, byteOffsetStart: number, byteOffsetEnd: number, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  replace(sessionId: number, byteOffsetStart: number, byteOffsetEnd: number, text: string, originalText: string, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  setSelection(sessionId: number, anchorByteOffset: number, headByteOffset: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  undo(sessionId: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  redo(sessionId: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  // IME composition
  commitText(sessionId: number, byteStart: number, byteEndExclusive: number, replacementText: string, resultingSelectionAnchor: number, resultingSelectionHead: number, compositionSessionId: number, compositionBaseRevision: number, compositionGeneration: number, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  deleteSurrounding(sessionId: number, beforeByteStart: number, beforeByteEndExclusive: number, afterByteStart: number, afterByteEndExclusive: number, cause: EditorTransactionCause, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  beginComposition(sessionId: number, replaceStart: number, replaceEndExclusive: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  updateComposition(sessionId: number, compositionSessionId: number, compositionGeneration: number, newPreeditText: string, newPreeditCursorOffset: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  finishComposition(sessionId: number, compositionSessionId: number, compositionGeneration: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  cancelComposition(sessionId: number, compositionSessionId: number, compositionGeneration: number, expectedRevision: number): Promise<ResultEnvelope<EditorEditResult>>
  // 查询
  getText(sessionId: number): Promise<ResultEnvelope<string>>
  getRevision(sessionId: number): Promise<ResultEnvelope<number>>
  // #606: grapheme cluster 边界查询（UTF-8 byte offset）。
  // 平台端 Left/Right/Backspace/Delete 通过此方法确定光标移动目标，
  // 不再自己用 UTF-16 ±1 猜字符边界（避免切进组合附加符 / ZWJ emoji）。
  previousGraphemeBoundary(sessionId: number, byteOffset: number): Promise<ResultEnvelope<number>>
  nextGraphemeBoundary(sessionId: number, byteOffset: number): Promise<ResultEnvelope<number>>
}
