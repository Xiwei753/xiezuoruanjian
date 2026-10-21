// WideWritingToolbar — 宽屏写作工作台顶部贯通工具条（Issue #972）。
//
// 手稿（docs/ui/reference/widescreen/宽屏全打开_202610051638_33443.png）：
//   左：返回 / 撤销 / 重做
//   中：字体 / 段落 / 替换
//   右：同步 / 搜索 / 设置
//
// 这里是普通 ArkUI Button，不再用 HDS 的 HeaderActionCapsule / systemMaterial：
// 宽屏整页由 WideWritingWorkbench 自己排版，顶部是一条贯通工具带，
// 高度由内容自然撑开（约 61vp，接近参考图的 62vp），不按设备写死。
// 所有动作都由 WritingScreen 注入：
//   - onBack / onUndo / onRedo / 字号 / 行距是现有编辑命令；
//   - onHeaderAction 直接转给 NavigationHeaderActionDispatcher（与窄屏页头同一条路径）。

import { ActionRole, ActionSlot } from '../../../../app/navigation/ScreenSemantics'
import { sortedHeaderTrailingActions } from '../../../../app/navigation/NavigationHeaderActionModel'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'

const TOOLBAR_EDGE_PADDING_VP: number = 16
const TOOLBAR_ICON_SIZE_VP: number = 36
const FONT_SIZE_STEP_PT: number = 1
const FONT_SIZE_MIN_PT: number = 10
const FONT_SIZE_MAX_PT: number = 72
const LINE_SPACING_STEP: number = 0.25
const LINE_SPACING_MIN: number = 1
const LINE_SPACING_MAX: number = 3

@ComponentV2
export struct WideWritingToolbar {
  @Param headerActions: ActionSlot[] = []
  /** Core 设置里的真实字号（pt），由 WritingScreen 的编辑中草稿传入。 */
  @Param fontSizePt: number = 0
  /** Core 设置里的真实行距倍率。 */
  @Param lineSpacing: number = 0
  @Param onBack: () => void = () => {}
  @Param onUndo: () => void = () => {}
  @Param onRedo: () => void = () => {}
  @Param onFontSizeChange: (value: number) => void = () => {}
  @Param onLineSpacingChange: (value: number) => void = () => {}
  @Param onHeaderAction: (role: ActionRole) => void = () => {}

  private clampFontSize(value: number): number {
    return Math.min(FONT_SIZE_MAX_PT, Math.max(FONT_SIZE_MIN_PT, Math.round(value)))
  }

  private clampLineSpacing(value: number): number {
    const clamped: number = Math.min(LINE_SPACING_MAX, Math.max(LINE_SPACING_MIN, value))
    return Math.round(clamped * 100) / 100
  }

  private actionLabel(role: ActionRole): string {
    if (role === ActionRole.Sync) {
      return '同步'
    }
    if (role === ActionRole.Search) {
      return '搜索'
    }
    if (role === ActionRole.Settings) {
      return '设置'
    }
    return ''
  }

  private actionIcon(role: ActionRole): Resource {
    if (role === ActionRole.Sync) {
      return $r('app.media.ic_refresh')
    }
    if (role === ActionRole.Search) {
      return $r('app.media.ic_search')
    }
    return $r('app.media.ic_settings')
  }

  @Builder
  PlainIconButton(glyph: string, onTriggered: () => void) {
    Button({ type: ButtonType.Normal, stateEffect: true }) {
      Text(glyph)
        .fontSize(20)
        .fontColor($r('sys.color.icon_primary'))
    }
    .width(TOOLBAR_ICON_SIZE_VP)
    .height(TOOLBAR_ICON_SIZE_VP)
    .borderRadius(DesignTokens.radiusPill)
    .backgroundColor($r('sys.color.comp_background_gray'))
    .margin({ left: 8 })
    .onClick(() => {
      onTriggered()
    })
  }

  @Builder
  Stepper(label: string, valueText: string, onDecrease: () => void, onIncrease: () => void) {
    Row() {
      Text(label)
        .fontSize(13)
        .fontColor($r('sys.color.font_secondary'))
        .margin({ right: 8 })

      Button('−', { type: ButtonType.Normal, stateEffect: true })
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
        .maxLines(1)
        .margin({ left: 8, right: 8 })

      Button('+', { type: ButtonType.Normal, stateEffect: true })
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
    .margin({ right: 20 })
  }

  @Builder
  HeaderActionButton(slot: ActionSlot) {
    Button({ type: ButtonType.Normal, stateEffect: true }) {
      Row() {
        Image(this.actionIcon(slot.role))
          .width(16)
          .height(16)
          .fillColor($r('sys.color.icon_primary'))

        Text(this.actionLabel(slot.role))
          .fontSize(13)
          .fontColor($r('sys.color.font_primary'))
          .margin({ left: 6 })
      }
      .alignItems(VerticalAlign.Center)
    }
    .height(36)
    .padding({ left: 10, right: 10 })
    .borderRadius(DesignTokens.radiusPill)
    .backgroundColor($r('sys.color.comp_background_gray'))
    .margin({ left: 8 })
    .onClick(() => {
      this.onHeaderAction(slot.role)
    })
  }

  build() {
    Column() {
      Row() {
        // ── 左：返回 / 撤销 / 重做 ──
        Row() {
          Button({ type: ButtonType.Normal, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.chevron_left'))
              .fontSize(24)
              .fontColor([$r('sys.color.icon_primary')])
          }
          .width(TOOLBAR_ICON_SIZE_VP)
          .height(TOOLBAR_ICON_SIZE_VP)
          .borderRadius(DesignTokens.radiusPill)
          .backgroundColor($r('sys.color.comp_background_gray'))
          .onClick(() => {
            this.onBack()
          })

          this.PlainIconButton('↶', (): void => {
            this.onUndo()
          })
          this.PlainIconButton('↷', (): void => {
            this.onRedo()
          })
        }
        .alignItems(VerticalAlign.Center)

        // ── 中：字体 / 段落 / 替换 ──
        Row() {
          if (this.fontSizePt > 0) {
            this.Stepper('字体',
              `${Math.round(this.fontSizePt)}px`,
              (): void => {
                this.onFontSizeChange(this.clampFontSize(this.fontSizePt - FONT_SIZE_STEP_PT))
              },
              (): void => {
                this.onFontSizeChange(this.clampFontSize(this.fontSizePt + FONT_SIZE_STEP_PT))
              })

            this.Stepper('段落',
              `${this.lineSpacing.toFixed(2)}x`,
              (): void => {
                this.onLineSpacingChange(this.clampLineSpacing(this.lineSpacing - LINE_SPACING_STEP))
              },
              (): void => {
                this.onLineSpacingChange(this.clampLineSpacing(this.lineSpacing + LINE_SPACING_STEP))
              })
          }

          // 替换命令目前没有会话级实现：保留按钮占位、明确禁用，
          // 不伪造替代表单、不往 Core 写假状态。
          Button('替换', { type: ButtonType.Normal })
            .height(32)
            .padding({ left: 12, right: 12 })
            .fontSize(13)
            .fontColor($r('sys.color.font_secondary'))
            .borderRadius(DesignTokens.radiusPill)
            .backgroundColor($r('sys.color.comp_background_gray'))
            .enabled(false)
            .opacity(0.45)
        }
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
        .alignItems(VerticalAlign.Center)

        // ── 右：同步 / 搜索 / 设置（普通 Button，不走 HDS 胶囊）──
        Row() {
          ForEach(sortedHeaderTrailingActions(this.headerActions), (slot: ActionSlot) => {
            this.HeaderActionButton(slot)
          }, (slot: ActionSlot) => slot.role)
        }
        .alignItems(VerticalAlign.Center)
      }
      .width('100%')
      .padding({
        left: TOOLBAR_EDGE_PADDING_VP,
        right: TOOLBAR_EDGE_PADDING_VP,
        top: 10,
        bottom: 10
      })
      .alignItems(VerticalAlign.Center)

      Column()
        .width('100%')
        .height(1)
        .backgroundColor($r('sys.color.comp_divider'))
    }
    .width('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }
}
