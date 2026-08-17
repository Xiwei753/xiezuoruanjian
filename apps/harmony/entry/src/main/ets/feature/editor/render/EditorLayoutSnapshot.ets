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
  // Issue #629 评论14 第3项：contentWidth 用于行导航 resolver 身份匹配。
  // SujianEditor 在 onAreaChange 中更新 contentWidth，刷新行布局时传给 LayoutSnapshot。
  // 行导航命令出队时用此值判断布局版本是否匹配当前 Core snapshot。
  readonly contentWidth: number

  constructor(
    text: string,
    selectionAnchor: number,
    selectionHead: number,
    compositionStart: number | null,
    compositionEnd: number | null,
    cursorByteOffset: number,
    revision: number,
    fontSize: number = 16,
    contentWidth: number = 0
  ) {
    this.text = text
    this.selectionAnchor = selectionAnchor
    this.selectionHead = selectionHead
    this.compositionStart = compositionStart
    this.compositionEnd = compositionEnd
    this.cursorByteOffset = cursorByteOffset
    this.revision = revision
    this.fontSize = fontSize
    this.contentWidth = contentWidth
  }

  // 从 Core EditorSessionSnapshot 构造布局快照。
  // selectionHead = snap.cursor, cursorByteOffset = snap.cursor。
  // 有 composition 时：text = 临时显示文本（committed text 的
  //   [replaceByteStart, replaceByteEndExclusive) 替换为 preeditText），
  //   compositionStart/End = preedit 在显示文本中的 UTF-8 byte 范围。
  // 无 composition 时：text = snap.text（committed text），compositionStart/End = null。
  // 保存正文仍只取 snap.text（committed text），不把 preedit 写进文件。
  static fromEditorSnapshot(snap: EditorSessionSnapshot, fontSize?: number, contentWidth?: number): EditorLayoutSnapshot {
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
        fontSize ?? 16,
        contentWidth ?? 0
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
    // Issue #629 评论7 第3项：composition 活跃时显示光标/选区按 preedit cursor 投影到 displayText。
    // preeditCursorUtf16（IME 协议要求的 preedit 内 UTF-16 cursor）转成 preedit 内 UTF-8 byte offset，
    // 再加 replaceByteStart 得 displayCaretByte（displayText 中的 byte offset）。
    // selection/cursor 全部 collapse 到 displayCaretByte，不用 committed text 的 snap.cursor（不是 displayText 坐标）。
    const preeditCursorByte: number = TextOffsetMapper.utf16ToUtf8(
      composition.preeditText, composition.preeditCursorUtf16)
    const displayCaretByte: number = composition.replaceByteStart + preeditCursorByte
    return new EditorLayoutSnapshot(
      displayText,
      displayCaretByte,
      displayCaretByte,
      compositionStart,
      compositionEnd,
      displayCaretByte,
      snap.revision,
      fontSize ?? 16,
      contentWidth ?? 0
    )
  }
}