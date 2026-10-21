// WideSettingsSection — 宽屏设置浮层的分类卡片。
//
// Issue #972：宽屏设置从零重写，不再复用窄屏 ExpandableCategory 的列表行外观。
// 本组件只画一张可展开的分类卡片：
//   - 标题 / 当前值摘要 / 展开箭头组成点击区；
//   - 展开内容由调用方通过 @BuilderParam 传入，设置项仍由 SettingsContent 的
//     现有 builder 绘制，不复制任何 ViewModel、保存或同步逻辑。
// 浮层负责两列摆放，卡片本身不决定列宽与滚动；高度由标题 / 摘要 / 展开内容自然撑开，
// 内边距按卡片宽度比例计算，不针对具体设备写固定尺寸。

import { DesignTokens } from '../../../../ui/theme/DesignTokens'

@ComponentV2
export struct WideSettingsSection {
  /** 分类标题（外观 / 编辑器 / 同步等） */
  @Param title: string = ''
  /** 分类当前值摘要；空串不显示 */
  @Param summary: string = ''
  /** 是否展开显示下方设置项 */
  @Param expanded: boolean = false
  /** 点击标题行切换展开 / 收起 */
  @Param onToggle: () => void = () => {}

  @Builder
  emptyContent() {}

  /** 展开后的设置项内容，由调用方提供。 */
  @BuilderParam content: () => void = this.emptyContent

  @Builder
  SectionHeader() {
    Row() {
      Column({ space: 4 }) {
        Text(this.title)
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
          .textAlign(TextAlign.Start)

        if (this.summary.length > 0) {
          Text(this.summary)
            .fontSize(13)
            .fontColor($r('sys.color.font_secondary'))
            .textAlign(TextAlign.Start)
        }
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)

      Image(this.expanded
        ? $r('sys.media.ohos_ic_public_arrow_down')
        : $r('sys.media.ohos_ic_public_arrow_right'))
        .width(20)
        .height(20)
        .fillColor($r('sys.color.font_secondary'))
    }
    .width('100%')
    // 卡片自然撑高：只留比例内边距，不写最小 / 固定高度。
    .padding({ left: '4%', right: '4%', top: '3%', bottom: '3%' })
    .alignItems(VerticalAlign.Center)
    .onClick(() => {
      this.onToggle()
    })
  }

  build() {
    Column() {
      this.SectionHeader()

      if (this.expanded) {
        Column() {
          this.content()
        }
        .width('100%')
        .padding({ left: '4%', right: '4%', bottom: '3%' })
        .animation({ duration: 150, curve: Curve.EaseInOut })
      }
    }
    .width('100%')
    .backgroundColor($r('sys.color.comp_background_gray'))
    .borderRadius(DesignTokens.cardRadius)
    .clip(true)
  }
}
