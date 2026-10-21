// WideToolRail — 宽屏写作工作台最右常驻工具轨（Issue #972）。
//
// 手稿固定为「星图 / AI」两轨 + 底部折叠按钮：
//   - 选中工具与 WritingScreen.selectedToolKey 对齐；
//   - 底部按钮与 toolPaneVisible / onToggleToolPane 对齐；
//   - 右侧工具面板收起后这条 rail 仍然保留，是重新展开面板的入口之一。
// rail 不承载同步 / 搜索 / 设置，那些只在顶部贯通工具条里。

import { DesignTokens } from '../../../../ui/theme/DesignTokens'

interface ToolRailEntry {
  key: string
  label: string
}

@ComponentV2
export struct WideToolRail {
  @Param selectedToolKey: string = 'starmap'
  @Param toolPaneVisible: boolean = false
  @Param onSelectTool: (key: string) => void = () => {}
  @Param onToggleToolPane: () => void = () => {}

  private tools: ToolRailEntry[] = [
    { key: 'starmap', label: '星图' },
    { key: 'ai', label: 'AI' }
  ]

  @Builder
  ToolButton(entry: ToolRailEntry) {
    Column() {
      Text(entry.label)
        .fontSize(entry.key === 'ai' ? 12 : 11)
        .fontWeight(this.selectedToolKey === entry.key ? FontWeight.Bold : FontWeight.Normal)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
    }
    .width('100%')
    .height(52)
    .justifyContent(FlexAlign.Center)
    .borderRadius(DesignTokens.radiusSm)
    .backgroundColor(this.selectedToolKey === entry.key
      ? $r('sys.color.comp_background_gray')
      : Color.Transparent)
    .onClick(() => {
      this.onSelectTool(entry.key)
    })
  }

  build() {
    Column() {
      ForEach(this.tools, (entry: ToolRailEntry) => {
        this.ToolButton(entry)
      }, (entry: ToolRailEntry) => entry.key)

      Blank()

      Button({ type: ButtonType.Normal, stateEffect: true }) {
        Text(this.toolPaneVisible ? '›' : '‹')
          .fontSize(16)
          .fontColor($r('sys.color.icon_secondary'))
      }
      .width(32)
      .height(32)
      .borderRadius(DesignTokens.radiusPill)
      .backgroundColor($r('sys.color.comp_background_gray'))
      .onClick(() => {
        this.onToggleToolPane()
      })
    }
    .width('100%')
    .height('100%')
    .padding({ top: 8, bottom: 8 })
    .alignItems(HorizontalAlign.Center)
    .border({ width: { left: 1 }, color: $r('sys.color.comp_divider') })
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
