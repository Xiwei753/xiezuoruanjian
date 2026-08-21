// editor_display_projection.ts — 编辑器 display projection 纯逻辑。
//
// Issue #629 评论5358224312 第1项：composition 连续序列必须走和生产代码同一份 display projection。
// 此文件是唯一的纯 display projection 算法事实来源：
//   - EditorLayoutSnapshot.ets fromEditorSnapshot() 调用 projectEditorDisplay
//   - editor_semantic_dispatcher.test.mjs 直接 import projectEditorDisplay
// 任何 composition display 投影都必须经此函数，禁止在调用方复制第二份算法。
//
// 约束：本文件是 .ts，必须能被 Node .mjs 测试直接 import，所以不能 import 任何 .ets 文件。
// 只依赖 string/number/TextEncoder（全局）+ 同目录 text_offset_mapper.ts 的 utf8ToUtf16/utf16ToUtf8。
//
// AGENTS.md 边界：Core 保存业务真相；本文件是纯投影，不含业务逻辑、不调 Core、不持有状态。

import { utf8ToUtf16, utf16ToUtf8 } from '../input/text_offset_mapper.ts'

/** Composition 状态输入（不依赖 EditorDtos.ets 的类型，纯结构契约）。 */
export interface EditorCompositionStateInput {
  readonly sessionId: number
  readonly baseRevision: number
  readonly generation: number
  /** UTF-8 byte offset in committed text，半开区间起点。 */
  readonly replaceByteStart: number
  /** UTF-8 byte offset in committed text，半开区间终点。 */
  readonly replaceByteEndExclusive: number
  readonly preeditText: string
  /** preedit 内 UTF-16 code unit offset（IME 协议要求）。 */
  readonly preeditCursorUtf16: number
}

/** EditorSessionSnapshot 输入（不依赖 EditorDtos.ets 的类型，纯结构契约）。 */
export interface EditorSessionSnapshotInput {
  readonly text: string
  readonly cursor: number
  readonly selectionAnchor: number
  readonly revision: number
  readonly generation: number
  readonly composition: EditorCompositionStateInput | null
}

/** Display projection 结果。所有 byte offset 都在 displayText 坐标系下。 */
export interface EditorDisplayProjection {
  readonly text: string
  readonly cursorByteOffset: number
  readonly selectionAnchorByteOffset: number
  readonly selectionHeadByteOffset: number
  readonly compositionStartByteOffset: number | null
  readonly compositionEndByteOffset: number | null
}

/**
 * 把 EditorSessionSnapshot 投影成 display 视图。
 *
 * 无 composition：text=snap.text, cursor/selection 全部用 snap 的 byte offset, compositionStart/End=null。
 *
 * 有 composition：
 *   1. committedText = snap.text
 *   2. startUtf16 = utf8ToUtf16(committedText, replaceByteStart)
 *   3. endUtf16 = utf8ToUtf16(committedText, replaceByteEndExclusive)
 *   4. before = committedText.substring(0, startUtf16)
 *   5. after = committedText.substring(endUtf16)
 *   6. displayText = before + preeditText + after
 *   7. compositionStart = replaceByteStart
 *   8. preeditUtf8Len = TextEncoder().encode(preeditText).length
 *   9. compositionEnd = replaceByteStart + preeditUtf8Len
 *  10. preeditCursorByte = utf16ToUtf8(preeditText, preeditCursorUtf16)
 *  11. displayCaretByte = replaceByteStart + preeditCursorByte
 *  12. cursor/selection 全部 collapse 到 displayCaretByte
 *
 * 保存正文仍只取 committed text（snap.text），不把 preedit 写进文件。
 */
export function projectEditorDisplay(snap: EditorSessionSnapshotInput): EditorDisplayProjection {
  const composition: EditorCompositionStateInput | null = snap.composition
  if (composition === null || composition === undefined) {
    return {
      text: snap.text,
      cursorByteOffset: snap.cursor,
      selectionAnchorByteOffset: snap.selectionAnchor,
      selectionHeadByteOffset: snap.cursor,
      compositionStartByteOffset: null,
      compositionEndByteOffset: null,
    }
  }
  const committedText: string = snap.text
  const startUtf16: number = utf8ToUtf16(committedText, composition.replaceByteStart)
  const endUtf16: number = utf8ToUtf16(committedText, composition.replaceByteEndExclusive)
  const before: string = committedText.substring(0, startUtf16)
  const after: string = committedText.substring(endUtf16)
  const displayText: string = before + composition.preeditText + after
  // compositionStart/End 是 preedit 在显示文本中的 UTF-8 byte 范围。
  // before 部分的 UTF-8 byte 长度 = replaceByteStart（committed text 的前缀不变）。
  const compositionStart: number = composition.replaceByteStart
  const preeditUtf8Len: number = new TextEncoder().encode(composition.preeditText).length
  const compositionEnd: number = composition.replaceByteStart + preeditUtf8Len
  // preeditCursorUtf16（IME 协议要求的 preedit 内 UTF-16 cursor）转成 preedit 内 UTF-8 byte offset，
  // 再加 replaceByteStart 得 displayCaretByte（displayText 中的 byte offset）。
  const preeditCursorByte: number = utf16ToUtf8(composition.preeditText, composition.preeditCursorUtf16)
  const displayCaretByte: number = composition.replaceByteStart + preeditCursorByte
  return {
    text: displayText,
    cursorByteOffset: displayCaretByte,
    selectionAnchorByteOffset: displayCaretByte,
    selectionHeadByteOffset: displayCaretByte,
    compositionStartByteOffset: compositionStart,
    compositionEndByteOffset: compositionEnd,
  }
}
