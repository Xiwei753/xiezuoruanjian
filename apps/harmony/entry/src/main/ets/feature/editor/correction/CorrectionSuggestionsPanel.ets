import { MappedCorrectionSuggestion } from './CorrectionDtos'
import { CorrectionIgnoreScope } from './ICorrectionBridge'

/** Compact presentation for Core correction diagnostics and their explicit actions. */
@Component
export struct CorrectionSuggestionsPanel {
  @Prop suggestions: MappedCorrectionSuggestion[] = []
  @Prop isChecking: boolean = false
  @Prop errorMessage: string = ''
  onCheckChapter: (() => void) | null = null
  onApply: ((suggestion: MappedCorrectionSuggestion) => void) | null = null
  onIgnore: ((suggestion: MappedCorrectionSuggestion, scope: CorrectionIgnoreScope) => void) | null = null
  onAddDictionary: ((suggestion: MappedCorrectionSuggestion) => void) | null = null
  @State private selectedSuggestion: MappedCorrectionSuggestion | null = null

  private ignoreSelected(scope: CorrectionIgnoreScope): void {
    const suggestion = this.selectedSuggestion
    if (suggestion !== null) {
      this.onIgnore?.(suggestion, scope)
    }
  }

  private addSelectedToDictionary(): void {
    const suggestion = this.selectedSuggestion
    if (suggestion !== null) {
      this.onAddDictionary?.(suggestion)
    }
  }

  private reasonFor(suggestion: MappedCorrectionSuggestion): string {
    switch (suggestion.ruleId) {
      case 'fixed.should': return '常见固定错词：通常写作“应该”。'
      case 'fixed.already': return '常见固定错词：通常写作“已经”。'
      case 'particle.dei.urgent':
      case 'particle.dei.run':
      case 'particle.dei.slow': return '助词用法提示，请结合句意确认。'
      case 'confusion.custom': return '命中了混淆词词典中的词条。'
      case 'proper.similarity': return '与作品专名存在形近候选。'
      case 'detector.oov': return '该词未命中当前词典，属于统计疑点，请结合上下文判断。'
      case 'detector.char_mad': return '字符级语言模型发现了较少见的搭配，仅作统计提示。'
      case 'detector.candidate': return '替换候选的上下文评分高于原词。'
      default: return '检查到重复或标点规则提示，请结合上下文确认。'
    }
  }

  @Builder
  IgnoreMenu() {
    Menu() {
      if (this.selectedSuggestion !== null) {
        MenuItem({ content: '忽略一次' })
          .onClick(() => this.ignoreSelected('once'))
        MenuItem({ content: '本章忽略' })
          .onClick(() => this.ignoreSelected('chapter'))
        MenuItem({ content: '永久忽略此词' })
          .onClick(() => this.ignoreSelected('permanent'))
        MenuItem({ content: '关闭此规则' })
          .onClick(() => this.ignoreSelected('rule'))
        MenuItem({ content: '加入词典' })
          .onClick(() => this.addSelectedToDictionary())
      }
    }
  }

  build() {
    Column({ space: 8 }) {
      Row({ space: 8 }) {
        Text('智能校对')
          .fontSize(15)
          .fontWeight(FontWeight.Medium)
        if (this.isChecking) {
          LoadingProgress()
            .width(16)
            .height(16)
          Text('正在检查')
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))
        } else if (this.suggestions.length > 0) {
          Text(`发现 ${this.suggestions.length} 条建议`)
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))
        }
        Blank()
        Button('校对本章')
          .height(34)
          .enabled(!this.isChecking)
          .onClick(() => this.onCheckChapter?.())
      }
      .width('100%')

      if (this.errorMessage.length > 0) {
        Text(this.errorMessage)
          .fontSize(12)
          .fontColor($r('sys.color.ohos_id_color_warning'))
          .width('100%')
      }

      if (this.suggestions.length > 0) {
        Scroll() {
          Column({ space: 8 }) {
            ForEach(this.suggestions, (suggestion: MappedCorrectionSuggestion) => {
              Column({ space: 6 }) {
                Row({ space: 8 }) {
                  Text(`“${suggestion.original}” → “${suggestion.replacement}”`)
                    .fontSize(14)
                    .layoutWeight(1)
                  Button('替换')
                    .height(30)
                    .onClick(() => this.onApply?.(suggestion))
                  Button('更多')
                    .height(30)
                    .onClick(() => { this.selectedSuggestion = suggestion })
                    .bindMenu(this.IgnoreMenu)
                }
                Text(`${suggestion.source} · ${suggestion.ruleId} · ${suggestion.severity}`)
                  .fontSize(12)
                  .fontColor($r('sys.color.font_secondary'))
                  .width('100%')
                Text(this.reasonFor(suggestion))
                  .fontSize(12)
                  .fontColor($r('sys.color.font_secondary'))
                  .width('100%')
              }
              .width('100%')
              .padding(10)
              .borderRadius(8)
              .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
            }, (suggestion: MappedCorrectionSuggestion) => `${suggestion.ruleId}_${suggestion.startByte}_${suggestion.endByte}`)
          }
          .width('100%')
        }
        .width('100%')
        .constraintSize({ maxHeight: 180 })
        .scrollBar(BarState.Off)
      }
    }
    .width('100%')
    .padding({ left: 12, right: 12, top: 8, bottom: 8 })
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
