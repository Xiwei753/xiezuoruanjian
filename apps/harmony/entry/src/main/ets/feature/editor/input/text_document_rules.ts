// text_document_rules.ts — 磁盘/平台原文的段落解释（UTF-16 绝对索引）。
// 纯 TS、不依赖 ArkUI，可被 Node 单测直接 import。
//
// 本文件只做**磁盘/平台原文的段落解释**，是 Core
// `core/writer_core/src/editor/text_document_rules.rs` 中 `paragraph_byte_ranges`
// 的 UTF-16 索引镜像：
//   - 分隔符：`\n`（1 个 UTF-16 单元）、`\r\n`（2 个单元，算**一个**段落边界）、
//     `\u2029`（1 个单元）；孤立 `\r` **不是**分隔符。
//   - 段落 range 的 end 不含分隔符（CRLF 两个字符都不属于前一段内容）；
//     连续分隔符产生空段落；末尾分隔符产生末尾空段落；空正文返回空数组。
//
// 导入/粘贴写入正文前的换行与控制字符规范化由 Core `text_document_rules.rs`
// （normalize_inserted_text）与 Qt 入口负责，本文件不复制 normalize 逻辑，
// 也不改写正文；这里只读原文、只返回段落坐标。

// 单个段落在正文中的范围（UTF-16，end 不含分隔符）。
export interface ParagraphRange {
  start: number
  end: number
  hasTrailingNewline: boolean
}

// index 处段落分隔符的 UTF-16 长度；不是分隔符时返回 0。
// 先识别 `\r\n` 再识别 `\r`：孤立 `\r` 长度为 0（正文内容，不是段落边界）。
function separatorLengthAt(text: string, index: number): number {
  const code: number = text.charCodeAt(index)
  if (code === 10) {
    // '\n'
    return 1
  }
  if (code === 13) {
    // '\r'：只有紧跟 '\n' 才构成 CRLF 段落边界。
    if (index + 1 < text.length && text.charCodeAt(index + 1) === 10) {
      return 2
    }
    return 0
  }
  if (code === 0x2029) {
    // U+2029 PARAGRAPH SEPARATOR
    return 1
  }
  return 0
}

// 按统一段落规则切分正文，返回每个段落的 UTF-16 半开 range（不含分隔符）。
// 单次线性扫描 O(n)；空正文返回空数组。
export function listParagraphRanges(text: string): ParagraphRange[] {
  const ranges: ParagraphRange[] = []
  if (text.length === 0) {
    return ranges
  }
  let start: number = 0
  let index: number = 0
  while (index < text.length) {
    const separatorLength: number = separatorLengthAt(text, index)
    if (separatorLength > 0) {
      const range: ParagraphRange = {
        start: start,
        end: index,
        hasTrailingNewline: true
      }
      ranges.push(range)
      index += separatorLength
      start = index
    } else {
      index += 1
    }
  }
  const last: ParagraphRange = {
    start: start,
    end: text.length,
    hasTrailingNewline: false
  }
  ranges.push(last)
  return ranges
}

// insertedText 内每个分隔符之后的绝对 UTF-16 起点（CRLF 只压一次，取分隔符结束处）。
// 语义与旧实现“每个 '\n' 后的绝对位置”一致，但按统一段落规则识别 CRLF 与 U+2029。
export function insertedParagraphStarts(insertedText: string, insertionStartUtf16: number): number[] {
  const starts: number[] = []
  let index: number = 0
  while (index < insertedText.length) {
    const separatorLength: number = separatorLengthAt(insertedText, index)
    if (separatorLength > 0) {
      starts.push(insertionStartUtf16 + index + separatorLength)
      index += separatorLength
    } else {
      index += 1
    }
  }
  return starts
}
