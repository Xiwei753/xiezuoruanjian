// WideToolPanel — 宽屏写作工作台右侧可展开工具面板（Issue #972）。
//
// 面板只消费 WritingScreen 已有的 selectedToolKey / toolPaneVisible：
//   - 工具身份来自最右 WideToolRail 的选中项；
//   - 收起动作回到 onToggleToolPane；
// 星图 / AI 的真实内容尚未接入，这里给出明确占位，
// 不伪造业务数据，也不在端侧另造一份星图/AI 状态。

import { DesignTokens } from '../../../../ui/theme/DesignTokens'

const TOOL_PANEL_HEADER_HEIGHT_VP: number = 52

@ComponentV2
export struct WideToolPanel {
  @Param toolKey: string = 'starmap'
  @Param onClose: () => void = () => {}

  private toolTitle(): string {
    return this.toolKey === 'ai' ? 'AI' : '星图'
  }

  private placeholderTitle(): string {
    return this.toolKey === 'ai' ? 'AI 面板尚未接入内容' : '星图面板尚未接入内容'
  }

  private placeholderDescription(): string {
    if (this.toolKey === 'ai') {
      return 'AI 能力尚未启用。接入后在此展示与当前章节相关的操作，接入前不生成占位结果。'
    }
    return '星图内容尚未接入写作页。接入后在此展示当前作品的星图关系，接入前不生成占位数据。'
  }

  build() {
    Column() {
      Row() {
        Text(this.toolTitle())
          .fontSize(15)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))

        Blank()

        Button({ type: ButtonType.Normal, stateEffect: true }) {
          Text('›')
            .fontSize(16)
            .fontColor($r('sys.color.icon_secondary'))
        }
        .width(28)
        .height(28)
        .borderRadius(DesignTokens.radiusPill)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .onClick(() => {
          this.onClose()
        })
      }
      .width('100%')
      .height(TOOL_PANEL_HEADER_HEIGHT_VP)
      .padding({ left: 16, right: 12 })
      .alignItems(VerticalAlign.Center)

      Column()
        .width('100%')
        .height(1)
        .backgroundColor($r('sys.color.comp_divider'))

      Scroll() {
        Column() {
          Column() {
            Text(this.placeholderTitle())
              .fontSize(14)
              .fontWeight(FontWeight.Medium)
              .fontColor($r('sys.color.font_primary'))

            Text(this.placeholderDescription())
              .fontSize(12)
              .fontColor($r('sys.color.font_secondary'))
              .lineHeight(20)
              .margin({ top: 8 })
          }
          .width('100%')
          .padding(16)
          .alignItems(HorizontalAlign.Start)
          .borderRadius(DesignTokens.radiusMd)
          .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
        }
        .width('100%')
        .padding(16)
      }
      .layoutWeight(1)
      .scrollBar(BarState.Off)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
