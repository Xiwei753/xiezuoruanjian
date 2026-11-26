// TextOffsetMapper — UTF-8 byte offset ↔ UTF-16 code unit offset 映射工具。
// Core 用 byte offset，ArkTS string 是 UTF-16，必须显式转换。
// 不调 Core，纯函数。
//
// 纯逻辑抽到 text_offset_mapper.ts（可被 Node 单测直接 import），那里是本端唯一的
// UTF-8/UTF-16 换算实现（含孤立代理项按 WTF-8 3 byte 的统一协议）；
// 本 .ets 文件保留 TextOffsetMapper class 静态 API 供 ArkTS 端调用，
// 静态方法体全部委托给 text_offset_mapper.ts，不再保留第二套算法。

import {
  utf16ToUtf8 as utf16ToUtf8Impl,
  utf16ToUtf8Checked as utf16ToUtf8CheckedImpl,
  utf8ByteLength as utf8ByteLengthImpl,
  utf8ByteOffsetToUtf16 as utf8ByteOffsetToUtf16Impl,
  utf8ToUtf16 as utf8ToUtf16Impl
} from './text_offset_mapper'

export class TextOffsetMapper {
  // UTF-16 code unit offset → UTF-8 byte offset
  static utf16ToUtf8(text: string, utf16Offset: number): number {
    return utf16ToUtf8Impl(text, utf16Offset)
  }

  // Issue #1034 评论 #23：严格版 UTF-16→UTF-8，非法 offset 返回 null
  // （完整代理对中间、非整数、越界；孤立代理项前后是合法边界，按 3 byte 计）。
  static utf16ToUtf8Checked(text: string, utf16Offset: number): number | null {
    return utf16ToUtf8CheckedImpl(text, utf16Offset)
  }

  // UTF-8 byte offset → UTF-16 code unit offset（非严格版，非法位置左对齐/末尾钳位）。
  static utf8ToUtf16(text: string, utf8Offset: number): number {
    return utf8ToUtf16Impl(text, utf8Offset)
  }

  // Issue #1063 评论第 2 条：严格版 byte→UTF-16，非字符边界/越界返回 -1，
  // patch 批次校验用这个版本，不能把非法 byte offset 静默钳位后应用。
  static utf8ByteOffsetToUtf16(text: string, byteOffset: number): number {
    return utf8ByteOffsetToUtf16Impl(text, byteOffset)
  }

  // 文本 UTF-8 byte 长度（与上面的严格边界判定共用同一编码单元协议）。
  static utf8ByteLength(text: string): number {
    return utf8ByteLengthImpl(text)
  }
}
