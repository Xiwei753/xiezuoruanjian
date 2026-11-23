// StyledParagraphIndent — 正文首行缩进的纯排版样式构造器（Issue #1054）。
//
// 背景：RichEditorParagraphStyle 只有整段缩进 leadingMargin，没有首行缩进 textIndent。
// 首行缩进必须通过属性字符串（StyledString）的 ParagraphStyle.textIndent 表达。
// 本文件只构造视觉样式：
// - 按 Core 纯文本正文的真实段落（'\n'）切分，覆盖首段和末尾空段；
// - 缩进量 = fontSize * autoIndentWidth，与 Qt 的 text_indent 语义一致；
// - 不插入任何空格，不改变 UTF-8/UTF-16 正文坐标与 Core 文本。

import { LengthMetrics } from '@kit.ArkUI'

// 单个真实段落在正文中的范围（UTF-16，end 不含换行符）。
export interface StyledParagraphRange {
  start: number
  end: number
  // 段落后面是否有硬换行；末尾段为 false。
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

// 以 '\n' 切分真实段落；保留首段、连续空段和末尾空段。
// 返回的 start/end 是段落内容范围，换行符不属于段落内容。
export function listParagraphRanges(text: string): StyledParagraphRange[] {
  const ranges: StyledParagraphRange[] = []
  let paragraphStart = 0
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10) {
      ranges.push({ start: paragraphStart, end: i, hasTrailingNewline: true })
      paragraphStart = i + 1
    }
  }
  ranges.push({ start: paragraphStart, end: text.length, hasTrailingNewline: false })
  return ranges
}

// 构造每个真实段落的 PARAGRAPH_STYLE。
// 段落样式按段首字符定位；空段落（没有字符可缩进）不设置，避免把样式作用到下一段。
// wordBreak / textAlign 与 Issue #1053 的原生排版策略保持一致；
// ArkUI 属性字符串的 ParagraphStyle 没有 lineBreakStrategy 字段，不能伪造该属性。
export function buildParagraphIndentStyles(text: string,
  options: StyledParagraphIndentOptions): StyleOptions[] {
  const styles: StyleOptions[] = []
  if (text.length === 0) {
    return styles
  }
  const indentVp: number = resolveTextIndentVp(options)
  const ranges: StyledParagraphRange[] = listParagraphRanges(text)
  for (let i = 0; i < ranges.length; i++) {
    const range = ranges[i]
    // 段落样式按段首字符定位。空段落（首段为空、连续空行、末尾空段）没有自身字符，
    // 退回承载它的换行符位置；缩进值对所有段落相同，不会改变相邻段落的视觉结果。
    let anchor = range.start
    if (range.end <= range.start) {
      if (anchor >= text.length) {
        anchor = text.length - 1
      }
      if (anchor < 0) {
        continue
      }
    }
    styles.push({
      start: anchor,
      length: 1,
      styledKey: StyledStringKey.PARAGRAPH_STYLE,
      styledValue: new ParagraphStyle({
        textAlign: TextAlign.Start,
        wordBreak: WordBreak.BREAK_WORD,
        textIndent: LengthMetrics.vp(indentVp)
      })
    })
  }
  return styles
}
