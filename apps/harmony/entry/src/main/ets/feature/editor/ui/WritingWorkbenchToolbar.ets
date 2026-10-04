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
// Issue #825 复核5：三个角色容器严格占 Core 的 bounds，段间距为 0，
// 条带外层不再加会改变角色几何的 padding；视觉内边距只加在各段内部。
// 章节标题不进 ToolbarLeading（200vp 只放返回/撤销/重做）。
//
// 三段内容都走真实能力，不摆占位：
//   - Leading：返回（与窄屏同一个 onTitleBack 路径）+ 撤销/重做
//     （EditorSessionCoordinator.undo()/redo()，与输入法的快捷键同一条路径）；
//   - Center：字号 / 行距。两者都是 Core 设置里的真实字段
//     （editorFontSize / editorLineSpacingMultiplier），改完走
//     bridge.saveLocalSettings() 落盘，再由 WritingScreen 用当前 coordinator
//     snapshot 重建 EditorLayoutSnapshot。没有后端的入口（如替换）先不摆。
//   - Trailing：复用现成的 HeaderActionCapsule，动作来源仍是 Core ScreenPolicy 的
//     ActionSlot[] → sortedHeaderTrailingActions → NavigationHeaderActionDispatcher，
//     与窄屏标题栏完全同一条路径，这里不复制一份动作表。

import { ActionSlot } from '../../../corebridge/dto/PlatformDtos'
import { HeaderActionCapsule } from '../../../app/navigation/impl/api23/HeaderActionCapsule'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

/** ToolbarLeading / ToolbarTrailing 在 Core 没给 bounds 时的兜底宽度（vp），
 *  与 Core metrics 的 toolbar_leading_width_dp / toolbar_trailing_width_dp 同值，
 *  避免平台自己另定一个阈值 */
const DEFAULT_TOOLBAR_GROUP_WIDTH_VP: number = 200
/** 段内图标按钮边长（vp） */
const TOOLBAR_ICON_SIZE_VP: number = 36
/** 段内视觉内边距（vp）——只影响段内内容，不改角色 bounds */
const GROUP_PADDING_HORIZONTAL_VP: number = 8
const GROUP_PADDING_EDGE_VP: number = 12
/** 字号步进（pt） */
const FONT_SIZE_STEP_PT: number = 1
const FONT_SIZE_MIN_PT: number = 10
const FONT_SIZE_MAX_PT: number = 72
/** 行距步进 */
const LINE_SPACING_STEP: number = 0.25
const LINE_SPACING_MIN: number = 1
const LINE_SPACING_MAX: number = 3

@ComponentV2
export struct WritingWorkbenchToolbar {
  /** Core 的 ToolbarLeading/Center/Trailing 竖直方向 bounds 高度（vp） */
  @Param toolbarHeightVp: number = 64
  /** Core 的 ToolbarLeading bounds 宽度；<= 0 表示 Core 没给（退回 Core 默认段宽） */
  @Param leadingWidthVp: number = -1
  /** Core 的 ToolbarTrailing bounds 宽度；<= 0 表示 Core 没给（退回 Core 默认段宽） */
  @Param trailingWidthVp: number = -1
  @Param headerActions: ActionSlot[] = []
  /** Core 设置里的真实排版字段，当前值 */
  @Param fontSizePt: number = 0
  @Param lineSpacing: number = 0
  @Param onBack: () => void = () => {}
  @Param onUndo: () => void = () => {}
  @Param onRedo: () => void = () => {}
  /** 字号 / 行距变更：WritingScreen 负责 saveLocalSettings + 重建 snapshot */
  @Param onFontSizeChange: (value: number) => void = () => {}
  @Param onLineSpacingChange: (value: number) => void = () => {}

  private leadingWidth(): number {
    return this.leadingWidthVp > 0 ? this.leadingWidthVp : DEFAULT_TOOLBAR_GROUP_WIDTH_VP
  }

  private trailingWidth(): number {
    return this.trailingWidthVp > 0 ? this.trailingWidthVp : DEFAULT_TOOLBAR_GROUP_WIDTH_VP
  }

  private clampFontSize(value: number): number {
    return Math.min(FONT_SIZE_MAX_PT, Math.max(FONT_SIZE_MIN_PT, Math.round(value)))
  }

  private clampLineSpacing(value: number): number {
    const clamped = Math.min(LINE_SPACING_MAX, Math.max(LINE_SPACING_MIN, value))
    return Math.round(clamped * 100) / 100
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

  /** Center 段里的一个「减 / 值 / 加」步进器；label 是字段名，valueText 是当前值文本 */
  @Builder
  Stepper(label: string, valueText: string, onDecrease: () => void, onIncrease: () => void) {
    Row() {
      Text(label)
        .fontSize(13)
        .fontColor($r('sys.color.icon_primary'))
        .margin({ right: GROUP_PADDING_HORIZONTAL_VP })

      Button('−', { type: ButtonType.Normal })
        .width(28)
        .height(28)
        .fontSize(15)
        .fontColor($r('sys.color.icon_primary'))
        .borderRadius(DesignTokens.radiusPill)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .onClick(() => {
          onDecrease()
        })

      Text(valueText)
        .fontSize(13)
        .fontColor($r('sys.color.font_primary'))
        .minFontSize(9)
        .maxFontSize(13)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .margin({ left: GROUP_PADDING_HORIZONTAL_VP, right: GROUP_PADDING_HORIZONTAL_VP })

      Button('+', { type: ButtonType.Normal })
        .width(28)
        .height(28)
        .fontSize(15)
        .fontColor($r('sys.color.icon_primary'))
        .borderRadius(DesignTokens.radiusPill)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .onClick(() => {
          onIncrease()
        })
    }
    .alignItems(VerticalAlign.Center)
  }

  build() {
    Column() {
      Row() {
        // ── ToolbarLeading：返回 + 撤销/重做（200vp 只有这三个入口）──
        Row() {
          this.IconAction('←', this.onBack)

          // 撤销/重做走 EditorSessionCoordinator，和输入法快捷键同一条事务路径。
          // Core 没有 canUndo/canRedo，栈空时 coordinator 自己 no-op，按钮恒可用。
          this.IconAction('↶', this.onUndo)
          this.IconAction('↷', this.onRedo)
        }
        .width(this.leadingWidth())
        .padding({ left: GROUP_PADDING_EDGE_VP, right: GROUP_PADDING_HORIZONTAL_VP })
        .alignItems(VerticalAlign.Center)

        // ── ToolbarCenter：字号 / 行距（都是 Core 设置里的真实字段）──
        Row() {
          if (this.fontSizePt > 0) {
            this.Stepper('字号',
              `${Math.round(this.fontSizePt)}px`,
              (): void => {
                this.onFontSizeChange(this.clampFontSize(this.fontSizePt - FONT_SIZE_STEP_PT))
              },
              (): void => {
                this.onFontSizeChange(this.clampFontSize(this.fontSizePt + FONT_SIZE_STEP_PT))
              })

            this.Stepper('行距',
              `${this.lineSpacing.toFixed(2)}x`,
              (): void => {
                this.onLineSpacingChange(this.clampLineSpacing(this.lineSpacing - LINE_SPACING_STEP))
              },
              (): void => {
                this.onLineSpacingChange(this.clampLineSpacing(this.lineSpacing + LINE_SPACING_STEP))
              })
          }
        }
        // Core 的 ToolbarCenter 宽度 = 整条带 - Leading - Trailing，
        // layoutWeight(1) 正好等于 Core 给的 bounds。
        .layoutWeight(1)
        .padding({ left: GROUP_PADDING_HORIZONTAL_VP, right: GROUP_PADDING_HORIZONTAL_VP })
        .alignItems(VerticalAlign.Center)
        .justifyContent(FlexAlign.Center)

        // ── ToolbarTrailing：现有同步 / 搜索 / 设置 ──
        Row() {
          HeaderActionCapsule({ actionSlots: this.headerActions })
        }
        .width(this.trailingWidth())
        .padding({ left: GROUP_PADDING_HORIZONTAL_VP, right: GROUP_PADDING_EDGE_VP })
        .alignItems(VerticalAlign.Center)
        .justifyContent(FlexAlign.End)
      }
      .width('100%')
      .height(this.toolbarHeightVp)
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .height(this.toolbarHeightVp)
    .alignItems(HorizontalAlign.Start)
    .backgroundColor($r('sys.color.background_secondary'))
  }
}