// EditorTextStyleProjector — 把编辑器设置投影成 ArkUI StyledString 的样式。
//
// Issue #768 任务8：当前 Harmony 正文 Text 没有应用 editorLineSpacingMultiplier、
// autoIndentEnabled、autoIndentWidth 设置。Android 端用 TextIndent 处理首行缩进，
// Harmony 端用 StyledString 的 ParagraphStyle({ textIndent }) 和 LineHeightStyle 实现。
//
// 本文件只做设置→样式的投影，不做其他逻辑。

/** 编辑器样式配置输入。 */
export interface EditorStyleConfig {
  /** 字体大小（vp） */
  fontSize: number
  /** 行高倍数（如 1.8） */
  lineSpacingMultiplier: number
  /** 自动首行缩进开关 */
  autoIndentEnabled: boolean
  /** 首行缩进宽度（字符数，如 2.0） */
  autoIndentWidth: number
}

/**
 * 把编辑器设置投影成 ArkUI StyledString 的样式。
 *
 * - 对全文应用 TextStyle({ fontSize })
 * - 对全文应用 LineHeightStyle({ lineHeight = fontSize * lineSpacingMultiplier })
 * - 如果 autoIndentEnabled 为 true，对每个段落首行应用 ParagraphStyle({ textIndent = autoIndentWidth * fontSize })
 * - 如果 autoIndentEnabled 为 false，textIndent 为 0
 */
export class EditorTextStyleProjector {
  /**
   * 把纯文本和设置投影成 MutableStyledString。
   *
   * @param text 纯文本内容
   * @param config 编辑器样式配置
   * @returns 应用了字号、行高、首行缩进样式的 MutableStyledString
   */
  static projectToStyledString(text: string, config: EditorStyleConfig): MutableStyledString {
    // 处理空文本
    if (text.length === 0) {
      return new MutableStyledString('')
    }

    const styledString = new MutableStyledString(text)
    const textLength = text.length

    // 1. 对全文应用 TextStyle（字号）
    const textStyle = new TextStyle({
      fontSize: config.fontSize,
    })
    styledString.setStyle(textStyle, 0, textLength)

    // 2. 对全文应用 LineHeightStyle（行高 = fontSize * lineSpacingMultiplier）
    const lineHeightVp = config.fontSize * config.lineSpacingMultiplier
    const lineHeightStyle = new LineHeightStyle({
      lineHeight: lineHeightVp,
    })
    styledString.setStyle(lineHeightStyle, 0, textLength)

    // 3. 首行缩进：按段落应用 ParagraphStyle
    const indentVp = config.autoIndentEnabled ? config.autoIndentWidth * config.fontSize : 0
    const paragraphStyle = new ParagraphStyle({
      textIndent: indentVp,
    })

    // 按段落范围应用 ParagraphStyle：每个段落是从段落开始到下一个 \n（含 \n）的范围
    let paragraphStart = 0
    for (let i = 0; i < textLength; i++) {
      if (text.charAt(i) === '\n') {
        // 段落范围：[paragraphStart, i+1)，包含 \n
        styledString.setStyle(paragraphStyle, paragraphStart, i + 1)
        paragraphStart = i + 1
      }
    }
    // 最后一个段落（可能没有以 \n 结尾）
    if (paragraphStart < textLength) {
      styledString.setStyle(paragraphStyle, paragraphStart, textLength)
    }

    return styledString
  }
}
