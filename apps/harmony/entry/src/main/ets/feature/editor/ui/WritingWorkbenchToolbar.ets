// WritingWorkbenchToolbar — 宽屏工作台的顶部工具条带（Core 的 ToolbarLeading /
// ToolbarCenter / ToolbarTrailing 三个角色）。
//
// 为什么需要它：Core 的 resolve_workbench_layout 把顶部整条工具带切成三段
// （ToolbarLeading 靠左固定宽、ToolbarTrailing 靠右固定宽、ToolbarCenter 占中间），
// 下面才是 ChapterNavigation | Editor | ToolPane | ToolRail。
// 宽屏下 HDS 标题栏只有「章节标题 + HeaderTrailing」，既没有 ToolbarCenter，
// 返回/撤销/重做也没有按 ToolbarLeading 组织，所以宽屏自己画这一条带，
// 按 Core 的 bounds 摆；窄屏继续用原来的壳（HeaderTitleBarStack / 自绘标题 Row）。
//
// 三段内容都走真实能力，不摆占位：
//   - Leading：返回（与窄屏同一个 onTitleBack 路径）+ 撤销/重做
//     （EditorSessionCoordinator.undo()/redo()，与输入法的快捷键同一条路径）；
//   - Center：Core 下发的真实格式状态（editorFontSize / editorLineSpacingMultiplier）。
//     Core 的 Writing ScreenPolicy 目前只声明 Sync/Search/Settings，ActionRole 里
//     也没有格式角色，所以这里只显示状态、不摆没有后端的按钮；等 Core 补出运行期
//     格式动作，入口加在这一段即可，条带结构与 bounds 不需要再改；
//   - Trailing：复用现成的 HeaderActionCapsule，动作来源仍是 Core ScreenPolicy 的
//     ActionSlot[] → sortedHeaderTrailingActions → NavigationHeaderActionDispatcher，
//     与窄屏标题栏完全同一条路径，这里不复制一份动作表。

import { ActionSlot } from '../../../corebridge/dto/PlatformDtos'
import { HeaderActionCapsule } from '../../../app/navigation/impl/api23/HeaderActionCapsule'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

/** 工具条带左右内边距（vp），与 HDS 标题栏默认值一致 */
const TOOLBAR_PADDING_HORIZONTAL_VP: number = 16
/** ToolbarLeading / ToolbarTrailing 在 Core 没给 bounds 时的兜底宽度（vp），
 *  与 Core metrics 的 toolbar_leading_width_dp / toolbar_trailing_width_dp 同值，
 *  避免平台自己另定一个阈值 */
const DEFAULT_TOOLBAR_GROUP_WIDTH_VP: number = 200
/** 段内图标按钮边长（vp） */
const TOOLBAR_ICON_SIZE_VP: number = 36

@ComponentV2
export struct WritingWorkbenchToolbar {
  /** Core 的 ToolbarLeading/ToolbarCenter 竖直方向 bounds 高度（vp） */
  @Param toolbarHeightVp: number = 64
  /** Core 的 ToolbarLeading bounds 宽度；<= 0 表示 Core 没给（退回 Core 默认段宽） */
  @Param leadingWidthVp: number = -1
  /** Core 的 ToolbarTrailing bounds 宽度；<= 0 表示 Core 没给（退回 Core 默认段宽） */
  @Param trailingWidthVp: number = -1
  @Param chapterTitle: string = ''
  @Param headerActions: ActionSlot[] = []
  /** Core 下发的真实格式状态，只读显示（Core 目前没有运行期格式动作） */
  @Param fontSizePt: number = 0
  @Param lineSpacing: number = 0
  @Param onBack: () => void = () => {}
  @Param onUndo: () => void = () => {}
  @Param onRedo: () => void = () => {}

  private leadingWidth(): number {
    return this.leadingWidthVp > 0 ? this.leadingWidthVp : DEFAULT_TOOLBAR_GROUP_WIDTH_VP
  }

  private trailingWidth(): number {
    return this.trailingWidthVp > 0 ? this.trailingWidthVp : DEFAULT_TOOLBAR_GROUP_WIDTH_VP
  }

  @Builder
  IconAction(glyph: string, onClick: () => void) {
    Button({ type: ButtonType.Normal }) {
      Text(glyph)
        .fontSize(18)
        .fontColor($r('sys.color.icon_primary'))
    }
    .width(TOOLBAR_ICON_SIZE_VP)
    .height(TOOLBAR_ICON_SIZE_VP)
    .borderRadius(DesignTokens.radiusPill)
    .backgroundColor($r('sys.color.comp_background_gray'))
    .onClick(() => {
      onClick()
    })
  }

  build() {
    Column() {
      Row() {
        // ── ToolbarLeading：返回 + 章节名 + 撤销/重做 ──
        Row() {
          this.IconAction('←', this.onBack)

          Text(this.chapterTitle)
            .fontSize(16)
            .fontWeight(FontWeight.Bold)
            .fontColor($r('sys.color.font_primary'))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)
            .margin({ left: 8, right: 8 })

          // 撤销/重做走 EditorSessionCoordinator，和输入法快捷键同一条事务路径。
          // Core 没有 canUndo/canRedo，栈空时 coordinator 自己 no-op，按钮恒可用。
          this.IconAction('↶', this.onUndo)
          this.IconAction('↷', this.onRedo)
        }
        .width(this.leadingWidth())
        .alignItems(VerticalAlign.Center)

        // ── ToolbarCenter：Core 下发的真实格式状态 ──
        Row() {
          if (this.fontSizePt > 0) {
            Text(`${Math.round(this.fontSizePt)}px · ${this.lineSpacing.toFixed(1)}x`)
              .fontSize(13)
              .fontColor($r('sys.color.font_primary'))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        // Core 的 ToolbarCenter 宽度 = 整条带 - Leading - Trailing，
        // 这里用 layoutWeight(1) 正好等于 Core 给的 bounds，不另写死宽度。
        .layoutWeight(1)
        .alignItems(VerticalAlign.Center)
        .justifyContent(FlexAlign.Center)

        // ── ToolbarTrailing：现有同步 / 搜索 / 设置 ──
        Row() {
          HeaderActionCapsule({ actionSlots: this.headerActions })
        }
        .width(this.trailingWidth())
        .alignItems(VerticalAlign.Center)
        .justifyContent(FlexAlign.End)
      }
      .width('100%')
      .height(this.toolbarHeightVp)
      .alignItems(VerticalAlign.Center)
      .padding({
        left: TOOLBAR_PADDING_HORIZONTAL_VP,
        right: TOOLBAR_PADDING_HORIZONTAL_VP
      })
    }
    .width('100%')
    .height(this.toolbarHeightVp)
    .alignItems(HorizontalAlign.Start)
    .backgroundColor($r('sys.color.background_secondary'))
  }
}