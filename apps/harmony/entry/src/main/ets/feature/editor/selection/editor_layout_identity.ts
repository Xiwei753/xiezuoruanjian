// editor_layout_identity.ts — 编辑器布局身份的纯逻辑定义。
//
// Issue #629 评论5357756359 第2项 + 评论21：把 EditorLayoutIdentity 和
// matchesEditorLayoutIdentity 提取到独立生产文件，供 LineNavigationResolver.ets、
// LineLayoutStore 和 Node 测试共同使用，消除测试中的复制实现。
// 不依赖 ArkUI / EditorSessionSnapshot，只依赖 number/string。
//
// 布局身份 = 编辑状态身份：
// - revision + generation + compositionGeneration + compositionSessionId 精确对应 Core 编辑状态；
// - displayText 确保显示文本一致。
// compositionSessionId 避免 session A 的旧布局被误认成 session B。

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
 * null 输入返回 false（防御式：等待者可能持有 null state）。
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
