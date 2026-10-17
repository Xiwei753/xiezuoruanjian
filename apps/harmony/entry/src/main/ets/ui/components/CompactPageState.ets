// CompactPageState — 窄屏页面公共的加载中 / 出错重试 / 空状态视觉。
//
// Issue #932：作品、星图、工作区、统计各自重复画 LoadingProgress(48vp)、错误文本 + 重试按钮、
// 空状态标题 + 说明。这里只画视觉内容，不 import 任何 ViewModel / Core 服务；
// 文案与重试入口由页面传入，业务状态仍归各自页面。
//
// 期望作为 Column / Row / Flex 的直接子项，占满父容器剩余高度（layoutWeight(1)）。
// 宽屏各页的网格卡片形态不经由本组件。

import { DesignTokens } from '../theme/DesignTokens'

/** 公共页面状态类型。 */
export enum CompactPageStateKind {
  Loading = 'loading',
  Error = 'error',
  Empty = 'empty'
}

@ComponentV2
export struct CompactPageState {
  @Param kind: CompactPageStateKind = CompactPageStateKind.Loading
  /** 主文案：加载中提示 / 错误信息 / 空状态标题 */
  @Param title: string = ''
  /** 辅助说明，空串不显示 */
  @Param description: string = ''
  /** 仅错误状态且非 null 时显示重试按钮 */
  @Param onRetry: (() => void) | null = null
  /** 仅空状态可选的系统 symbol 图标；null 不显示 */
  @Param icon: Resource | null = null

  build() {
    Column() {
      if (this.kind === CompactPageStateKind.Loading) {
        LoadingProgress()
          .width(48)
          .height(48)

        if (this.title.length > 0) {
          Text(this.title)
            .fontSize(14)
            .fontColor($r('sys.color.font_secondary'))
            .margin({ top: 12 })
        }
      } else {
        if (this.icon !== null) {
          SymbolGlyph(this.icon)
            .fontSize(64)
            .fontColor([$r('sys.color.icon_primary')])
            .margin({ bottom: 16 })
        }

        if (this.title.length > 0) {
          Text(this.title)
            .fontSize(16)
            .fontColor($r('sys.color.font_primary'))
            .textAlign(TextAlign.Center)
        }

        if (this.description.length > 0) {
          Text(this.description)
            .fontSize(13)
            .fontColor($r('sys.color.font_secondary'))
            .textAlign(TextAlign.Center)
            .margin({ top: 8 })
        }

        if (this.kind === CompactPageStateKind.Error && this.onRetry !== null) {
          Button($r('app.string.retry'))
            .margin({ top: 16 })
            .borderRadius(DesignTokens.inputFieldRadius)
            .onClick(() => {
              if (this.onRetry !== null) {
                this.onRetry()
              }
            })
        }
      }
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
    .padding({ left: 16, right: 16 })
  }
}
