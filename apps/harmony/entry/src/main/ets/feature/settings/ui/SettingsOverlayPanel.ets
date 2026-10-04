// ─── SettingsOverlayPanel.ets ───
// 宽屏设置模态浮层的内容（ArkUI 窗口层级 custom dialog）。
//
// Issue #825 复核第2项：之前的常驻 Stack 宿主只是画在最上面的一层，
// 没有系统返回/侧滑返回语义——设置打开时按返回，事件仍会落到下面的 NavDestination，
// 在写作页甚至直接触发 requestLeave()。现在改成
// getUIContext().getPromptAction().openCustomDialog()：系统负责遮罩、居中和返回拦截，
// 「背景仍是打开设置前的页面」和普通模态是同一个容器语义，不需要手搓第二套导航。
//
// 面板只画内容与底色；遮罩由 openCustomDialog 的 maskColor 提供，这里不再自己画全屏遮罩。
// 内容仍是 SettingsContent 这一份，设置内容 / 保存 / 主题逻辑都继续走现有 SettingsViewModel。
//
// 为什么要单独的组件 + 文件级 @Builder 函数：
// ComponentContent 的构造参数是 wrapBuilder(...)，ArkTS 要求 wrapBuilder 收到的是
// 直接的 @Builder 函数引用，不能是 this.xxx 形式的成员方法引用
// （与 feature/starmap/ui/CreateTitleDialog.ets 同一套写法）。

import { UIContext, ComponentContent } from '@kit.ArkUI'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { SettingsContent } from './SettingsContent'

/**
 * 宽屏设置浮层参数
 */
export interface SettingsOverlayArgs {
  /** 关闭回调：只关对话框，不动下面的 NavPathStack */
  onRequestClose: () => void
  /**
   * 是否两列。
   * Issue #825 复核第3项：Core 在 600–839vp 已经给 Workbench，但一级导航仍是 Bottom，
   * 那一档窗口塞两列设置太窄，所以只有 primaryNavigationPlacement === Side 才两列。
   * 这一档仍然走浮层（overlayLayout 固定 true），只是单列。
   */
  twoColumn: boolean
}

@ComponentV2
struct SettingsOverlayPanel {
  /** 关闭回调：只关对话框 */
  @Param onRequestClose: () => void = () => {}
  /** 是否两列排设置分组 */
  @Param twoColumn: boolean = false

  build() {
    Column() {
      SettingsContent({
        overlayLayout: true,
        twoColumn: this.twoColumn,
        onRequestClose: () => { this.onRequestClose() }
      })
    }
    // ComponentContent 版本的对话框完全跟随内容样式（等价 customStyle=true），
    // 百分比相对窗口尺寸；再叠一层上限，避免超宽屏上拉得过散。
    .width('92%')
    .height('86%')
    .constraintSize({ maxWidth: 1120, maxHeight: 880 })
    .backgroundColor($r('sys.color.background_secondary'))
    .borderRadius(DesignTokens.dialogRadius)
  }
}

/** 文件级 @Builder 函数：把 ComponentContent 传来的参数转交给组件。 */
@Builder
function SettingsOverlayPanelBuilder(args: SettingsOverlayArgs) {
  SettingsOverlayPanel({
    onRequestClose: args.onRequestClose,
    twoColumn: args.twoColumn
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
  return new ComponentContent<SettingsOverlayArgs>(uiContext, wrapBuilder(SettingsOverlayPanelBuilder), args)
}
