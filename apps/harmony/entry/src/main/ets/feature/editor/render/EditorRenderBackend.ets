// EditorRenderBackend — 编辑器静态渲染后端。
// 输入只能是 EditorLayoutSnapshot。先做静态文本、选区、composition 下划线、光标和命中测试。
// 动画层以后新增 motion 消费同一份 snapshot，不改输入层和 session 层。

import { EditorLayoutSnapshot } from './EditorLayoutSnapshot'

export interface EditorRenderResult {
  text: string
  selectionStartUtf16: number
  selectionEndUtf16: number
  compositionStartUtf16: number | null
  compositionEndUtf16: number | null
  cursorUtf16: number
  revision: number
}

export class EditorRenderBackend {
  // 把 byte offset snapshot 转成 UTF-16 offset 渲染结果（ArkUI Text 用 UTF-16）。
  static render(snapshot: EditorLayoutSnapshot): EditorRenderResult {
    // 此处仅做 offset 语义转换占位；实际渲染由 SujianEditor 组件消费本结果。
    return {
      text: snapshot.text,
      selectionStartUtf16: snapshot.selectionAnchor,
      selectionEndUtf16: snapshot.selectionHead,
      compositionStartUtf16: snapshot.compositionStart,
      compositionEndUtf16: snapshot.compositionEnd,
      cursorUtf16: snapshot.cursorByteOffset,
      revision: snapshot.revision
    }
  }

  // 命中测试：给定 UTF-16 offset，返回是否在选区内。
  static hitTest(snapshot: EditorLayoutSnapshot, utf16Offset: number): 'before' | 'inside' | 'after' {
    const start = Math.min(snapshot.selectionAnchor, snapshot.selectionHead)
    const end = Math.max(snapshot.selectionAnchor, snapshot.selectionHead)
    if (utf16Offset < start) return 'before'
    if (utf16Offset > end) return 'after'
    return 'inside'
  }
}
