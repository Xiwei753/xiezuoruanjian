// StarMapStatsSection.ets — Issue #1036 星图创作明细（窄/宽统计共用）。
//
// 只展示 Core 返回的 selected.starMap；正文总字数仍使用同一份 Explorer，
// 星图当前文字 / 操作 / 真实计时以独立明细列出，不重复折算。

import { StarMapStatsSummary } from '../../../corebridge/dto/StatsDtos'
import { formatInt, formatMeasuredDuration } from './StatsFormat'

@Component
export struct StarMapStatsSection {
  @Prop stats: StarMapStatsSummary | null = null

  private hasData(): boolean {
    return this.stats !== null && this.stats.totalRecords > 0
  }

  private topTitle(): string {
    if (this.stats === null || this.stats.starmaps.length === 0) {
      return ''
    }
    return this.stats.starmaps[0].title
  }

  build() {
    Column() {
      if (this.hasData() && this.stats !== null) {
        Column({ space: 6 }) {
          Row() {
            Text('星图创作')
              .fontSize(12)
              .fontWeight(FontWeight.Medium)
              .fontColor($r('sys.color.font_secondary'))
            Blank()
            Text(`当前文字 ${formatInt(this.stats.currentChars)} 字`)
              .fontSize(11)
              .fontColor($r('sys.color.font_tertiary'))
          }
          .width('100%')

          Text(`操作 ${formatInt(this.stats.totalOperations)} 次 · 人工 ${formatInt(this.stats.humanOperations)} · AI ${formatInt(this.stats.aiOperations)} · 前台 ${formatMeasuredDuration(this.stats.activeSeconds)}`)
            .fontSize(11)
            .fontColor($r('sys.color.font_secondary'))
            .width('100%')

          if (this.topTitle().length > 0) {
            Text(`主要星图：${this.topTitle()}`)
              .fontSize(11)
              .fontColor($r('sys.color.font_tertiary'))
              .width('100%')
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        .width('100%')
        .padding({ top: 8 })
      }
    }
    .width('100%')
  }
}
