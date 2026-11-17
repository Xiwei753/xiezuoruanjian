// SujianEditor — 写作区组件宿主。
// Issue #1034 评论 #5：改用 ArkUI 原生 RichEditor 替换自研编辑器。
// Issue #1034 评论 #6/#7 修复：正确使用 RichEditor API、onReady 加载正文、
//   fontSize/lineHeight 通过 span style 应用、移除无效 lineSpacingMultiplier 属性、
//   移除 NativeCaretOverlay 空状态、onPaste 区分粘贴事件。
// 系统组件自行处理纵向滚动、长按选区、拖拽选择、手柄、粘贴、系统 IME 与无障碍。
// 移除外层 Scroll、正文 Text、Stack 选区 Rect、onTouch/onGestureJudgeBegin/GestureGroup、
// hitTestContent 和自定义拖选状态。

import { NativeRichEditorBridge } from '../input/NativeRichEditorBridge'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { MappedCorrectionSuggestion } from '../correction/CorrectionDtos'
import { AnnotationRangeDto } from '../../../corebridge/dto/AnnotationDtos'
import hilog from '@ohos.hilog'

const EDITOR_LOG_DOMAIN = 0x0000
const EDITOR_LOG_TAG = 'SujianEditor'
const EDITOR_HORIZONTAL_PADDING_VP = 20
const EDITOR_TOP_PADDING_VP = 20
const EDITOR_BOTTOM_PADDING_VP = 32

@Component
export struct SujianEditor {
  // 编辑会话协调器，由父组件注入。
  editorCoordinator: EditorSessionCoordinator | null = null
  // RichEditor 桥接，由父组件注入。
  richEditorBridge: NativeRichEditorBridge | null = null
  // 纠错建议
  @Prop @Watch('onCorrectionSuggestionsChanged') correctionSuggestions: MappedCorrectionSuggestion[] = []
  // 标注范围
  @Prop @Watch('onAnnotationRangesChanged') annotationRanges: AnnotationRangeDto[] = []
  // 超链接回调
  hasHyperlinkAt: (utf16Offset: number, revision: number, text: string) => boolean =
    (_utf16Offset: number, _revision: number, _text: string): boolean => false
  onHyperlinkTap: (utf16Offset: number, revision: number, text: string) => boolean =
    (_utf16Offset: number, _revision: number, _text: string): boolean =><boolean>false
  // 字号和行距
  @Prop @Watch('onFormatChanged') fontSize: number = 16
  @Prop @Watch('onFormatChanged') lineSpacingMultiplier: number = 1.8
  // 颜色
  caretColor: ResourceColor = $r('sys.color.ohos_id_color_text_primary')
  // RichEditor 控制器
  private richEditorController: RichEditorController = new RichEditorController()
  // 是否已初始化
  @State private initialized: boolean = false

  aboutToDisappear(): void {
  }

  // 纠错建议变化时更新 RichEditor 中的样式
  // Issue #1034 评论 #7：后续用 updateSpanStyle 实现纠错下划线
  private onCorrectionSuggestionsChanged(): void {
    if (!this.richEditorController || this.correctionSuggestions.length === 0) {
      return
    }
  }

  // 标注范围变化时更新 RichEditor 中的样式
  // Issue #1034 评论 #7：后续用 updateSpanStyle 实现标注背景
  private onAnnotationRangesChanged(): void {
    if (!this.richEditorController || this.annotationRanges.length === 0) {
      return
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
  // onReady 只调用此方法一次，避免 onFormatChanged 落在空内容上。
  private async initializeEditor(): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.syncFromCore()
    }
    this.onFormatChanged()
  }

  // 从 Core snapshot 同步正文到 RichEditor（程序回放）
  async syncFromCore(): Promise<void> {
    if (this.richEditorBridge) {
      await this.richEditorBridge.syncFromCore()
    }
  }

  // 初始加载正文
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
            // Issue #1034 评论 #8：先 await syncFromCore 加载正文，再应用格式，
            // 避免 onFormatChanged 落在空内容上。
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
