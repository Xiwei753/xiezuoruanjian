// SujianEditor — 写作区组件宿主。
// Issue #1034 评论 #5：改用 ArkUI 原生 RichEditor 替换自研编辑器。
// Issue #1034 评论 #6/#7/#8 修复：正确使用 RichEditor API、onReady 加载正文、
//   fontSize/lineHeight 通过 span style 应用、移除无效 lineSpacingMultiplier 属性、
//   移除 NativeCaretOverlay 空状态、onPaste 区分粘贴事件。
// Issue #1034 评论 #8 补全：纠错下划线、标注背景/链接下划线、超链接 span 点击回调。
// 系统组件自行处理纵向滚动、长按选区、拖拽选择、手柄、粘贴、系统 IME 与无障碍。

import { NativeRichEditorBridge } from '../input/NativeRichEditorBridge'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { MappedCorrectionSuggestion } from '../correction/CorrectionDtos'
import { AnnotationRangeDto } from '../../../corebridge/dto/AnnotationDtos'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import hilog from '@ohos.hilog'

const EDITOR_LOG_DOMAIN = 0x0000
const EDITOR_LOG_TAG = 'SujianEditor'
const EDITOR_HORIZONTAL_PADDING_VP = 20
const EDITOR_TOP_PADDING_VP = 20
const EDITOR_BOTTOM_PADDING_VP = 32

@Component
export struct SujianEditor {
  editorCoordinator: EditorSessionCoordinator | null = null
  richEditorBridge: NativeRichEditorBridge | null = null
  @Prop @Watch('onCorrectionSuggestionsChanged') correctionSuggestions: MappedCorrectionSuggestion[] = []
  @Prop @Watch('onAnnotationRangesChanged') annotationRanges: AnnotationRangeDto[] = []
  hasHyperlinkAt: (utf16Offset: number, revision: number, text: string) => boolean =
    (_utf16Offset: number, _revision: number, _text: string): boolean => false
  onHyperlinkTap: (utf16Offset: number, revision: number, text: string) => boolean =
    (_utf16Offset: number, _revision: number, _text: string): boolean => false
  @Prop @Watch('onFormatChanged') fontSize: number = 16
  @Prop @Watch('onFormatChanged') lineSpacingMultiplier: number = 1.8
  caretColor: ResourceColor = $r('sys.color.ohos_id_color_text_primary')
  private richEditorController: RichEditorController = new RichEditorController()
  @State private initialized: boolean = false
  private previousCorrectionRanges: number[] = []
  private previousTagBackgroundRanges: number[] = []
  private previousLinkUnderlineRanges: number[] = []

  aboutToDisappear(): void {
    if (this.richEditorBridge) {
      this.richEditorBridge.beginSession()
    }
  }

  // 纠错建议变化时更新 RichEditor 中的样式
  // Issue #1034 评论 #8：通过 updateSpanStyle 实现纠错波浪下划线。
  // 先清除上一轮装饰范围，再应用新一轮。
  private onCorrectionSuggestionsChanged(): void {
    if (!this.richEditorController) {
      return
    }

    const lineHeight: number = this.fontSize * this.lineSpacingMultiplier

    for (let i = 0; i < this.previousCorrectionRanges.length; i += 2) {
      const start = this.previousCorrectionRanges[i]
      const end = this.previousCorrectionRanges[i + 1]
      this.richEditorController.updateSpanStyle({
        start: start,
        end: end,
        textStyle: {
          fontSize: this.fontSize,
          lineHeight: lineHeight,
          decoration: { type: TextDecorationType.None }
        }
      })
    }
    this.previousCorrectionRanges = []

    if (this.correctionSuggestions.length === 0) {
      return
    }

    for (const suggestion of this.correctionSuggestions) {
      this.richEditorController.updateSpanStyle({
        start: suggestion.startUtf16,
        end: suggestion.endUtf16,
        textStyle: {
          fontSize: this.fontSize,
          lineHeight: lineHeight,
          decoration: {
            type: TextDecorationType.Underline,
            color: '#E74C3C',
            style: TextDecorationStyle.WAVY
          }
        }
      })
      this.previousCorrectionRanges.push(suggestion.startUtf16, suggestion.endUtf16)
    }
  }

  // 标注范围变化时更新 RichEditor 中的样式
  // Issue #1034 评论 #8：通过 updateSpanStyle 实现标注背景和链接下划线。
  // Issue #1034 评论 #10：linkUnderline 用 deleteSpans + addTextSpan 重建带 gesture.onClick 的 span；
  //   tagBackground 继续用 updateSpanStyle；程序性改 Span 时隔离编辑事实回调。
  // AnnotationRangeDto 的 startByte/endByte 是 UTF-8，需转换为 UTF-16。
  private onAnnotationRangesChanged(): void {
    if (!this.richEditorController) {
      return
    }

    const lineHeight: number = this.fontSize * this.lineSpacingMultiplier

    if (this.richEditorBridge) {
      this.richEditorBridge.setProgrammaticUpdate(true)
    }

    // 清除上一轮 tagBackground 装饰范围
    for (let i = 0; i < this.previousTagBackgroundRanges.length; i += 2) {
      const start = this.previousTagBackgroundRanges[i]
      const end = this.previousTagBackgroundRanges[i + 1]
      this.richEditorController.updateSpanStyle({
        start: start,
        end: end,
        textStyle: {
          fontSize: this.fontSize,
          lineHeight: lineHeight,
          textBackgroundStyle: { color: Color.Transparent }
        }
      })
    }
    this.previousTagBackgroundRanges = []

    // 清除上一轮 linkUnderline：deleteSpans + addTextSpan 恢复普通文本（无 gesture）
    for (let i = 0; i < this.previousLinkUnderlineRanges.length; i += 2) {
      const start = this.previousLinkUnderlineRanges[i]
      const end = this.previousLinkUnderlineRanges[i + 1]
      const spans = this.richEditorController.getSpans({ start, end })
      let textContent = ''
      for (const span of spans) {
        if ((span as RichEditorTextSpanResult).value !== undefined) {
          const textSpan = span as RichEditorTextSpanResult
          textContent += textSpan.value.substring(textSpan.offsetInSpan[0], textSpan.offsetInSpan[1])
        }
      }
      this.richEditorController.deleteSpans({ start, end })
      if (textContent.length > 0) {
        this.richEditorController.addTextSpan(textContent, {
          offset: start,
          style: {
            fontSize: this.fontSize,
            lineHeight: lineHeight,
            decoration: { type: TextDecorationType.None }
          }
        })
      }
    }
    this.previousLinkUnderlineRanges = []

    if (this.annotationRanges.length === 0) {
      if (this.richEditorBridge) {
        this.richEditorBridge.setProgrammaticUpdate(false)
      }
      return
    }

    const text: string = this.richEditorBridge !== null ? this.richEditorBridge.getCurrentText() : ''
    if (text.length === 0) {
      if (this.richEditorBridge) {
        this.richEditorBridge.setProgrammaticUpdate(false)
      }
      return
    }

    for (const range of this.annotationRanges) {
      const startUtf16 = TextOffsetMapper.utf8ToUtf16(text, range.startByte)
      const endUtf16 = TextOffsetMapper.utf8ToUtf16(text, range.endByte)

      if (range.kind === 'tagBackground') {
        this.richEditorController.updateSpanStyle({
          start: startUtf16,
          end: endUtf16,
          textStyle: {
            fontSize: this.fontSize,
            lineHeight: lineHeight,
            textBackgroundStyle: { color: '#3498DB33' }
          }
        })
        this.previousTagBackgroundRanges.push(startUtf16, endUtf16)
      } else if (range.kind === 'linkUnderline') {
        const spans = this.richEditorController.getSpans({ start: startUtf16, end: endUtf16 })
        let textContent = ''
        for (const span of spans) {
          if ((span as RichEditorTextSpanResult).value !== undefined) {
            const textSpan = span as RichEditorTextSpanResult
            textContent += textSpan.value.substring(textSpan.offsetInSpan[0], textSpan.offsetInSpan[1])
          }
        }
        this.richEditorController.deleteSpans({ start: startUtf16, end: endUtf16 })
        if (textContent.length > 0) {
          const linkOffset = startUtf16
          const linkText = textContent
          this.richEditorController.addTextSpan(textContent, {
            offset: startUtf16,
            style: {
              fontSize: this.fontSize,
              lineHeight: lineHeight,
              decoration: {
                type: TextDecorationType.Underline,
                color: '#3498DB',
                style: TextDecorationStyle.SOLID
              }
            },
            gesture: {
              onClick: () => {
                if (this.richEditorBridge) {
                  const revision = this.richEditorBridge.getLastAppliedRevision()
                  this.onHyperlinkTap(linkOffset, revision, linkText)
                }
              }
            }
          })
        }
        this.previousLinkUnderlineRanges.push(startUtf16, endUtf16)
      }
    }

    if (this.richEditorBridge) {
      this.richEditorBridge.setProgrammaticUpdate(false)
    }
  }

  // 字号/行距变化时更新 RichEditor 的文本样式
  // Issue #1034 评论 #8：updateSpanStyle 官方签名是单参数：
  // updateSpanStyle({ start, end, textStyle: { fontSize, lineHeight } })
  private onFormatChanged(): void {
    if (!this.richEditorController) {
      return
    }
    const lineHeight: number = this.fontSize * this.lineSpacingMultiplier
    this.richEditorController.setTypingStyle({
      fontSize: this.fontSize,
      lineHeight: lineHeight
    })
    this.richEditorController.updateSpanStyle({
      start: 0,
      end: -1,
      textStyle: {
        fontSize: this.fontSize,
        lineHeight: lineHeight
      }
    })
  }

  // Issue #1034 评论 #8：初始化顺序——先加载正文，再应用格式。
  private async initializeEditor(): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.syncFromCore()
    }
    this.onFormatChanged()
    this.onCorrectionSuggestionsChanged()
    this.onAnnotationRangesChanged()
  }

  async syncFromCore(): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.syncFromCore()
    }
  }

  async loadInitialText(text: string, cursorByteOffset: number): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.loadInitialText(text, cursorByteOffset)
      this.initialized = true
    }
  }

  build() {
    Stack() {
      RichEditor({ controller: this.richEditorController })
        .width('100%')
        .height('100%')
        .barState(BarState.Auto)
        .caretColor(this.caretColor)
        .fontSize(this.fontSize)
        .padding({
          left: EDITOR_HORIZONTAL_PADDING_VP,
          right: EDITOR_HORIZONTAL_PADDING_VP,
          top: EDITOR_TOP_PADDING_VP,
          bottom: EDITOR_BOTTOM_PADDING_VP
        })
        .onReady(() => {
          hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'RichEditor onReady')
          if (this.richEditorBridge) {
            this.richEditorBridge.setController(this.richEditorController)
            this.initializeEditor()
          }
        })
        .onWillChange((value: RichEditorChangeValue): boolean => {
          if (this.richEditorBridge) {
            return this.richEditorBridge.onWillChange(value)
          }
          return true
        })
        .onDidChange((rangeBefore: TextRange, rangeAfter: TextRange) => {
          if (this.richEditorBridge) {
            this.richEditorBridge.onDidChange(rangeBefore, rangeAfter)
          }
        })
        .onSelectionChange((value: TextRange): void => {
          if (this.richEditorBridge) {
            this.richEditorBridge.onSelectionChange(value.start, value.end)
          }
        })
        .onPaste(() => {
          if (this.richEditorBridge) {
            this.richEditorBridge.setPendingPaste()
          }
        })
        .enablePreviewText(true)
        .aboutToIMEInput((insertValue: RichEditorInsertValue): boolean => {
          if (this.richEditorBridge) {
            return this.richEditorBridge.aboutToIMEInput(insertValue)
          }
          return true
        })
        .onDidIMEInput((range: TextRange): void => {
          if (this.richEditorBridge) {
            this.richEditorBridge.onDidIMEInput(range)
          }
        })
        .onFocus(() => {
          hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'RichEditor focus acquired')
        })
        .onBlur(() => {
          hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'RichEditor focus lost')
        })
    }
    .width('100%')
    .height('100%')
  }
}
