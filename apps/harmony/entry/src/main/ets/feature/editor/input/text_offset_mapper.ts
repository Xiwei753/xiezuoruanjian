// text_offset_mapper.ts — UTF-8 byte offset ↔ UTF-16 code unit offset 映射纯逻辑。
// 不依赖 ArkUI，只依赖 string/number/TextEncoder（全局），可被 Node 单测直接 import。
// 生产由 TextOffsetMapper.ets re-export 后供 ArkTS 端调用；测试由 Node 单测直接 import。
//
// Core 用 byte offset，ArkTS string 是 UTF-16，必须显式转换。
// 不调 Core，纯函数。

/** UTF-16 code unit offset → UTF-8 byte offset。 */
export function utf16ToUtf8(text: string, utf16Offset: number): number {
  if (utf16Offset <= 0) return 0
  const limited = utf16Offset > text.length ? text.length : utf16Offset
  const sub = text.substring(0, limited)
  // JS/ArkTS string 按 UTF-16 编码；转成 UTF-8 byte length。
  return new TextEncoder().encode(sub).length
}

/** UTF-8 byte offset → UTF-16 code unit offset。 */
export function utf8ToUtf16(text: string, utf8Offset: number): number {
  if (utf8Offset <= 0) return 0
  // 逐码点累计 byte 长度，直到达到 utf8Offset。
  let byteLen = 0
  let utf16Index = 0
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i)
    let charByteLen = 1
    if (code < 0x80) {
      charByteLen = 1
    } else if (code < 0x800) {
      charByteLen = 2
    } else if (code >= 0xD800 && code <= 0xDBFF) {
      // 高代理项：UTF-16 surrogate pair 对应 4 字节 UTF-8。
      charByteLen = 4
      i += 1  // 跳过低代理项
    } else {
      charByteLen = 3
    }
    if (byteLen + charByteLen > utf8Offset) {
      return utf16Index
    }
    byteLen += charByteLen
    utf16Index += (charByteLen === 4 ? 2 : 1)
  }
  return utf16Index
}
