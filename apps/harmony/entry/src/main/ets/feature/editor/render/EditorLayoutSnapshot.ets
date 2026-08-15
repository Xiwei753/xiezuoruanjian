// EditorLayoutSnapshot — 编辑器静态布局值对象。
// 输入层 / session 层产出本对象，render 层消费。动画层以后新增 motion 消费同一份 snapshot。

export class EditorLayoutSnapshot {
  readonly text: string
  readonly selectionAnchor: number  // byte offset
  readonly selectionHead: number    // byte offset
  readonly compositionStart: number | null  // byte offset
  readonly compositionEnd: number | null      // byte offset
  readonly cursorByteOffset: number  // byte offset
  readonly revision: number

  constructor(
    text: string,
    selectionAnchor: number,
    selectionHead: number,
    compositionStart: number | null,
    compositionEnd: number | null,
    cursorByteOffset: number,
    revision: number
  ) {
    this.text = text
    this.selectionAnchor = selectionAnchor
    this.selectionHead = selectionHead
    this.compositionStart = compositionStart
    this.compositionEnd = compositionEnd
    this.cursorByteOffset = cursorByteOffset
    this.revision = revision
  }

  static fromEditorSnapshot(snap: {
    text: string
    selectionAnchor: number
    selectionHead: number
    compositionStart?: number
    compositionEnd?: number
    revision: number
  }): EditorLayoutSnapshot {
    return new EditorLayoutSnapshot(
      snap.text,
      snap.selectionAnchor,
      snap.selectionHead,
      snap.compositionStart ?? null,
      snap.compositionEnd ?? null,
      snap.selectionHead,
      snap.revision
    )
  }
}
