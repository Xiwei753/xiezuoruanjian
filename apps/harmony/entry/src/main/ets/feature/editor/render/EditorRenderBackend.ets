// EditorRenderBackend — 编辑器静态渲染后端。
// 输入只能是 EditorLayoutSnapshot。先做静态文本、选区、composition 下划线、光标和命中测试。
// 动画层以后新增 motion 消费同一份 snapshot，不改输入层和 session 层。

import { EditorLayoutSnapshot } from './EditorLayoutSnapshot'
import { TextOffsetMapper } from '../input/TextOffsetMapper'

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
  // EditorLayoutSnapshot 中 selectionAnchor/selectionHead/compositionStart/compositionEnd/cursorByteOffset
  // 均为 UTF-8 byte offset，必须经 TextOffsetMapper.utf8ToUtf16 转成 UTF-16 code unit offset 后再交给渲染层。
  static render(snapshot: EditorLayoutSnapshot): EditorRenderResult {
    const text = snapshot.text
    return {
      text,
      selectionStartUtf16: TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionAnchor),
      selectionEndUtf16: TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionHead),
      compositionStartUtf16: snapshot.compositionStart === null
        ? null
        : TextOffsetMapper.utf8ToUtf16(text, snapshot.compositionStart),
      compositionEndUtf16: snapshot.compositionEnd === null
        ? null
        : TextOffsetMapper.utf8ToUtf16(text, snapshot.compositionEnd),
      cursorUtf16: TextOffsetMapper.utf8ToUtf16(text, snapshot.cursorByteOffset),
      revision: snapshot.revision
    }
  }

  // 命中测试：给定 UTF-16 offset，返回是否在选区内。
  // snapshot 的 selectionAnchor/selectionHead 是 byte offset，先转成 UTF-16 offset 再与 utf16Offset 比较，
  // 避免混用两种 offset 语义导致中文等非 ASCII 文本命中判断错位。
  static hitTest(snapshot: EditorLayoutSnapshot, utf16Offset: number): 'before' | 'inside' | 'after' {
    const text = snapshot.text
    const anchorUtf16 = TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionAnchor)
    const headUtf16 = TextOffsetMapper.utf8ToUtf16(text, snapshot.selectionHead)
    const start = Math.min(anchorUtf16, headUtf16)
    const end = Math.max(anchorUtf16, headUtf16)
    if (utf16Offset < start) return 'before'
    if (utf16Offset > end) return 'after'
    return 'inside'
  }
}
