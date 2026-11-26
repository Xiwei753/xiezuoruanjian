// Issue #1063：编辑会话冲突材料 DTO。
// 这条入口只写 Core 既有冲突材料（base.md/local.md/incoming.md/editor-session.json），
// 不发布正文、不做版本 CAS —— 冲突状态机自己决定何时把哪份正文交给 Core。
// 字段与 Core `EditorConflictMaterialRequestDto` / 回执 camelCase 对齐。

import { ChapterVersion } from './ProjectDtos'
import { DecodeResult } from '../codec/CoreWireDecoders'

// 发往 Core 的冲突材料。projectId/volumeId 由桥接层从 chapterId 解析后补上，
// 页面只关心 chapterId 与双方正文。
export interface EditorConflictMaterialRequest {
  chapterId: string
  // null = 无基线（Core 不写 base.md）；'' = 合法空基线（Core 会写空的 base.md）。
  baseContent: string | null
  // Core 侧最终正文 → local.md。
  coreContent: string
  // 平台侧待提交正文 → incoming.md。
  nativeContent: string
  coreVersion: ChapterVersion | null
  localVersion: ChapterVersion | null
  coreSessionId: number | null
  localSessionId: number | null
  // 冲突时刻的版本/事务序号（均可为 null，Core 原样写进 editor-session.json 供诊断）。
  coreRevision: number | null
  baseRevision: number | null
  changeSequence: number | null
  detail: string
}

// Core 回执：事务 id + 冲突目录 + 双方内容 sha256（供状态机校验写盘一致）。
export interface EditorConflictMaterialReceipt {
  transactionId: string
  conflictDir: string
  coreContentSha256: string
  nativeContentSha256: string
}

function err<T>(message: string): DecodeResult<T> {
  return { ok: false, error: message }
}

function ok<T>(value: T): DecodeResult<T> {
  return { ok: true, value: value }
}

// 显式逐字段解码：字段缺失或类型不符立即返回明确错误，不靠 `as T` 强转
//（强转后 undefined 会流到 UI，沿用 CoreWireDecoders 的既有 decoder 约定）。
// 参数取 Object 以便直接作为 `decodeEnvelope(json, decodeEditorConflictMaterialReceipt)` 的 decoder。
export function decodeEditorConflictMaterialReceipt(data: Object): DecodeResult<EditorConflictMaterialReceipt> {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return err<EditorConflictMaterialReceipt>('EditorConflictMaterialReceipt: expected object')
  }
  const rec = data as Record<string, Object>

  const transactionId: Object | undefined = rec['transactionId']
  if (typeof transactionId !== 'string' || transactionId.length === 0) {
    return err<EditorConflictMaterialReceipt>('transactionId missing or not string')
  }
  const conflictDir: Object | undefined = rec['conflictDir']
  if (typeof conflictDir !== 'string' || conflictDir.length === 0) {
    return err<EditorConflictMaterialReceipt>('conflictDir missing or not string')
  }
  const coreContentSha256: Object | undefined = rec['coreContentSha256']
  if (typeof coreContentSha256 !== 'string' || coreContentSha256.length === 0) {
    return err<EditorConflictMaterialReceipt>('coreContentSha256 missing or not string')
  }
  const nativeContentSha256: Object | undefined = rec['nativeContentSha256']
  if (typeof nativeContentSha256 !== 'string' || nativeContentSha256.length === 0) {
    return err<EditorConflictMaterialReceipt>('nativeContentSha256 missing or not string')
  }

  const value: EditorConflictMaterialReceipt = {
    transactionId: transactionId,
    conflictDir: conflictDir,
    coreContentSha256: coreContentSha256,
    nativeContentSha256: nativeContentSha256
  }
  return ok<EditorConflictMaterialReceipt>(value)
}
