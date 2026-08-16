// EditorLayoutSnapshot — 编辑器静态布局值对象。
// 输入层 / session 层产出本对象，render 层消费。动画层以后新增 motion 消费同一份 snapshot。
// Issue #629 评论 3：fromEditorSnapshot 接受 EditorSessionSnapshot（cursor 而非 selectionHead）；
// compositionStart/End 从 snapshot 不读（设 null），静态编辑器不渲染 composition 高亮。

import { EditorSessionSnapshot } from '../../../corebridge/dto/EditorDtos'

export class EditorLayoutSnapshot {
  readonly text: string
  readonly selectionAnchor: number  // byte offset
  readonly selectionHead: number    // byte offset
  readonly compositionStart: number | null  // byte offset
  readonly compositionEnd: number | null      // byte offset
  readonly cursorByteOffset: number  // byte offset
  readonly revision: number
  // 字体大小（vp）。render 层做确定性命中测试时由 SujianEditor 传给 Text.fontSize
  // 和 getMeasureUtils().measureText，保证测量与渲染用同一字体度量。
  readonly fontSize: number

  constructor(
    text: string,
    selectionAnchor: number,
    selectionHead: number,
    compositionStart: number | null,
    compositionEnd: number | null,
    cursorByteOffset: number,
    revision: number,
    fontSize: number = 16
  ) {
    this.text = text
    this.selectionAnchor = selectionAnchor
    this.selectionHead = selectionHead
    this.compositionStart = compositionStart
    this.compositionEnd = compositionEnd
    this.cursorByteOffset = cursorByteOffset
    this.revision = revision
    this.fontSize = fontSize
  }

  // 从 Core EditorSessionSnapshot 构造布局快照。
  // selectionHead = snap.cursor, cursorByteOffset = snap.cursor。
  // compositionStart/End 设 null（静态编辑器不读 composition 范围）。
  static fromEditorSnapshot(snap: EditorSessionSnapshot, fontSize?: number): EditorLayoutSnapshot {
    return new EditorLayoutSnapshot(
      snap.text,
      snap.selectionAnchor,
      snap.cursor,
      null,
      null,
      snap.cursor,
      snap.revision,
      fontSize ?? 16
    )
  }
}
