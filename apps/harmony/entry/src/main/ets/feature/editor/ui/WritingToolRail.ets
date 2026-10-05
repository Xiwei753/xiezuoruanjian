// Issue #829：宽屏写作区最右侧 ToolRail（Core 七角色之一）。
//
// 手稿固定为：顶部工具条 + 左章节树 + 中间正文 + 右工具面板 + 最右窄 rail。
// rail 本身只负责“选哪个工具”和“展开/收起 ToolPane”，不承载同步 / 搜索 / 设置。
// 手稿里最右 rail 明确是“星图 / AI”，所以骨架先固定这两个入口；
// 真实星图 / AI 内容由 ToolPane 后续接入，不再改 rail 的空间结构。
//
// 宽度仍由 Core 的 WorkbenchRole.ToolRail bounds 决定，不在端侧另算断点。

import { DesignTokens } from '../../../ui/theme/DesignTokens'

export interface ToolRailEntry {
  key: string
  label: string
}

@ComponentV2
export struct WritingToolRail {
  @Param railWidthVp: number = 56
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

      Button(this.toolPaneVisible ? '›' : '‹', { type: ButtonType.Normal })
        .width(36)
        .height(36)
        .fontSize(16)
        .fontColor($r('sys.color.font_primary'))
        .backgroundColor($r('sys.color.comp_background_gray'))
        .borderRadius(DesignTokens.radiusPill)
        .onClick(() => {
          this.onToggleToolPane()
        })
    }
    .width(this.railWidthVp)
    .height('100%')
    .padding({ top: 8, bottom: 8 })
    .alignItems(HorizontalAlign.Center)
    .backgroundColor($r('sys.color.background_secondary'))
  }
}