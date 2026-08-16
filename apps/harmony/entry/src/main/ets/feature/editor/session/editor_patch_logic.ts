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

/** 应用单个 DisplayPatch：把 text[replaceByteStart, replaceByteEndExclusive) 替换为 insertedText。 */
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
    newText = applyPatch(newText, patches[i])
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
