// IEditorSessionBridge — 编辑会话与 Core 之间的接口契约。
// byte offset 始终是 UTF-8 byte offset（Core 用 byte offset）。
// ArkTS 端通过 TextOffsetMapper 做 UTF-16 ↔ UTF-8 转换后再调用本接口。

import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'

export interface EditorSnapshot {
  sessionId: string
  revision: number
  text: string
  selectionAnchor: number  // byte offset
  selectionHead: number    // byte offset
  compositionStart?: number  // byte offset
  compositionEnd?: number    // byte offset
}

export interface IEditorSessionBridge {
  create(initialText: string): Promise<ResultEnvelope<EditorSnapshot>>
  close(sessionId: string): Promise<ResultEnvelope<boolean>>
  snapshot(sessionId: string): Promise<ResultEnvelope<EditorSnapshot>>
  insert(sessionId: string, byteOffset: number, text: string): Promise<ResultEnvelope<EditorSnapshot>>
  delete(sessionId: string, byteOffsetStart: number, byteOffsetEnd: number): Promise<ResultEnvelope<EditorSnapshot>>
  replace(sessionId: string, byteOffsetStart: number, byteOffsetEnd: number, text: string): Promise<ResultEnvelope<EditorSnapshot>>
  setSelection(sessionId: string, anchorByteOffset: number, headByteOffset: number): Promise<ResultEnvelope<EditorSnapshot>>
  undo(sessionId: string): Promise<ResultEnvelope<EditorSnapshot>>
  redo(sessionId: string): Promise<ResultEnvelope<EditorSnapshot>>
  // IME composition
  commitText(sessionId: string, byteOffset: number, text: string): Promise<ResultEnvelope<EditorSnapshot>>
  deleteSurrounding(sessionId: string, beforeByteLen: number, afterByteLen: number): Promise<ResultEnvelope<EditorSnapshot>>
  beginComposition(sessionId: string, byteOffset: number): Promise<ResultEnvelope<EditorSnapshot>>
  updateComposition(sessionId: string, preedit: string): Promise<ResultEnvelope<EditorSnapshot>>
  finishComposition(sessionId: string, committed: string): Promise<ResultEnvelope<EditorSnapshot>>
  cancelComposition(sessionId: string): Promise<ResultEnvelope<EditorSnapshot>>
  // 查询
  getText(sessionId: string): Promise<ResultEnvelope<string>>
  getRevision(sessionId: string): Promise<ResultEnvelope<number>>
}
