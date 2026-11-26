// text_offset_mapper.ts — UTF-8 byte offset ↔ UTF-16 code unit offset 映射纯逻辑。
// 不依赖 ArkUI，只依赖 string/number/TextEncoder（全局），可被 Node 单测直接 import。
// 生产由 TextOffsetMapper.ets re-export 后供 ArkTS 端调用；测试由 Node 单测直接 import。
//
// Core 用 byte offset，ArkTS string 是 UTF-16，必须显式转换。
// 不调 Core，纯函数。
//
// ── 唯一换算协议（Issue #1063 评论第 3 条）─────────────────────────────
// 本文件是鸿蒙端唯一的 UTF-8 byte ↔ UTF-16 code unit 换算实现，其它模块
// （session/editor_patch_logic.ts、TextOffsetMapper.ets）只能委托这里，不得再写第二套。
//
// 字符串按 UTF-16 code unit 逐“编码单元”推进，每单元只可能是：
//   - BMP 非代理项（含 U+0000..U+FFFF 中除代理项外的所有标量值）：UTF-16 1 单元，
//     UTF-8 1/2/3 byte；
//   - 完整代理对（高代理项 D800-DBFF + 紧随的低代理项 DC00-DFFF）：1 个 Unicode
//     scalar value，UTF-16 2 单元，UTF-8 4 byte；
//   - 孤立代理项（单独的高代理项或单独的低代理项，常见于 substring 截断出半个
//     代理对）：按 **WTF-8** 编码为 3 byte，UTF-16 1 单元。
// 孤立代理项取 3 byte 而不是 4 byte 是有意的统一口径：它同时也是 TextEncoder 对
// 孤立代理项的输出长度（编码为 U+FFFD，3 byte），因此本文件与平台 TextEncoder 得到的
// byte 长度一致，不会出现“这里 3 byte、那里 4 byte”的两套算法；孤立高代理项也绝不会
// 被当成完整代理对而吞掉后一个字符。

/**
 * index 处是否是完整代理对的高代理项（与紧随的低代理项组成一个 scalar value）。
 * 孤立高代理项返回 false。
 */
function isSurrogatePairAt(text: string, index: number): boolean {
  if (index < 0 || index + 1 >= text.length) {
    return false
  }
  const high: number = text.charCodeAt(index)
  if (high < 0xD800 || high > 0xDBFF) {
    return false
  }
  const low: number = text.charCodeAt(index + 1)
  return low >= 0xDC00 && low <= 0xDFFF
}

/**
 * index 处编码单元的 UTF-8 byte 长度（1/2/3/4），按本文件唯一协议：
 * 完整代理对 4 byte；孤立代理项（高/低）3 byte；BMP 按标量值范围。
 */
function utf8UnitByteLength(text: string, index: number): number {
  const code: number = text.charCodeAt(index)
  if (code < 0x80) {
    return 1
  }
  if (code < 0x800) {
    return 2
  }
  if (code >= 0xD800 && code <= 0xDBFF) {
    return isSurrogatePairAt(text, index) ? 4 : 3
  }
  // 低代理项（孤立）与其余 BMP 标量值都是 3 byte。
  return 3
}

/** 该编码单元占多少 UTF-16 code unit：完整代理对 2，其余 1。 */
function utf16LengthForUnit(byteLength: number): number {
  return byteLength === 4 ? 2 : 1
}

/** 计算 UTF-8 byte length（纯函数，不依赖平台 TextEncoder）。 */
export function utf8ByteLength(text: string): number {
  let len: number = 0
  let i: number = 0
  while (i < text.length) {
    const unitBytes: number = utf8UnitByteLength(text, i)
    len += unitBytes
    i += utf16LengthForUnit(unitBytes)
  }
  return len
}

/** UTF-16 code unit offset → UTF-8 byte offset。 */
export function utf16ToUtf8(text: string, utf16Offset: number): number {
  if (!Number.isFinite(utf16Offset) || utf16Offset <= 0) {
    return 0
  }
  const limited: number = utf16Offset > text.length ? text.length : utf16Offset
  const sub: string = text.substring(0, limited)
  // JS/ArkTS string 按 UTF-16 编码；转成 UTF-8 byte length。
  return utf8ByteLength(sub)
}

/**
 * UTF-16 code unit offset → UTF-8 byte offset（严格版）。
 * 校验：整数、0 <= offset <= text.length、offset 不落在完整代理对中间。
 * 返回 null 表示 offset 非法（落在完整代理对中间、非整数或超出范围）。
 *
 * 孤立代理项按 WTF-8 视为一个 3 byte 编码单元：孤立代理项之前/之后的 offset
 * 都是合法 UTF-16 边界，与 utf8ByteLength 的 3 byte 口径一致；只有“完整代理对
 * 的高、低两个 code unit 之间”这一个位置非法（那会拆开一个 scalar value）。
 *
 * Issue #1034 评论 #23/#24：选区/编辑/恢复命令必须使用此版本，
 * 避免孤立代理项或非整数位置产生非法 Core 字节边界。
 */
export function utf16ToUtf8Checked(text: string, utf16Offset: number): number | null {
  if (!Number.isInteger(utf16Offset) || utf16Offset < 0 || utf16Offset > text.length) {
    return null
  }
  // 检查 offset 是否落在完整代理对中间：
  // offset-1 是高代理项且 offset 是低代理项 → 完整代理对的中间位置。
  if (utf16Offset > 0 && utf16Offset < text.length) {
    if (isSurrogatePairAt(text, utf16Offset - 1)) {
      return null
    }
  }
  return utf16ToUtf8(text, utf16Offset)
}

/**
 * UTF-8 byte offset → UTF-16 code unit offset（非严格版，方向性钳位）。
 * byteOffset 落在某个编码单元中间或超过文本总 byte 长度时，左对齐到前一个真实
 * 字符边界（越界钳到文本末尾）。只用于“读位置/展示”这类不得失败的老调用点；
 * 需要判定合法性时用 utf8ByteOffsetToUtf16 或 utf16ToUtf8Checked。
 */
export function utf8ToUtf16(text: string, utf8Offset: number): number {
  if (!Number.isFinite(utf8Offset) || utf8Offset <= 0) {
    return 0
  }
  let byteLen: number = 0
  let utf16Index: number = 0
  let i: number = 0
  while (i < text.length) {
    const unitBytes: number = utf8UnitByteLength(text, i)
    if (byteLen + unitBytes > utf8Offset) {
      // 落在该编码单元中间：左对齐到前一个真实字符边界。
      return utf16Index
    }
    byteLen += unitBytes
    utf16Index += utf16LengthForUnit(unitBytes)
    i += utf16LengthForUnit(unitBytes)
    if (byteLen === utf8Offset) {
      return utf16Index
    }
  }
  // 超过文本总 byte 长度：钳位到末尾。
  return utf16Index
}

/**
 * UTF-8 byte offset → UTF-16 code unit offset（严格版）。
 * byteOffset 必须是整数、0 <= byteOffset <= 文本 UTF-8 总 byte 长度，且恰好落在
 * 某个编码单元的起始边界上；否则返回 -1（调用方据此判定 patch 批次非法，不能静默钳位）。
 *
 * 边界判定与 utf8ByteLength 共用同一编码单元口径（孤立代理项 = 3 byte），因此
 * “byte 长度 + 严格边界”两套判断永远不会互相矛盾。
 */
export function utf8ByteOffsetToUtf16(text: string, byteOffset: number): number {
  if (!Number.isInteger(byteOffset) || byteOffset < 0) {
    return -1
  }
  if (byteOffset === 0) {
    return 0
  }
  let byteLen: number = 0
  let utf16Index: number = 0
  let i: number = 0
  while (i < text.length) {
    const unitBytes: number = utf8UnitByteLength(text, i)
    byteLen += unitBytes
    utf16Index += utf16LengthForUnit(unitBytes)
    i += utf16LengthForUnit(unitBytes)
    if (byteLen === byteOffset) {
      return utf16Index
    }
    if (byteLen > byteOffset) {
      // 落在当前编码单元中间：非字符边界。
      return -1
    }
  }
  // 遍历完所有编码单元仍未达到 byteOffset → 越界。
  return -1
}
