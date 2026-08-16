// EditorLayoutSnapshot — 编辑器静态布局值对象。
// 输入层 / session 层产出本对象，render 层消费。动画层以后新增 motion 消费同一份 snapshot。
// Issue #629 评论 3：fromEditorSnapshot 接受 EditorSessionSnapshot（cursor 而非 selectionHead）。
// Issue #629 评论6 Part B：fromEditorSnapshot 接受 composition 参数，有 composition 时
// 构造临时显示文本（committed text 的 [replaceByteStart, replaceByteEndExclusive) 替换为 preeditText），
// compositionStart/End 设为 preedit 在显示文本中的 UTF-8 byte 范围。
// 保存正文仍只取 committed text（snap.text），不把 preedit 写进文件。

import { EditorSessionSnapshot, EditorCompositionState } from '../../../corebridge/dto/EditorDtos'
import { TextOffsetMapper } from '../input/TextOffsetMapper'

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
  // 有 composition 时：text = 临时显示文本（committed text 的
  //   [replaceByteStart, replaceByteEndExclusive) 替换为 preeditText），
  //   compositionStart/End = preedit 在显示文本中的 UTF-8 byte 范围。
  // 无 composition 时：text = snap.text（committed text），compositionStart/End = null。
  // 保存正文仍只取 snap.text（committed text），不把 preedit 写进文件。
  static fromEditorSnapshot(snap: EditorSessionSnapshot, fontSize?: number): EditorLayoutSnapshot {
    const composition: EditorCompositionState | null = snap.composition
    if (composition === null || composition === undefined) {
      // 无 composition：显示文本 = committed text，compositionStart/End = null。
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
    // 有 composition：构造临时显示文本。
    // 把 committed text 的 [replaceByteStart, replaceByteEndExclusive)（UTF-8 byte offset）
    // 替换为 preeditText。byte offset 先转成 UTF-16 code unit offset 再 substring。
    const committedText: string = snap.text
    const startUtf16: number = TextOffsetMapper.utf8ToUtf16(committedText, composition.replaceByteStart)
    const endUtf16: number = TextOffsetMapper.utf8ToUtf16(committedText, composition.replaceByteEndExclusive)
    const before: string = committedText.substring(0, startUtf16)
    const after: string = committedText.substring(endUtf16)
    const displayText: string = before + composition.preeditText + after
    // compositionStart/End 是 preedit 在显示文本中的 UTF-8 byte 范围。
    // before 部分的 UTF-8 byte 长度 = replaceByteStart（committed text 的前缀不变）。
    // preeditText 的 UTF-8 byte 长度用 TextEncoder 计算。
    const compositionStart: number = composition.replaceByteStart
    const preeditUtf8Len: number = new TextEncoder().encode(composition.preeditText).length
    const compositionEnd: number = composition.replaceByteStart + preeditUtf8Len
    return new EditorLayoutSnapshot(
      displayText,
      snap.selectionAnchor,
      snap.cursor,
      compositionStart,
      compositionEnd,
      snap.cursor,
      snap.revision,
      fontSize ?? 16
    )
  }
}