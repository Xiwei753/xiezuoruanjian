// editor_patch_carry.ts — 事务 patch 的纯数据投影与逐笔文本搬运。
//
// Issue #879 最新复核评论问题3：字形身份与位置映射都必须按事务链“逐笔搬运”，
// 而不是把某一笔的 UTF-8 坐标当成整条链的坐标。
//
// 本模块只做两件事：
// 1. 把 DisplayPatch 投影成不依赖 DTO 的纯数据（GlyphCarryPatch），
//    使字形身份表 / 位置映射可以在 Node 下直接单测。
// 2. 在一段文本上重放一笔事务的 patches，得到该笔事务结束后的文本。
//
// 纯逻辑：不依赖 ArkUI，也不 import .ets，可被 Node 单测直接 import。

import { utf8ToUtf16, utf16ToUtf8, utf8ByteLength } from '../input/text_offset_mapper.ts'

/**
 * 一笔事务里的一条替换指令——DisplayPatch 的纯数据投影。
 *
 * replaceStartByte/replaceEndByte 是**该笔事务开始前文本**的 UTF-8 byte 范围，
 * insertedText 是替换进去的新文本。
 */
export interface GlyphCarryPatch {
  replaceStartByte: number
  replaceEndByte: number
  insertedText: string
}

/** 在 UTF-16 坐标下的一条替换指令（内部使用）。 */
export interface Utf16CarryPatch {
  replaceStartUtf16: number
  replaceEndUtf16: number
  insertedText: string
}

/**
 * 把一笔事务的 patch 列表转换成 UTF-16 坐标指令。
 *
 * 校验：
 * - replace 范围不超过文本总字节长度；
 * - replace 起止都落在合法 UTF-8 字符边界上（offset 附近没有多字节截断）；
 * - 指令之间不重叠。
 *
 * 返回 null 表示这一笔无法按 UTF-8 边界安全搬运——调用方必须放弃本次动画，
 * 不能用“长度碰巧相等”或线性外推凑出结果。
 */
export function toUtf16CarryPatches(text: string, patches: GlyphCarryPatch[]): Utf16CarryPatch[] | null {
  const byteLen = utf8ByteLength(text)
  const ops: Utf16CarryPatch[] = []
  for (const patch of patches) {
    if (patch.replaceStartByte < 0 || patch.replaceEndByte < patch.replaceStartByte) {
      return null
    }
    if (patch.replaceEndByte > byteLen) {
      return null
    }
    const startUtf16 = utf8ToUtf16(text, patch.replaceStartByte)
    const endUtf16 = utf8ToUtf16(text, patch.replaceEndByte)
    // 合法性：转回来必须还是同一个 byte offset（否则落在多字节字符中间）
    if (utf16ToUtf8(text, startUtf16) !== patch.replaceStartByte) {
      return null
    }
    if (utf16ToUtf8(text, endUtf16) !== patch.replaceEndByte) {
      return null
    }
    ops.push({
      replaceStartUtf16: startUtf16,
      replaceEndUtf16: endUtf16,
      insertedText: patch.insertedText,
    })
  }
  ops.sort((a: Utf16CarryPatch, b: Utf16CarryPatch): number =>
    a.replaceStartUtf16 - b.replaceStartUtf16)
  for (let i = 1; i < ops.length; i++) {
    if (ops[i].replaceStartUtf16 < ops[i - 1].replaceEndUtf16) {
      return null
    }
  }
  return ops
}

/**
 * 在一段文本上重放一笔事务的 patches，返回该笔事务结束后的文本。
 *
 * 返回 null 表示 patch 范围非法（越界 / 非字符边界 / 重叠），调用方不得继续。
 */
export function applyPatchesToText(text: string, patches: GlyphCarryPatch[]): string | null {
  const ops = toUtf16CarryPatches(text, patches)
  if (ops === null) {
    return null
  }
  let result = ''
  let cursor = 0
  for (const op of ops) {
    result += text.substring(cursor, op.replaceStartUtf16)
    result += op.insertedText
    cursor = op.replaceEndUtf16
  }
  result += text.substring(cursor)
  return result
}
