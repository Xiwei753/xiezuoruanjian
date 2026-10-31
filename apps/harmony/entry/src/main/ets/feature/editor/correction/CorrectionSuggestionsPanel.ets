import { MappedCorrectionSuggestion } from './CorrectionDtos'
import { CorrectionIgnoreScope } from './ICorrectionBridge'

/**
 * Correction diagnostics presentation: progress, suggestions and their explicit actions.
 * Model/dictionary resources are managed in settings; this panel only renders proofing.
 */
@Component
export struct CorrectionSuggestionsPanel {
  @Prop suggestions: MappedCorrectionSuggestion[] = []
  @Prop isChecking: boolean = false
  @Prop errorMessage: string = ''
  onCheckChapter: (() => void) | null = null
  onApply: ((suggestion: MappedCorrectionSuggestion) => void) | null = null
  onIgnore: ((suggestion: MappedCorrectionSuggestion, scope: CorrectionIgnoreScope) => void) | null = null
  onAddDictionary: ((suggestion: MappedCorrectionSuggestion) => void) | null = null
  onJump: ((suggestion: MappedCorrectionSuggestion) => void) | null = null
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
      case 'neural.macbert4csc': return '离线神经网络模型根据上下文给出了等长替换建议，请结合语境核对。'
      default: return '检查到重复或标点规则提示，请结合上下文确认。'
    }
  }

  private severityLabel(suggestion: MappedCorrectionSuggestion): string {
    switch (suggestion.severity) {
      case 'error': return '错误'
      case 'warning': return '提醒'
      default: return '参考'
    }
  }

  private severityColor(suggestion: MappedCorrectionSuggestion): ResourceColor {
    switch (suggestion.severity) {
      case 'error': return $r('sys.color.ohos_id_color_alert')
      case 'warning': return $r('sys.color.ohos_id_color_warning')
      default: return $r('sys.color.font_secondary')
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

  @Builder
  SuggestionActions(suggestion: MappedCorrectionSuggestion) {
    Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.End, alignItems: ItemAlign.Center }) {
      Button('跳转')
        .height(30)
        .margin({ left: 8, top: 2, bottom: 2 })
        .onClick(() => this.onJump?.(suggestion))
      Button('采纳')
        .height(30)
        .margin({ left: 8, top: 2, bottom: 2 })
        .onClick(() => this.onApply?.(suggestion))
      Button('更多')
        .height(30)
        .margin({ left: 8, top: 2, bottom: 2 })
        .onClick(() => { this.selectedSuggestion = suggestion })
        .bindMenu(this.IgnoreMenu)
    }
  }

  build() {
    Column({ space: 8 }) {
      Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start, alignItems: ItemAlign.Center }) {
        Text('智能校对')
          .fontSize(15)
          .fontWeight(FontWeight.Medium)
          .margin({ right: 8 })
        if (this.isChecking) {
          LoadingProgress()
            .width(16)
            .height(16)
            .margin({ right: 4 })
          Text('正在检查')
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))
            .margin({ right: 8 })
        } else if (this.suggestions.length > 0) {
          Text(`发现 ${this.suggestions.length} 条建议`)
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))
            .margin({ right: 8 })
        }
        Button('校对本章')
          .height(34)
          .enabled(!this.isChecking)
          .margin({ left: 8 })
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
        Column({ space: 8 }) {
          ForEach(this.suggestions, (suggestion: MappedCorrectionSuggestion) => {
            Column({ space: 6 }) {
              Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Start, alignItems: ItemAlign.Center }) {
                Text(this.severityLabel(suggestion))
                  .fontSize(12)
                  .fontWeight(FontWeight.Medium)
                  .fontColor(this.severityColor(suggestion))
                  .margin({ right: 8 })
                Text(`“${suggestion.original}” → “${suggestion.replacement}”`)
                  .fontSize(14)
                  .layoutWeight(1)
                  .constraintSize({ minWidth: 120 })
              }
              .width('100%')

              Text(`${suggestion.source} · ${suggestion.ruleId} · ${suggestion.severity}`)
                .fontSize(12)
                .fontColor($r('sys.color.font_secondary'))
                .width('100%')
              Text(this.reasonFor(suggestion))
                .fontSize(12)
                .fontColor($r('sys.color.font_secondary'))
                .width('100%')

              this.SuggestionActions(suggestion)
            }
            .width('100%')
            .padding(10)
            .borderRadius(8)
            .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
          }, (suggestion: MappedCorrectionSuggestion) => `${suggestion.ruleId}_${suggestion.startByte}_${suggestion.endByte}`)
        }
        .width('100%')
      }
    }
    .width('100%')
    .padding({ left: 12, right: 12, top: 8, bottom: 8 })
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
