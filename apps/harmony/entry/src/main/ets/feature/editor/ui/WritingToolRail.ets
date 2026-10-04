// Issue #825 复核第 1 项：宽屏写作区最右侧的 ToolRail（Core 七角色之一）。
//
// 草图语义：顶部左 返回/撤销/重做、顶部中 字体/段落/替换、顶部右 同步/搜索/设置，
// 内容区是「章节树 | 正文 | ToolPane | 最右 ToolRail」。同步/搜索/设置属于
// ToolbarTrailing，仍然画在顶部工具栏，本组件不再重复它们。
//
// ToolRail 的职责只有两件事：
//   1. 选哪个工具（切换 ToolPane 承载的内容）；
//   2. 展开 / 收起 ToolPane。
//
// 当前 Harmony 还没有真实的 AI / 星图侧栏内容可接，所以工具入口表是空的——
// 不造假按钮。工具列表非空时，选中项才会自动展开 ToolPane。
//
// 宽度不在这里写死：Core 的 WorkbenchRole.ToolRail bounds 说了算（ToolRail 恒为 56dp）。

import { DesignTokens } from '../../../ui/theme/DesignTokens'

// 一条 rail 入口。icon 为空表示这条入口还没有真实内容，不渲染。
export interface ToolRailEntry {
  key: string
  label: string
  icon: Resource
}

@ComponentV2
export struct WritingToolRail {
  // Core 给的 ToolRail bounds 宽度（dp）。
  @Param railWidthVp: number = 56
  // 当前选中的工具 key；空串表示没选任何工具（ToolPane 由展开/收起入口控制）。
  @Param selectedToolKey: string = ''
  @Param toolPaneVisible: boolean = false
  // 选中工具时回调；空列表时不会被调用。
  @Param onSelectTool: (key: string) => void = () => {}
  // 展开 / 收起 ToolPane。
  @Param onToggleToolPane: () => void = () => {}

  private tools: ToolRailEntry[] = []

  @Builder
  ToolButton(entry: ToolRailEntry) {
    Column({ space: 2 }) {
      Image(entry.icon)
        .width(20)
        .height(20)
        .fillColor($r('sys.color.icon_primary'))

      Text(entry.label)
        .fontSize(10)
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

      // 工具列表为空时，rail 只保留 ToolPane 的展开/收起入口。
      if (this.tools.length === 0) {
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
    }
    .width(this.railWidthVp)
    .height('100%')
    .padding({ top: 8, bottom: 8 })
    .alignItems(HorizontalAlign.Center)
    .backgroundColor($r('sys.color.background_secondary'))
  }
}