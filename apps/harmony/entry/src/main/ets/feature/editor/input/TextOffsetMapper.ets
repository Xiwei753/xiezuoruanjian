// TextOffsetMapper — UTF-8 byte offset ↔ UTF-16 code unit offset 映射工具。
// Core 用 byte offset，ArkTS string 是 UTF-16，必须显式转换。
// 不调 Core，纯函数。
//
// 纯逻辑抽到 text_offset_mapper.ts（可被 Node 单测直接 import）；
// 本 .ets 文件保留 TextOffsetMapper class 静态 API 供 ArkTS 端调用，
// 静态方法体委托给 text_offset_mapper.ts 的函数。

import { utf16ToUtf8 as utf16ToUtf8Impl, utf8ToUtf16 as utf8ToUtf16Impl } from './text_offset_mapper'

export class TextOffsetMapper {
  // UTF-16 code unit offset → UTF-8 byte offset
  static utf16ToUtf8(text: string, utf16Offset: number): number {
    return utf16ToUtf8Impl(text, utf16Offset)
  }

  // UTF-8 byte offset → UTF-16 code unit offset
  static utf8ToUtf16(text: string, utf8Offset: number): number {
    return utf8ToUtf16Impl(text, utf8Offset)
  }
}
