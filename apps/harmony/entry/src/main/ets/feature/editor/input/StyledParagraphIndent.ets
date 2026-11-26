// StyledParagraphIndent — 正文首行缩进的纯排版样式构造器（Issue #1054）。
//
// 背景：RichEditorParagraphStyle 只有整段缩进 leadingMargin，没有首行缩进 textIndent。
// 首行缩进必须通过属性字符串（StyledString）的 ParagraphStyle.textIndent 表达。
// 本文件只构造视觉样式：
// - 段落切分统一走 text_document_rules.ts（与 Core paragraph_byte_ranges 同规则）：
//   CRLF（两个字符）是一个段落边界、U+2029 也是、`\n` 也是，孤立 CR 不是；
//   连续分隔符保留空段，末尾分隔符产生末尾空段；
// - 缩进量 = fontSize * autoIndentWidth，与 Qt 的 text_indent 语义一致；
// - 不插入任何空格，不改变 UTF-8/UTF-16 正文坐标与 Core 文本。

import { LengthMetrics } from '@kit.ArkUI'
import { ParagraphRange, listParagraphRanges as listDocumentParagraphRanges } from './text_document_rules'

// 单个真实段落在正文中的范围（UTF-16，end 不含段落分隔符；CRLF 两个字符都不计入）。
export interface StyledParagraphRange {
  start: number
  end: number
  // 段落后面是否有分隔符；末尾段为 false。
  hasTrailingNewline: boolean
}

// 首行缩进排版参数。只来自 LocalSettings，不复制业务状态。
export interface StyledParagraphIndentOptions {
  fontSize: number
  autoIndentEnabled: boolean
  autoIndentWidth: number
}

// Qt 端语义：text_indent = fontSize * autoIndentWidth。
export function resolveTextIndentVp(options: StyledParagraphIndentOptions): number {
  if (!options.autoIndentEnabled || options.autoIndentWidth <= 0 || options.fontSize <= 0) {
    return 0
  }
  return options.fontSize * options.autoIndentWidth
}

// 以统一段落规则切分真实段落（CRLF 是一个边界、U+2029 也是、孤立 CR 不是）；
// 保留首段、连续空段和末尾空段。返回的 start/end 是段落内容范围，分隔符不属于段落内容。
// 显式逐字段映射为 StyledParagraphRange，避免 ArkTS 结构类型判定问题。
export function listParagraphRanges(text: string): StyledParagraphRange[] {
  const documentRanges: ParagraphRange[] = listDocumentParagraphRanges(text)
  const ranges: StyledParagraphRange[] = []
  for (let i = 0; i < documentRanges.length; i++) {
    const source: ParagraphRange = documentRanges[i]
    const mapped: StyledParagraphRange = {
      start: source.start,
      end: source.end,
      hasTrailingNewline: source.hasTrailingNewline
    }
    ranges.push(mapped)
  }
  return ranges
}

// 构造整篇正文的 PARAGRAPH_STYLE 占位。
//
// 所有段落共用同一个首行缩进值，因此只需要一条覆盖全文的样式占位：
// 每个段落的段首字符都落在该范围内，新增/拆分段落时新字符也会继承这段样式，
// 不需要在每次换行后重建全文属性字符串（setStyledString 会全量重绘并滚动到末尾）。
// wordBreak / textAlign 与 Issue #1053 的原生排版策略保持一致；
// ArkUI 属性字符串的 ParagraphStyle 没有 lineBreakStrategy 字段，不能伪造该属性。
export function buildParagraphIndentStyles(text: string,
  options: StyledParagraphIndentOptions): StyleOptions[] {
  if (text.length === 0) {
    return []
  }
  const indentVp: number = resolveTextIndentVp(options)
  return [{
    start: 0,
    length: text.length,
    styledKey: StyledStringKey.PARAGRAPH_STYLE,
    styledValue: new ParagraphStyle({
      textAlign: TextAlign.Start,
      wordBreak: WordBreak.BREAK_WORD,
      textIndent: LengthMetrics.vp(indentVp)
    })
  }]
}
