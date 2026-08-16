// editor_patch_logic.ts — 编辑会话 patch 应用的纯逻辑模块。
// 不依赖 ArkUI，只依赖 string/number/Array。
// 生产由 EditorSessionState.ets 调用；测试由 Node 单测直接 import。
//
// 与 Core api/types/editor.rs 真实契约对齐：字段名严格 camelCase
// （Core serde rename_all = "camelCase"）。

/** DisplayPatch 形状（与 Core DTO 对齐，camelCase）。 */
export interface DisplayPatch {
  readonly baseRevision: number
  readonly newRevision: number
  readonly replaceByteStart: number
  readonly replaceByteEndExclusive: number
  readonly insertedText: string
  readonly resultingSelectionStart: number
  readonly resultingSelectionEnd: number
}

/** CompositionSession 形状。 */
export interface CompositionSession {
  readonly sessionId: number
  readonly baseRevision: number
  readonly generation: number
}

/** EditorContentDelta 形状。 */
export interface EditorContentDelta {
  readonly insertedChars: number
  readonly deletedChars: number
  readonly insertedNonWhitespaceChars: number
  readonly deletedNonWhitespaceChars: number
}

/** EditorEditResult 形状（与 Core DTO 对齐，camelCase）。 */
export interface EditorEditResult {
  readonly outcome: string
  readonly transactionId: number
  readonly baseRevision: number
  readonly newRevision: number
  readonly displayPatches: DisplayPatch[]
  readonly oldSelectionStart: number
  readonly oldSelectionEnd: number
  readonly newSelectionStart: number
  readonly newSelectionEnd: number
  readonly visualIntent: Record<string, unknown>
  readonly compositionSession: CompositionSession | null
  readonly contentDelta: EditorContentDelta
}

/** EditorSessionSnapshot 形状。 */
export interface EditorSessionSnapshot {
  readonly text: string
  readonly revision: number
  readonly cursor: number
  readonly selectionAnchor: number
  readonly generation: number
  readonly chapterId: string
}

/** 编辑命令结果枚举值。与 Rust EditorEditOutcome 变体字符串对齐。 */
export const APPLIED = 'applied'
export const APPLIED_WITH_ADJUSTED_SELECTION = 'appliedWithAdjustedSelection'
export const NO_CHANGE = 'noChange'
export const STALE_REVISION = 'staleRevision'
export const INVALID_OFFSET = 'invalidOffset'
export const INVALID_RANGE = 'invalidRange'

/**
 * UTF-8 byte offset → UTF-16 code unit offset（严格版）。
 * 逐 Unicode code point 累加 UTF-8 byte 长度和 UTF-16 code unit 长度。
 * byteOffset 必须恰好落在字符边界（某个 code point 的开始处）或等于文本总 byte 长度（末尾）。
 * 非字符边界（落在某个 code point 中间）或越界时返回 -1。
 *
 * 与 TextOffsetMapper.utf8ToUtf16 的区别：后者在非边界时静默截断到前一个边界，
 * 本函数严格返回 -1，让调用方明确处理失败（从 Core snapshot() 恢复）。
 */
export function utf8ByteOffsetToUtf16(text: string, byteOffset: number): number {
  if (byteOffset < 0) return -1
  if (byteOffset === 0) return 0
  let byteLen = 0
  let utf16Index = 0
  let i = 0
  while (i < text.length) {
    const code = text.charCodeAt(i)
    let charByteLen: number
    let utf16Step: number
    if (code < 0x80) {
      charByteLen = 1
      utf16Step = 1
    } else if (code < 0x800) {
      charByteLen = 2
      utf16Step = 1
    } else if (code >= 0xD800 && code <= 0xDBFF) {
      // 高代理项：UTF-16 surrogate pair 对应 1 个 code point，UTF-8 4 字节，UTF-16 2 code unit。
      charByteLen = 4
      utf16Step = 2
      i += 1  // 跳过低代理项
    } else {
      charByteLen = 3
      utf16Step = 1
    }
    byteLen += charByteLen
    utf16Index += utf16Step
    i += 1
    if (byteLen === byteOffset) {
      return utf16Index
    }
    if (byteLen > byteOffset) {
      // 落在当前 code point 中间，非字符边界
      return -1
    }
  }
  // 遍历完所有 code point 仍未达到 byteOffset → 越界
  return -1
}

/**
 * 严格应用单个 DisplayPatch：把 text[replaceByteStart, replaceByteEndExclusive)（UTF-8 byte offset）
 * 替换为 insertedText。byte offset 先转成 UTF-16 code unit offset 再 substring。
 *
 * 成功返回 { ok: true, text }；失败（非字符边界或越界）返回 { ok: false, reason }，
 * 让上层从 Core snapshot() 恢复，不静默截断。
 */
export function applyPatchStrict(
  text: string,
  patch: DisplayPatch
): { ok: true, text: string } | { ok: false, reason: string } {
  const startUtf16 = utf8ByteOffsetToUtf16(text, patch.replaceByteStart)
  if (startUtf16 < 0) {
    return {
      ok: false,
      reason: `replaceByteStart=${patch.replaceByteStart} 不是字符边界或越界`,
    }
  }
  const endUtf16 = utf8ByteOffsetToUtf16(text, patch.replaceByteEndExclusive)
  if (endUtf16 < 0) {
    return {
      ok: false,
      reason: `replaceByteEndExclusive=${patch.replaceByteEndExclusive} 不是字符边界或越界`,
    }
  }
  if (endUtf16 < startUtf16) {
    return {
      ok: false,
      reason: `转换后 endUtf16=${endUtf16} < startUtf16=${startUtf16}`,
    }
  }
  const before = text.substring(0, startUtf16)
  const after = text.substring(endUtf16)
  return { ok: true, text: before + patch.insertedText + after }
}

/**
 * @deprecated 旧版 applyPatch 直接把 UTF-8 byte offset 当 UTF-16 code unit offset 用，
 * 中文/emoji 会错位。新代码应使用 applyPatchStrict。保留导出以兼容现有调用方。
 * ASCII 文本下 byte offset === utf16 offset，结果与 applyPatchStrict 一致。
 */
export function applyPatch(text: string, patch: DisplayPatch): string {
  const before: string = text.substring(0, patch.replaceByteStart)
  const after: string = text.substring(patch.replaceByteEndExclusive)
  return before + patch.insertedText + after
}

/**
 * 从 EditorEditResult 增量更新 snapshot。
 * outcome 为 staleRevision/invalidOffset/invalidRange 时不更新（返回原 snapshot）。
 * 应用 displayPatches 到 text，更新 revision/cursor/selectionAnchor/generation。
 * compositionSession 非空时取其 generation；为空时保留原 snapshot.generation
 * （finishComposition 后 compositionSession=null，generation 不重置）。
 *
 * patch 应用失败（非字符边界或越界）时返回原 snapshot，
 * 让上层（EditorSessionState/Coordinator）检测到 text 未变化后从 Core snapshot() 恢复。
 * 多个 DisplayPatch 按顺序应用，每个 patch 都针对当时的当前文本转换 offset。
 */
export function applyEditResultToSnapshot(
  snapshot: EditorSessionSnapshot,
  result: EditorEditResult
): EditorSessionSnapshot {
  if (result.outcome !== APPLIED
    && result.outcome !== APPLIED_WITH_ADJUSTED_SELECTION
    && result.outcome !== NO_CHANGE) {
    return snapshot
  }
  let newText: string = snapshot.text
  const patches: DisplayPatch[] = result.displayPatches ?? []
  for (let i = 0; i < patches.length; i++) {
    const applied = applyPatchStrict(newText, patches[i])
    if (!applied.ok) {
      // patch 应用失败（非字符边界或越界）：返回原 snapshot，
      // 让上层从 Core snapshot() 恢复，不静默截断。
      return snapshot
    }
    newText = applied.text
  }
  return {
    text: newText,
    revision: result.newRevision,
    cursor: result.newSelectionEnd,
    selectionAnchor: result.newSelectionStart,
    generation: result.compositionSession ? result.compositionSession.generation : snapshot.generation,
    chapterId: snapshot.chapterId
  }
}
