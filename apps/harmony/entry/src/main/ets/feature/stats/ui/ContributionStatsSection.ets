// ContributionStatsSection — 贡献来源与对象操作（Issue #1010）。
//
// 只展示 Core `ContributionStatsDto` 已经算好的两个口径：
// - 历史操作量：各来源新增/删除/修改字数与操作次数；
// - 当前保留内容来源：当前仍保留文字中各来源的字符数。
// 端侧不重算、不把对象操作折算成码字。

import { ContributionActorStats, ContributionStats } from '../../../corebridge/dto/StatsDtos'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { COMPACT_CARD_PADDING_VP, COMPACT_CARD_SUPPORTING_FONT_VP } from '../../../ui/theme/LayoutTokens'
import { formatInt } from './StatsFormat'

export function contributionActorLabel(actor: string): string {
  if (actor === 'human') {
    return '人工'
  }
  if (actor === 'ai') {
    return 'AI'
  }
  if (actor === 'system') {
    return '系统'
  }
  return '未知'
}

@Component
export struct ContributionStatsSection {
  @Prop stats: ContributionStats | null = null

  private hasData(): boolean {
    return this.stats !== null && this.stats.actors.length > 0
  }

  private actors(): ContributionActorStats[] {
    return this.stats !== null ? this.stats.actors : []
  }

  private retainedTotalChars(): number {
    return this.stats !== null ? this.stats.retainedTotalChars : 0
  }

  private objectOperationText(): string {
    if (this.stats === null) {
      return ''
    }
    const total = this.stats.total
    return `对象操作：新建 ${formatInt(total.createdObjects)} · 更新 ${formatInt(total.updatedObjects)} · 删除 ${formatInt(total.deletedObjects)} · 关联 ${formatInt(total.linkedObjects)} · 解除 ${formatInt(total.unlinkedObjects)}`
  }

  build() {
    Column() {
      if (this.hasData()) {
        Column({ space: 8 }) {
          Row() {
            Text('贡献来源')
              .fontSize(COMPACT_CARD_SUPPORTING_FONT_VP)
              .fontWeight(FontWeight.Medium)
              .fontColor($r('sys.color.font_secondary'))
            Blank()
            Text(`当前保留 ${formatInt(this.retainedTotalChars())} 字`)
              .fontSize(11)
              .fontColor($r('sys.color.font_tertiary'))
          }
          .width('100%')

          ForEach(this.actors(), (actor: ContributionActorStats) => {
            Row() {
              Text(contributionActorLabel(actor.actor))
                .fontSize(13)
                .fontColor($r('sys.color.font_primary'))
                .width(48)
              Text(`操作 ${formatInt(actor.counts.operations)} 次 · 新增 ${formatInt(actor.counts.insertedChars)} 字 · 删除 ${formatInt(actor.counts.deletedChars)} 字`)
                .fontSize(12)
                .fontColor($r('sys.color.font_secondary'))
                .layoutWeight(1)
              Text(`保留 ${formatInt(actor.retainedChars)} 字`)
                .fontSize(12)
                .fontColor($r('sys.color.font_primary'))
                .margin({ left: 8 })
            }
            .width('100%')
          }, (actor: ContributionActorStats) => actor.actor)

          Text(this.objectOperationText())
            .fontSize(11)
            .fontColor($r('sys.color.font_tertiary'))
            .maxLines(2)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)
        .padding(COMPACT_CARD_PADDING_VP)
        .borderRadius(DesignTokens.cardRadius)
        .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
        .margin({ top: COMPACT_CARD_PADDING_VP })
      }
    }
    .width('100%')
  }
}
