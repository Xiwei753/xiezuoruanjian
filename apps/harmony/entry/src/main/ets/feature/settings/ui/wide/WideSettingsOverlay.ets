// WideSettingsOverlay — 宽屏设置浮层（Issue #972）。
//
// 对照 docs/ui/reference/widescreen/设置页_202610051638_33444.png：
// 在原页面之上居中的大面板，顶部有返回 / 标题「设置」与全宽搜索框，
// 下方是两列可展开的分类卡片，不是占满整屏的 SettingsScreen。
//
// 边界：
// - 窗口级 custom dialog 的打开 / 关闭 / 释放仍由 SettingsOverlayController 唯一持有；
//   本组件只画内容，关闭只回调 onRequestClose，不操作 NavPathStack。
// - 不判断窗口是否宽屏：进入这里就代表壳层已按唯一模式判定选择了浮层。
// - 设置项、保存、主题、同步、诊断等状态与行为仍全部在 SettingsContent / SettingsViewModel，
//   本组件只画外框并把顶部搜索接到全局搜索（Issue #982），不再维护设置页私有搜索词。
// - 面板是窗口层级对话框，关闭后下面的页面、路由栈和滚动位置保持原状。

import { UIContext, ComponentContent } from '@kit.ArkUI'
import { DesignTokens } from '../../../../ui/theme/DesignTokens'
import { SettingsContent } from '../SettingsContent'
import { SettingsSectionContext } from '../../presentation/SettingsSectionContext'

/**
 * 宽屏设置浮层参数
 */
export interface SettingsOverlayArgs {
  /** 关闭回调：只关对话框，不动下面的 NavPathStack */
  onRequestClose: () => void
  /** Issue #982：设置顶部搜索打开全局搜索（originScreen=settings）。 */
  onOpenGlobalSearch: (section: string) => void
}

@ComponentV2
export struct WideSettingsOverlay {
  /** 关闭回调：只关对话框 */
  @Param onRequestClose: () => void = () => {}
  /** Issue #982：顶部搜索统一走全局搜索，不在设置浮层里另做过滤器。 */
  @Param onOpenGlobalSearch: (section: string) => void = (_section: string): void => {}

  // ── 顶部返回 + 标题 ──
  @Builder
  OverlayHeader() {
    Row() {
      Button({ type: ButtonType.Circle, stateEffect: true }) {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(20)
          .height(20)
          .fillColor($r('sys.color.font_primary'))
      }
      .width(40)
      .height(40)
      .backgroundColor($r('sys.color.comp_background_gray'))
      .accessibilityText('关闭设置')
      .onClick(() => {
        this.onRequestClose()
      })

      Text($r('app.string.settings'))
        .fontSize(22)
        .fontWeight(FontWeight.Bold)
        .fontColor($r('sys.color.font_primary'))
        .margin({ left: 12 })
    }
    .width('100%')
    .alignItems(VerticalAlign.Center)
  }

  // ── 全宽搜索框 ──
  @Builder
  OverlaySearchField() {
    Row() {
      Image($r('app.media.ic_search'))
        .width(20)
        .height(20)
        .fillColor($r('sys.color.icon_primary'))
        .margin({ left: 14, right: 8 })

      Text('搜索设置、作品、章节与星图')
        .layoutWeight(1)
        .height(40)
        .fontSize(14)
        .fontColor($r('sys.color.font_secondary'))
        .textAlign(TextAlign.Start)
    }
    .width('100%')
    .height(44)
    // 顶部区间距同样按浮层宽度派生，不针对具体设备写死。
    .margin({ top: '1.5%' })
    .backgroundColor($r('sys.color.comp_background_gray'))
    .borderRadius(DesignTokens.inputFieldRadius)
    .onClick((): void => {
      this.onOpenGlobalSearch(SettingsSectionContext.getCurrentSection())
    })
  }

  build() {
    Column() {
      Column() {
        this.OverlayHeader()
        this.OverlaySearchField()
      }
      .width('100%')
      // 左右留白 = 浮层宽的 11.5%（参考图约 118 / 1027vp）；上下留白同比缩放，
      // 顶部区保持内容自然高度。
      .padding({ left: '11.5%', right: '11.5%', top: '2%', bottom: '2%' })

      // 外框 / 标题 / 搜索归浮层；内容层只负责两列分类卡片与设置项。
      Column() {
        SettingsContent({
          overlayLayout: true,
          overlaySearchText: '',
          onOpenGlobalSearch: this.onOpenGlobalSearch,
          onRequestClose: () => { this.onRequestClose() }
        })
      }
      .width('100%')
      .layoutWeight(1)
    }
    // ComponentContent 版本的对话框完全跟随内容样式（等价 customStyle=true）。
    // 按参考图比例取窗口的 82.4% x 92%，居中由打开对话框的 alignment 保证；
    // 不再叠设备专用上限，宽屏窗口按同一比例自适应。
    .width('82.4%')
    .height('92%')
    .backgroundColor($r('sys.color.background_secondary'))
    .borderRadius(DesignTokens.dialogRadius)
    .clip(true)
  }
}

/** 文件级 @Builder 函数：把 ComponentContent 传来的参数转交给组件。 */
@Builder
function WideSettingsOverlayBuilder(args: SettingsOverlayArgs) {
  WideSettingsOverlay({
    onRequestClose: args.onRequestClose,
    onOpenGlobalSearch: args.onOpenGlobalSearch
  })
}

/**
 * 构造宽屏设置浮层的 ComponentContent。
 * 调用方用 uiContext.getPromptAction().openCustomDialog(content, {...}) 打开，
 * 关闭时用同一个 content 调用 closeCustomDialog。
 *
 * @param uiContext 窗口级 UIContext（由壳层注册给 SettingsOverlayController）
 * @param args 浮层参数
 * @returns 可用于 openCustomDialog / closeCustomDialog 的 ComponentContent
 */
export function createSettingsOverlayContent(
  uiContext: UIContext,
  args: SettingsOverlayArgs
): ComponentContent<SettingsOverlayArgs> {
  return new ComponentContent<SettingsOverlayArgs>(uiContext, wrapBuilder(WideSettingsOverlayBuilder), args)
}
