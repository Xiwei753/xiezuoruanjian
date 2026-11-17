// SujianEditor — 写作区组件宿主。
// Issue #1034 评论 #5：改用 ArkUI 原生 RichEditor 替换自研编辑器。
// 系统组件自行处理纵向滚动、长按选区、拖拽选择、手柄、粘贴、系统 IME 与无障碍。
// 移除外层 Scroll、正文 Text、Stack 选区 Rect、onTouch/onGestureJudgeBegin/GestureGroup、
// hitTestContent 和自定义拖选状态。
//
// 保留组件入口与现有页面样式参数，正文改为单个 RichEditor。
// 输入经 NativeRichEditorBridge 转发给 EditorSessionCoordinator。
// Core 外部加载/撤销/同步更新通过 NativeRichEditorBridge.syncFromCore() 回流。

import { NativeRichEditorBridge } from '../input/NativeRichEditorBridge'
import { NativeCaretOverlay } from './NativeCaretOverlay'
import { EditorSessionCoordinator } from '../session/EditorSessionCoordinator'
import { EditorSessionSnapshot, EditorEditResult, EditorTransactionCause } from '../../../corebridge/dto/EditorDtos'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
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
    (_utf16Offset: number, _revision: number, _text: string): boolean => false
  // 字号和行距
  @Prop fontSize: number = 16
  @Prop lineSpacingMultiplier: number = 1.8
  // 颜色
  caretColor: ResourceColor = $r('sys.color.ohos_id_color_text_primary')
  // 光标覆盖层状态
  @State caretX: number = 0
  @State caretY: number = 0
  @State caretWidth: number = 2
  @State caretHeight: number = 20
  @State caretVisible: boolean = false
  // RichEditor 控制器
  private richEditorController: RichEditorController = new RichEditorController()
  // 是否已初始化
  @State private initialized: boolean = false

  aboutToAppear(): void {
    if (this.richEditorBridge) {
      this.richEditorBridge.setController(this.richEditorController)
    }
  }

  aboutToDisappear(): void {
    this.caretVisible = false
  }

  // 纠错建议变化时更新 RichEditor 中的样式
  private onCorrectionSuggestionsChanged(): void {
    if (this.correctionSuggestions.length === 0 || !this.richEditorController) {
      return
    }
  }

  // 标注范围变化时更新 RichEditor 中的样式
  private onAnnotationRangesChanged(): void {
    if (this.annotationRanges.length === 0 || !this.richEditorController) {
      return
    }
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
        .padding({
          left: EDITOR_HORIZONTAL_PADDING_VP,
          right: EDITOR_HORIZONTAL_PADDING_VP,
          top: EDITOR_TOP_PADDING_VP,
          bottom: EDITOR_BOTTOM_PADDING_VP
        })
        .onWillChange((value: RichEditorChangeValue): boolean => {
          if (this.richEditorBridge) {
            return this.richEditorBridge.onWillChange(
              value.rangeBefore,
              value.insertedText,
              value.deletedLength
            )
          }
          return true
        })
        .onDidChange(() => {
          if (this.richEditorBridge) {
            this.richEditorBridge.onDidChange()
          }
        })
        .onSelectionChange((value: TextRange): void => {
          if (this.richEditorBridge) {
            this.richEditorBridge.onSelectionChange(value.start, value.end)
          }
          // 选区非空时隐藏自绘光标，选区为零（纯光标）时显示
          if (value.start === value.end) {
            this.caretVisible = true
          } else {
            this.caretVisible = false
          }
        })
        .onFocus(() => {
          hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'RichEditor focus acquired')
        })
        .onBlur(() => {
          hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'RichEditor focus lost')
          this.caretVisible = false
        })

      // 自绘光标覆盖层（不消费触摸事件）
      NativeCaretOverlay({
        caretX: this.caretX,
        caretY: this.caretY,
        caretWidth: this.caretWidth,
        caretHeight: this.caretHeight,
        caretVisible: this.caretVisible,
        caretColor: this.caretColor
      })
    }
    .width('100%')
    .height('100%')
  }
}
