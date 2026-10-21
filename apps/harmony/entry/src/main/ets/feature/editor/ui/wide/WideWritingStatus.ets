// WideWritingStatus — 宽屏写作工作台正文列底部状态条（Issue #972）。
//
// 硬约束：只属于正文列底部，不跨章节树 / 工具面板，也不承担页面导航。
// 内容按手稿三段：字/分 · 今日字数 / 目标 · 时钟 + 保存状态。
// 数据全部由 WritingScreen 传入：
//   - charsPerMinute / progressCurrent 来自 Core 写作统计；
//   - clockText 是页面维护的展示时钟；
//   - saveStatus / saveStatusColor 保留现有 getSaveStatusText / getSaveStatusColor 语义。

const STATUS_BAR_CONTENT_HEIGHT_VP: number = 28

@ComponentV2
export struct WideWritingStatus {
  @Param charsPerMinute: number = 0
  @Param progressCurrent: number = 0
  @Param progressTarget: number = 0
  @Param clockText: string = ''
  @Param saveStatus: string = ''
  @Param saveStatusColor: ResourceColor = '#0A59F7'

  private speedText(): string {
    return `${this.charsPerMinute} 字/分`
  }

  private progressText(): string {
    if (this.progressTarget <= 0) {
      return `${this.progressCurrent.toLocaleString()} 字 · 目标未设置`
    }
    return `${this.progressCurrent.toLocaleString()} / ${this.progressTarget.toLocaleString()}`
  }

  build() {
    Column() {
      Column()
        .width('100%')
        .height(1)
        .backgroundColor($r('sys.color.comp_divider'))

      Row() {
        Text(this.speedText())
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))

        Blank()
          .layoutWeight(1)

        Text(this.progressText())
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))

        Blank()
          .layoutWeight(1)

        Row() {
          Text(this.clockText)
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))

          if (this.saveStatus.length > 0) {
            Text(this.saveStatus)
              .fontSize(12)
              .fontColor(this.saveStatusColor)
              .margin({ left: 8 })
          }
        }
        .alignItems(VerticalAlign.Center)
      }
      .width('100%')
      .height(STATUS_BAR_CONTENT_HEIGHT_VP)
      .padding({ left: 16, right: 16 })
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .height(STATUS_BAR_CONTENT_HEIGHT_VP + 1)
    .alignItems(HorizontalAlign.Start)
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
