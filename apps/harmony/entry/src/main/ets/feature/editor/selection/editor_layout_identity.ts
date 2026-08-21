// editor_layout_identity.ts — 编辑器布局身份的纯逻辑。
//
// Issue #629 评论21：把 EditorLayoutIdentity 和 matchesEditorLayoutIdentity
// 提到独立 .ts，供 LineNavigationResolver.ets、LineLayoutStore 和 Node 测试共同使用。
// 不依赖 ArkUI / EditorSessionSnapshot，只依赖 number/string。

/** 编辑器布局身份：精确对应 Core 编辑状态。 */
export interface EditorLayoutIdentity {
  readonly revision: number
  readonly generation: number
  readonly compositionGeneration: number
  readonly compositionSessionId: number
  readonly displayText: string
}

/**
 * 比较两个 EditorLayoutIdentity 是否匹配。
 * 5 字段全部相等才算匹配，包括 compositionSessionId。
 */
export function matchesEditorLayoutIdentity(
  state: EditorLayoutIdentity,
  expected: EditorLayoutIdentity,
): boolean {
  if (state === null || expected === null) {
    return false
  }
  return state.revision === expected.revision
    && state.generation === expected.generation
    && state.compositionGeneration === expected.compositionGeneration
    && state.compositionSessionId === expected.compositionSessionId
    && state.displayText === expected.displayText
}
