// editor_layout_identity — 布局身份的纯逻辑定义。
// Issue #629 评论5357756359 第2项：把 EditorLayoutIdentity 和 matchesEditorLayoutIdentity
// 提取到独立生产文件，LineNavigationResolver.ets 和测试都复用同一份 helper，
// 消除测试中的复制实现。
//
// 布局身份 = 编辑状态身份：
// - revision + generation + compositionGeneration + compositionSessionId 精确对应 Core 编辑状态；
// - displayText 确保显示文本一致。
// compositionSessionId 避免 session A 的旧布局被误认成 session B。

export interface EditorLayoutIdentity {
  readonly revision: number
  readonly generation: number
  readonly compositionGeneration: number
  readonly compositionSessionId: number
  readonly displayText: string
}

export function matchesEditorLayoutIdentity(
  state: EditorLayoutIdentity,
  expected: EditorLayoutIdentity,
): boolean {
  return state.revision === expected.revision
    && state.generation === expected.generation
    && state.compositionGeneration === expected.compositionGeneration
    && state.compositionSessionId === expected.compositionSessionId
    && state.displayText === expected.displayText
}
