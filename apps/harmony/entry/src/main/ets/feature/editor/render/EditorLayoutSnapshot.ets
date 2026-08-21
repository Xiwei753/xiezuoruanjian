// EditorLayoutSnapshot — 编辑器静态布局值对象。
// 输入层 / session 层产出本对象，render 层消费。动画层以后新增 motion 消费同一份 snapshot。
// Issue #629 评论 3：fromEditorSnapshot 接受 EditorSessionSnapshot（cursor 而非 selectionHead）。
// Issue #629 评论6 Part B：fromEditorSnapshot 接受 composition 参数，有 composition 时
// 构造临时显示文本（committed text 的 [replaceByteStart, replaceByteEndExclusive) 替换为 preeditText），
// compositionStart/End 设为 preedit 在显示文本中的 UTF-8 byte 范围。
// 保存正文仍只取 committed text（snap.text），不把 preedit 写进文件。
//
// Issue #629 评论15 第2项：contentWidth 不属于 Core editor snapshot 的编辑状态身份。
// 布局尺寸（contentWidth）属于 Harmony render/selection，不属于 Core editor snapshot。
// 改为 Core 已有的 generation + compositionGeneration 作为编辑状态身份字段。
//
// Issue #629 评论5358224312 第1项：display projection 算法提取到 editor_display_projection.ts，
// fromEditorSnapshot() 调用 projectEditorDisplay()，不保留第二份 projection 算法。
// 测试也直接 import projectEditorDisplay，保证生产/测试走同一份算法。

import { EditorSessionSnapshot } from '../../../corebridge/dto/EditorDtos'
import { projectEditorDisplay } from './editor_display_projection'

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
  // Issue #629 评论15 第2项：编辑状态身份字段。
  // generation + compositionGeneration 由 Core 维护，精确标识哪一次编辑状态。
  // 行导航 resolver 用这些字段（加上 displayText）作为 layout identity，
  // 而不是用 contentWidth（布局尺寸属于 Harmony render，不属于 Core 编辑状态）。
  readonly generation: number
  readonly compositionGeneration: number
  // Issue #629 R11 评论5329310563 第1项：compositionSessionId 由 Core 分配，
  // 每次 BeginComposition 都会重新编号。普通 begin composition 不会改 committed revision，
  // 所以 revision + generation + compositionGeneration 可能把 session A 的旧布局误认成 session B。
  // 必须把 compositionSessionId 一起加入 layout identity。
  readonly compositionSessionId: number

  constructor(
    text: string,
    selectionAnchor: number,
    selectionHead: number,
    compositionStart: number | null,
    compositionEnd: number | null,
    cursorByteOffset: number,
    revision: number,
    fontSize: number = 16,
    generation: number = 0,
    compositionGeneration: number = -1,
    compositionSessionId: number = 0
  ) {
    this.text = text
    this.selectionAnchor = selectionAnchor
    this.selectionHead = selectionHead
    this.compositionStart = compositionStart
    this.compositionEnd = compositionEnd
    this.cursorByteOffset = cursorByteOffset
    this.revision = revision
    this.fontSize = fontSize
    this.generation = generation
    this.compositionGeneration = compositionGeneration
    this.compositionSessionId = compositionSessionId
  }

  // 从 Core EditorSessionSnapshot 构造布局快照。
  // selectionHead = projection.selectionHeadByteOffset, cursorByteOffset = projection.cursorByteOffset。
  // 有 composition 时：text = projection.text（临时显示文本，committed text 的
  //   [replaceByteStart, replaceByteEndExclusive) 替换为 preeditText），
  //   compositionStart/End = projection.compositionStart/EndByteOffset（preedit 在显示文本中的 UTF-8 byte 范围）。
  // 无 composition 时：text = projection.text（committed text），compositionStart/End = null。
  // 保存正文仍只取 snap.text（committed text），不把 preedit 写进文件。
  // Issue #629 评论15 第2项：不再接受 contentWidth 参数；用 generation/compositionGeneration 标识编辑状态。
  // Issue #629 评论5358224312 第1项：display projection 由 projectEditorDisplay 统一计算，此处只做字段映射。
  static fromEditorSnapshot(snap: EditorSessionSnapshot, fontSize?: number): EditorLayoutSnapshot {
    const projection = projectEditorDisplay(snap)
    const hasComposition: boolean = snap.composition !== null && snap.composition !== undefined
    return new EditorLayoutSnapshot(
      projection.text,
      projection.selectionAnchorByteOffset,
      projection.selectionHeadByteOffset,
      projection.compositionStartByteOffset,
      projection.compositionEndByteOffset,
      projection.cursorByteOffset,
      snap.revision,
      fontSize ?? 16,
      snap.generation ?? 0,
      hasComposition ? (snap.composition!.generation ?? 0) : -1,
      hasComposition ? snap.composition!.sessionId : 0
    )
  }
}
